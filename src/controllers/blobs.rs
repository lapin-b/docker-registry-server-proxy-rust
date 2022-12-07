use std::{io::{self, SeekFrom}, os::unix::prelude::MetadataExt};

use axum::{http::StatusCode, extract::{Path, State, Query, BodyStream}, response::IntoResponse};
use eyre::ContextCompat;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tracing::info;


use crate::{data::helpers::{reject_invalid_container_refs, RegistryPathsHelper}, ApplicationState};
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
    reject_invalid_container_refs(&container_ref)?;

    if query_string.is_some() {
        return Ok((StatusCode::NOT_IMPLEMENTED).into_response());
    }

    let upload_lock = application.uploads.create_upload(&container_ref, &application.conf.temporary_registry_storage).await;
    let upload = upload_lock.read().await;
    info!("Initiating upload for [{}] blob {}", container_ref, upload.id);

    upload.create_containing_directory().await?;

    Ok((
        StatusCode::ACCEPTED,
        [
            ("Location", upload.http_upload_uri()),
            ("Range", "0-0".to_string()),
            ("Docker-Upload-UUID", upload.id.to_string())
        ]
    ).into_response())
}

#[tracing::instrument(skip_all, fields(container_ref = container_ref))]
pub async fn check_blob_exists(
    Path((container_ref, digest)): Path<(String, String)>,
    State(app): State<ApplicationState>
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;

    let (algo, hash) = digest
        .split_once(':')
        .ok_or(RegistryHttpError::invalid_hash_format(&digest))?;

    let file_path = RegistryPathsHelper::blob_path(&app.conf.registry_storage, &container_ref, hash);

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
    State(app): State<ApplicationState>,
    mut layer: BodyStream
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;

    let upload_lock = app.uploads
        .fetch_upload_string_uuid(&raw_upload_uuid)
        .await?
        .ok_or_else(|| RegistryHttpError::upload_id_not_found(&raw_upload_uuid))?;

    let upload = upload_lock.read().await;
    let mut file = upload.create_or_open_upload_file().await?;
    file.seek(io::SeekFrom::End(0)).await.unwrap();

    while let Some(chunk) = layer.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
    }

    let seek_position = file.stream_position().await?;

    Ok((
        StatusCode::ACCEPTED,
        [
            ("Range", format!("0-{}", seek_position)),
            ("Docker-Upload-UUID", upload.id.to_string()),
            ("Location", upload.http_upload_uri()),
            ("Docker-Distribution-Api-Version", "registry/2.0".to_string())
        ]
    ).into_response())
}

#[tracing::instrument(skip_all)]
pub async fn finalize_blob_upload(
    Path((container_ref, raw_upload_uuid)): Path<(String, String)>,
    State(app): State<ApplicationState>,
    Query(DigestQueryString { digest: docker_digest }): Query<DigestQueryString>,
    mut layer: BodyStream
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;

    let (_, hash) = docker_digest
        .split_once(':')
        .ok_or_else(|| RegistryHttpError::invalid_hash_format(&docker_digest))?;

    let upload_lock = app.uploads
        .fetch_upload_string_uuid(&raw_upload_uuid)
        .await?
        .ok_or_else(|| RegistryHttpError::upload_id_not_found(&raw_upload_uuid))?;

    let upload = upload_lock.read().await;

    let mut file = upload.create_or_open_upload_file().await?;
    file.seek(SeekFrom::End(0)).await?;
    while let Some(chunk) = layer.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
    }

    // Enf of upload, close the file and move it to its resting place
    drop(file);

    let blob_file_path = RegistryPathsHelper::blob_path(
        &app.conf.registry_storage,
        &container_ref,
        hash
    );

    let parent = blob_file_path.parent().context("No parent to a constructed path ?")?;
    tokio::fs::create_dir_all(parent).await?;
    tokio::fs::rename(&upload.temporary_file_path, &blob_file_path).await?;

    let upload_id = upload.id;
    app.uploads.delete_upload(upload_id).await;

    Ok((
        StatusCode::CREATED,
        [
            ("Location", format!("/v2/{}/blobs/{}", container_ref, docker_digest)),
            ("Docker-Content-Digest", docker_digest.clone())
        ]
    ).into_response())
}