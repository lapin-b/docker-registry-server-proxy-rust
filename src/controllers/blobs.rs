use std::{io, os::unix::prelude::MetadataExt};

use axum::{http::{StatusCode, Method}, extract::{Path, State}, response::IntoResponse, body::StreamBody};
use futures::stream::{self, StreamExt, Stream};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use tracing::info;

use crate::{data::helpers::{reject_invalid_container_refs, RegistryPathsHelper, self, reject_invalid_tags_refs, file256sum_async}, ApplicationState, docker_client::client::DockerClientError};
use crate::controllers::RegistryHttpResult;

use super::RegistryHttpError;

struct FileWritingStreamHelper<S> {
    file: tokio::fs::File,
    inner_stream: S,
}

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

#[tracing::instrument(skip_all, fields(container_ref = container_ref, digest = digest))]
pub async fn proxy_blob(
    Path((container_ref, digest)): Path<(String, String)>,
    State(app): State<ApplicationState>,
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;
    reject_invalid_tags_refs(&digest)?;

    // Check if we already have the blob file in our cache if we do, send it away
    // without bothering the upstream repository for a new blob. Otherwise, we will
    // have to fetch the upstream to dump the blob in the cache and to the downstream
    // client.

    info!("Checking if there is a cached blob");
    let blob_path = RegistryPathsHelper::blob_path(&app.conf.proxy_storage, &container_ref, &digest);
    if blob_path.is_file() {
        info!("Blob is cached, sending cached version");
        let blob_file = tokio::fs::File::open(&blob_path).await?;
        let blob_size = blob_file.metadata().await?.size();
        let blob_hash = file256sum_async(blob_path.clone()).await??;

        let body_stream = StreamBody::from(ReaderStream::new(blob_file));
        return Ok((
            StatusCode::OK,
            [
                ("Content-Type", "application/octet-stream".to_string()),
                ("Content-Length", blob_size.to_string()),
                ("Docker-Content-Digest", format!("sha256:{}", blob_hash)),
                ("Proxy-Docker-Cache", "HIT".to_string())
            ],
            body_stream
        ).into_response());
    }

    info!("Cache miss, downloading and sending blob");
    // Prepare the file system structure to received the blobs to cache
    tokio::fs::create_dir_all(blob_path.parent().unwrap()).await?;

    let docker_client = app.docker_clients.get_client(&container_ref).await?;
    match docker_client.query_blob(&digest).await {
        Ok(response) => {
            let stream_helper = FileWritingStreamHelper {
                file: tokio::fs::File::create(&blob_path).await?,
                inner_stream: response
                    .raw_response
                    .bytes_stream()
            };

            let downstream_response_stream = stream::unfold(
                stream_helper,
                |mut state| async move {
                    let next_chunk = state.inner_stream.next().await;

                    match next_chunk {
                        Some(Ok(chunk)) => {
                            let result = state
                                .file
                                .write_all(&chunk)
                                .await
                                .map(|_| chunk)
                                .map_err(|e| RegistryHttpError::from(e));

                            Some((result, state))
                        }

                        Some(Err(error)) => {
                            Some((Err(RegistryHttpError::from(error)), state))
                        }

                        None => None
                    }
            });

            return Ok((
                StatusCode::OK,
                [
                    ("Content-Type", "application/octet-stream".to_string()),
                    ("Content-Length", response.content_length.to_string()),
                    //("Docker-Content-Digest", response.hash),
                    ("Proxy-Docker-Cache", "MISS".to_string())
                ],
                StreamBody::new(downstream_response_stream)
            ).into_response())
        },

        Err(DockerClientError::UnexpectedStatusCode(404)) => {
            return Ok(StatusCode::NOT_FOUND.into_response());
        },

        Err(e) => return Err(e.into())
    };
}