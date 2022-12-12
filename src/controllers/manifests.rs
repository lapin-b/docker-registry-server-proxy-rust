use std::os::unix::prelude::MetadataExt;

use axum::{response::IntoResponse, extract::{Path, BodyStream, State}, TypedHeader, headers, http::StatusCode, body::StreamBody};

use tokio_util::io::ReaderStream;
use tracing::{info, warn};

use crate::{data::{helpers::{reject_invalid_container_refs, RegistryPathsHelper, reject_invalid_tags_refs}, manifests::{Manifest, ManifestMetadata}}, ApplicationState, docker_client::client::DockerClientError};
use crate::controllers::RegistryHttpResult;

use super::RegistryHttpError;

#[tracing::instrument(skip_all, fields(container_ref = container_ref, manifest_ref = manifest_ref))]
pub async fn upload_manifest(
    Path((container_ref, manifest_ref)): Path<(String, String)>,
    TypedHeader(content_type): TypedHeader<headers::ContentType>,
    State(app): State<ApplicationState>,
    mut body: BodyStream
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;
    reject_invalid_tags_refs(&manifest_ref)?;

    let mut manifest = Manifest::new(
        &app.conf.registry_storage, 
        &app.conf.temporary_registry_storage,
        &container_ref, 
        &manifest_ref
    );

    info!("Saving manifest");
    manifest.save_manifest((&mut body).into()).await?;
    info!("Saving metadata");
    manifest.save_manifest_metadata(&content_type.to_string()).await?;

    Ok((
        StatusCode::CREATED,
        [
            ("Location", format!("/v2/{}/manifests/{}", container_ref, manifest_ref)),
            ("Docker-Content-Digest", manifest.docker_hash()?.clone())
        ]
    ).into_response())
}

#[tracing::instrument(skip_all)]
pub async fn fetch_manifest(
    Path((container_ref, manifest_ref)): Path<(String, String)>,
    State(app): State<ApplicationState>,
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;
    reject_invalid_tags_refs(&manifest_ref)?;

    let manifest_path = RegistryPathsHelper::manifest_path(&app.conf.registry_storage, &container_ref, &manifest_ref);
    let manifest_file = match tokio::fs::File::open(&manifest_path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RegistryHttpError::manifest_not_found(&container_ref, &manifest_ref));
        }
        Err(e) => return Err(e.into())
    };
    let manifest_size = manifest_file.metadata().await?.size();

    let manifest_meta_path = RegistryPathsHelper::manifest_meta(&app.conf.registry_storage, &container_ref, &manifest_ref);
    let manifest_meta = tokio::fs::read_to_string(&manifest_meta_path).await?;
    let manifest_meta = serde_json::from_str::<ManifestMetadata>(&manifest_meta).unwrap();

    let manifest_stream = StreamBody::new(tokio_util::io::ReaderStream::new(manifest_file));

    Ok((
        StatusCode::OK,
        [
            ("Docker-Content-Digest", format!("sha256:{}", manifest_meta.hash)),
            ("Content-Type", manifest_meta.content_type.to_string()),
            ("Content-Length", manifest_size.to_string())
        ],
        manifest_stream
    ).into_response())
}

#[tracing::instrument(skip_all, fields(container_ref = container_ref, manifest_ref = manifest_ref))]
pub async fn proxy_fetch_manifest(
    Path((container_ref, manifest_ref)): Path<(String, String)>,
    State(app): State<ApplicationState>,
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;
    reject_invalid_tags_refs(&manifest_ref)?;

    // TODO: Rearrange code to support offline proxying, that is if the upstream proxy did send 429 or any 5xx HTTP code
    let client = app.docker_clients.get_client(&container_ref).await?;
    info!("Querying upstream HEAD to fetch the most manifest related to the tag");

    let (proxy_hash, content_length, content_type) = match client.query_manifest(&manifest_ref, true).await {
        // The ideal case: the server returns a 200 on the HEAD HTTP request
        Ok(proxy_response_head) => {
            info!("Upstream returned 200 on the HEAD. Checking for cached hash file {}", proxy_response_head.hash);

            // Check if we have the same copy of the manifest somewhere in our files before sending a GET request
            // to the upstream respository.
            let proxy_manifest_hash_path = RegistryPathsHelper::manifest_path(&app.conf.proxy_storage, &container_ref, &proxy_response_head.hash);
            if !proxy_manifest_hash_path.is_file() {
                info!("File does not exist. Querying and caching the upstream manifest");
                // We don't have the manifest, GET the manifest referenced by the hash sent by the server
                // and dump it into a file in our file system, no matter the original client request method.
                //
                // This time, if an error occurred, we don't care about the status code. The only reasons a registry would send
                // something other than a 200 is either rate limiting or server errors.
                //
                // Instead of bailing out, we could consider sending a stale version of the manifest. Later.
                let mut proxy_manifest = client.query_manifest(&proxy_response_head.hash, false).await?;

                tokio::fs::create_dir_all(&proxy_manifest_hash_path.parent().unwrap()).await?;
                let proxy_manifest_meta_hash_path = RegistryPathsHelper::manifest_meta(&app.conf.proxy_storage, &container_ref, &proxy_response_head.hash);
                tokio::fs::create_dir_all(proxy_manifest_meta_hash_path.parent().unwrap()).await?;
                let mut manifest_file = Manifest::new(&app.conf.proxy_storage, &app.conf.temporary_registry_storage, &container_ref, &manifest_ref);

                // And write all the things. The function will be in charge of writing the docker image manifest and its
                // related metadata, while making sure to not do stupid stuff such as overwriting the hash file with an
                // empty version of itself.
                manifest_file.save_manifest((&mut proxy_manifest.raw_response).into()).await?;
                manifest_file.save_manifest_metadata(&proxy_response_head.content_type).await?;
            } else {
                info!("Manifest is already cached");
            }

            (proxy_response_head.hash, proxy_response_head.content_length, proxy_response_head.content_type)
        },

        // Not ideal but easy to deal with: 404 Not Found
        Err(DockerClientError::UnexpectedStatusCode(code)) if code == 404 => {
            warn!("Upstream sent 404 Not Found");
            return Ok(StatusCode::NOT_FOUND.into_response())
        }

        Err(e) => return Err(e.into())
    };

    let proxy_manifest_hash_path = RegistryPathsHelper::manifest_path(&app.conf.proxy_storage, &container_ref, &manifest_ref);
    let body = StreamBody::new(ReaderStream::new(tokio::fs::File::open(&proxy_manifest_hash_path).await?));

    Ok((
        StatusCode::OK,
        [
            ("Content-Type", content_type.clone()),
            ("Docker-Content-Digest", proxy_hash.clone()),
            ("Content-Length", content_length.to_string())
        ],
        body
    ).into_response())
}