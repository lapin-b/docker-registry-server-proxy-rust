use std::{borrow::Cow, io, os::unix::prelude::MetadataExt};

use axum::{http::StatusCode, extract::{Path, State}, response::IntoResponse};
use eyre::ContextCompat;
use tracing::info;

use crate::{data::{upload_in_progress::UploadInProgress, helpers::reject_invalid_container_names}, ApplicationState};
use crate::controllers::RegistryHttpResult;

use super::RegistryHttpError;

#[tracing::instrument(skip_all)]
pub async fn initiate_upload(
    Path(container_ref): Path<String>, 
    State(application): State<ApplicationState>,
) -> RegistryHttpResult {
    reject_invalid_container_names(&container_ref)?;

    let mut uploads = application.uploads.write().await;
    let upload = UploadInProgress::new(&container_ref, &application.configuration.temporary_registry_storage);
    let upload_id = upload.id;
    tracing::info!("Initiating upload for [{}] blob {}", container_ref, upload_id);
    uploads.insert(upload_id, upload);
    drop(uploads);

    let uploads = application.uploads.read().await;
    let upload = uploads.get(&upload_id)
        .context("Upload key that just has been inserted doesn't exist")?;
    upload.create_containing_directory().await?;

    Ok((
        StatusCode::CREATED,
        [
            ("Location", upload.http_upload_uri()),
            ("Range", "0-0".to_string()),
            ("Docker-Upload-UUID", upload_id.to_string())
        ]
    ).into_response())
}

#[tracing::instrument(skip_all)]
pub async fn check_blob_exists(
    Path((container_ref, digest)): Path<(String, String)>,
    State(app_state): State<ApplicationState>
) -> RegistryHttpResult {
    reject_invalid_container_names(&container_ref)?;
    let (algo, hash) = digest
        .split_once(':')
        .ok_or_else(|| RegistryHttpError::InvalidHashFormat(Cow::from(digest.clone())))?;
    
    info!("Looking for blob [{}] in [{}]", container_ref, hash);

    let file_path = app_state.configuration
        .registry_storage
        .join(container_ref)
        .join("blobs")
        .join(hash);

    info!("Checking {:?}", file_path);

    let file_metadata = match tokio::fs::metadata(&file_path).await {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
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
