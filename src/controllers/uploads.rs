use axum::{http::StatusCode, extract::{Path, State, Query, BodyStream}, response::IntoResponse};
use serde::Deserialize;
use tracing::info;

use crate::{data::helpers::{reject_invalid_container_refs}, ApplicationState};
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

    let upload_lock = application.uploads.create_upload(
        &container_ref, &application.conf.temporary_registry_storage,
        &application.conf.registry_storage
    ).await;
    let upload = upload_lock.read().await;
    info!("Initiating upload for [{}] blob {}", container_ref, upload.id);

    upload.create_parent_directory().await?;

    Ok((
        StatusCode::ACCEPTED,
        [
            ("Location", upload.http_upload_uri()),
            ("Range", "0-0".to_string()),
            ("Docker-Upload-UUID", upload.id.to_string())
        ]
    ).into_response())
}

#[tracing::instrument(skip_all)]
pub async fn delete_upload(
    Path((container_ref, raw_upload_uuid)): Path<(String, String)>,
    State(app): State<ApplicationState>
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;

    let upload_lock = app.uploads
        .fetch_upload_string_uuid(&raw_upload_uuid)
        .await?
        .ok_or_else(|| RegistryHttpError::upload_id_not_found(&raw_upload_uuid))?;

    let upload = upload_lock.read().await;

    // Check container ref then remove
    upload.cleanup_upload().await?;
    app.uploads.delete_upload(upload.id).await;

    Ok((StatusCode::NO_CONTENT, "").into_response())
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
    let seek_position = upload.write_blob(&mut layer).await?;

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
    upload.write_blob(&mut layer).await?;
    upload.finalize_upload(&hash).await?;

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