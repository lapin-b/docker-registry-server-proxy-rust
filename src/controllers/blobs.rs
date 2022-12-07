use std::{io, os::unix::prelude::MetadataExt};

use axum::{http::StatusCode, extract::{Path, State}, response::IntoResponse};
use tracing::info;

use crate::{data::helpers::{reject_invalid_container_refs, RegistryPathsHelper}, ApplicationState};
use crate::controllers::RegistryHttpResult;

use super::RegistryHttpError;

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

