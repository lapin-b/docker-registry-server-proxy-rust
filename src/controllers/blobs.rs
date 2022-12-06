use std::{borrow::Cow, io::{self, SeekFrom}, os::unix::prelude::MetadataExt};

use axum::{http::{StatusCode}, headers, extract::{Path, State, TypedHeader, Query, BodyStream}, response::IntoResponse};
use eyre::ContextCompat;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::io::{AsyncSeekExt, BufWriter, AsyncWriteExt};
use tracing::{info, debug};
use uuid::Uuid;

use crate::{data::{upload_in_progress::UploadInProgress, helpers::reject_invalid_container_names}, ApplicationState};
use crate::controllers::RegistryHttpResult;

use super::RegistryHttpError;

#[derive(Deserialize)]
pub struct DigestQueryString {
    pub digest: String
}

#[tracing::instrument(skip_all)]
pub async fn initiate_upload(
    Path(container_ref): Path<String>,
    State(application): State<ApplicationState>,
    query_string: Option<Query<DigestQueryString>>
) -> RegistryHttpResult {
    reject_invalid_container_names(&container_ref)?;

    if query_string.is_some() {
        return Ok((StatusCode::NOT_IMPLEMENTED).into_response());
    }

    let mut uploads = application.uploads.write().await;
    let upload = UploadInProgress::new(&container_ref, &application.configuration.temporary_registry_storage);
    let upload_id = upload.id;
    info!("Initiating upload for [{}] blob {}", container_ref, upload_id);
    uploads.insert(upload_id, upload);
    drop(uploads);

    let uploads = application.uploads.read().await;
    let upload = uploads.get(&upload_id)
        .context("Upload key that just has been inserted doesn't exist")?;
    upload.create_containing_directory().await?;

    Ok((
        StatusCode::ACCEPTED,
        [
            ("Location", upload.http_upload_uri()),
            ("Range", "0-0".to_string()),
            ("Docker-Upload-UUID", upload_id.to_string())
        ]
    ).into_response())
}

#[tracing::instrument(skip_all, fields(container_ref = container_ref))]
pub async fn check_blob_exists(
    Path((container_ref, digest)): Path<(String, String)>,
    State(app_state): State<ApplicationState>
) -> RegistryHttpResult {
    reject_invalid_container_names(&container_ref)?;
    let (algo, hash) = digest
        .split_once(':')
        .ok_or_else(|| RegistryHttpError::InvalidHashFormat(Cow::from(digest.clone())))?;

    let file_path = app_state.configuration
        .registry_storage
        .join(container_ref)
        .join("blobs")
        .join(hash);

    info!("Checking if path [{:?}] exists", file_path);

    let file_metadata = match tokio::fs::metadata(&file_path).await {
        Ok(metadata) => {
            info!("File exists and is accessible");
            metadata
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            info!("File not found, returning 404");
            return Ok((StatusCode::NOT_FOUND).into_response())
        }
        Err(e) => return Err(e.into())
    };

    Ok((
        StatusCode::OK,
        [
            ("Content-Length", file_metadata.size().to_string()),
            ("Docker-Content-Digest", [algo, hash].join(":"))
        ]
    ).into_response())
}

#[tracing::instrument(skip_all)]
pub async fn process_blob_chunk_upload(
    Path((container_ref, raw_upload_uuid)): Path<(String, String)>,
    State(application): State<ApplicationState>,
    range: Option<TypedHeader<headers::ContentRange>>,
    mut layer: BodyStream
) -> RegistryHttpResult {
    let upload_uuid = Uuid::try_parse(&raw_upload_uuid)?;
    let upload_map = application.uploads.read().await;

    // Récupérer les infos sur le fichier temporaire et le bordel
    let upload_entry = upload_map.get(&upload_uuid)
        .ok_or_else(|| RegistryHttpError::InvalidUploadId(Cow::from(raw_upload_uuid.clone())))?;

    let mut file = if upload_entry.temporary_file.is_file() {
        tokio::fs::File::open(&upload_entry.temporary_file).await?
    } else {
        tokio::fs::File::create(&upload_entry.temporary_file).await?
    };

    file.seek(io::SeekFrom::End(0)).await.unwrap();
    let seek_position = file.stream_position().await.unwrap();
    let initial_file_size = file.metadata().await?.size();

    let start_range = range
        .clone()
        .map(
            |header| header.0.bytes_range()
                .map(|(start, _)| start)
                .unwrap_or_default()
        )
        .unwrap_or_default();

    debug!("Initial condition: seek position {}, file size {}, initial range {}, full range {:?}", seek_position, initial_file_size, start_range, range);
    while let Some(chunk) = layer.next().await {
        let chunk = chunk?;
        file.write(&chunk).await?;
    }
    let seek_position = file.stream_position().await.unwrap();
    let final_file_size = file.metadata().await.unwrap().size();
    debug!("Final condition: seek position {}, file size {}, initial range {}, full range {:?}", seek_position, final_file_size, start_range, range);

    Ok((
        StatusCode::ACCEPTED,
        [
            ("Range", format!("0-{}", seek_position)),
            ("Docker-Upload-UUID", raw_upload_uuid),
            ("Location", upload_entry.http_upload_uri()),
            ("Docker-Distribution-Api-Version", "registry/2.0".to_string())
        ]
    ).into_response())
}

#[tracing::instrument(skip_all)]
pub async fn finalize_blob_upload(
    Path((container_ref, raw_upload_uuid)): Path<(String, String)>,
    State(application): State<ApplicationState>,
    range: Option<TypedHeader<headers::ContentRange>>,
    Query(digest): Query<DigestQueryString>,
    mut layer: BodyStream
) -> RegistryHttpResult {
    let (_, hash) = digest.digest
        .split_once(':')
        .ok_or_else(|| RegistryHttpError::InvalidHashFormat(Cow::from(digest.digest.clone())))?;

    let upload_uuid = Uuid::try_parse(&raw_upload_uuid)?;
    let upload_map = application.uploads.read().await;

    let upload_entry = upload_map.get(&upload_uuid)
        .ok_or_else(|| RegistryHttpError::InvalidUploadId(Cow::from(raw_upload_uuid.clone())))?;

    let mut file = if upload_entry.temporary_file.is_file() {
        tokio::fs::File::open(&upload_entry.temporary_file).await?
    } else {
        tokio::fs::File::create(&upload_entry.temporary_file).await?
    };

    file.seek(SeekFrom::End(0)).await?;

    while let Some(chunk) = layer.next().await {
        let chunk = chunk?;
        file.write(&chunk).await?;
    }

    // Enf of upload, close the file and move it to its resting place
    drop(file);

    let blob_file_path = application.configuration.registry_storage
        .join(&container_ref)
        .join("blobs")
        .join(&hash);

    let parent = blob_file_path.parent().context("No parent to a constructed path ?")?;
    tokio::fs::create_dir_all(parent).await?;

    tokio::fs::rename(&upload_entry.temporary_file, &blob_file_path).await?;

    Ok((
        StatusCode::CREATED,
        [
            ("Location", format!("/v2/{}/blobs/{}", container_ref, digest.digest)),
            ("Docker-Content-Digest", digest.digest.clone())
        ]
    ).into_response())
}