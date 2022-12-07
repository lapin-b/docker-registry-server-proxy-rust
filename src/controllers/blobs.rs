use std::{io, os::unix::prelude::MetadataExt};

use axum::{http::{StatusCode, Method}, extract::{Path, State}, response::IntoResponse, body::StreamBody};
use tracing::info;

use crate::{data::helpers::{reject_invalid_container_refs, RegistryPathsHelper, self}, ApplicationState};
use crate::controllers::RegistryHttpResult;

use super::RegistryHttpError;

#[tracing::instrument(skip_all, fields(container_ref = container_ref))]
pub async fn check_blob_exists(
    Path((container_ref, digest)): Path<(String, String)>,
    http_method: Method,
    State(app): State<ApplicationState>
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;

    let (_algo, hash) = digest
        .split_once(':')
        .ok_or(RegistryHttpError::invalid_hash_format(&digest))?;

    let file_path = RegistryPathsHelper::blob_path(&app.conf.registry_storage, &container_ref, hash);
    info!("Checking if path [{:?}] exists", file_path);
    let blob_file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => {
            info!("File exists and is accessible"); 
            f
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            info!("File not found, returning 404");
            return Ok((StatusCode::NOT_FOUND).into_response())
        }
        Err(e) => return Err(e.into())
    };

    let blob_size = blob_file.metadata().await?.size();

    if http_method == Method::HEAD {
        return Ok((
            StatusCode::OK,
            [
                ("Content-Length", blob_size.to_string()),
                ("Docker-Content-Digest", format!("sha256:{}", hash))
            ]
        ).into_response());
    }

    // The client really wants the blob, send it away and calculate the real hash !
    let blob_sha256 = helpers::file256sum_async(file_path.clone()).await??;
    let response_body = StreamBody::new(tokio_util::io::ReaderStream::new(blob_file));

    Ok((
        StatusCode::OK,
        [
            ("Content-Type", "application/octet-stream".to_string()),
            ("Content-Length", blob_size.to_string()),
            ("Docker-Content-Digest", format!("sha256:{}", blob_sha256))
        ],
        response_body
    ).into_response())
}

