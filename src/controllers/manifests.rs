use std::os::unix::prelude::MetadataExt;

use axum::{response::IntoResponse, extract::{Path, BodyStream, State}, TypedHeader, headers, http::{StatusCode, self}, body::{StreamBody, BoxBody}, debug_handler};
use tokio::io::AsyncWriteExt;
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
    manifest.save_manifest(&mut body).await?;
    info!("Saving metadata");
    manifest.save_manifest_metadata(&content_type.to_string()).await?;

    Ok((
        StatusCode::CREATED,
        [
            ("Location", format!("/v2/{}/manifests/{}", container_ref, manifest_ref)),
            ("Docker-Content-Digest", format!("sha256:{}", manifest.docker_hash()?))
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
    http_method: http::Method
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;
    reject_invalid_tags_refs(&manifest_ref)?;

    // TODO: Rearrange code to support offline proxying, that is if the upstream proxy did send 429 or any 5xx HTTP code
    let client = app.docker_clients.get_client(&container_ref).await?;
    info!("Querying upstream HEAD to fetch the most manifest related to the tag");
    let manifest_head_response = client.query_manifest(&manifest_ref, true).await;
    let proxy_manifest_head = match manifest_head_response {
        Ok(proxy_response) => proxy_response,

        Err(DockerClientError::UnexpectedStatusCode(code)) if code == 404 => {
            warn!("Upstream sent 404 Not Found");
            return Ok(StatusCode::NOT_FOUND.into_response())
        }

        Err(e) => return Err(e.into())
    };

    // Check if we have the same copy of the manifest somewhere in our files before sending a GET request
    // to the upstream respository.

    let proxy_manifest_hash_path = RegistryPathsHelper::manifest_path(&app.conf.proxy_storage, &container_ref, &proxy_manifest_head.hash);
    info!("Check if cached manifest file exists for hash {}", proxy_manifest_head.hash);

    if !proxy_manifest_hash_path.is_file() {
        info!("File does not exist. Querying and caching the upstream manifest");
        tokio::fs::create_dir_all(&proxy_manifest_hash_path.parent().unwrap()).await?;
        // We don't have the manifest, GET the manifest referenced by the hash sent by the server
        // and dump it into a file in our file system, no matter the original client request method.

        // This time, if an error occurred, we don't care about the status code. If the registry sent a 200
        // for a HEAD request *AND* sent the hash, why would it suddently declare the manifest not found or other status
        // code ? Same goes for the hash.
        let mut proxy_manifest = client.query_manifest(&proxy_manifest_head.hash, false).await?;
        let mut proxy_manifest_file = tokio::fs::File::create(&proxy_manifest_hash_path).await?;
        info!("Writing cache");
        while let Some(chunk) = proxy_manifest.raw_response.chunk().await? {
            proxy_manifest_file.write_all(&chunk).await?;
        }

        let proxy_manifest_tag_path = RegistryPathsHelper::manifest_path(&app.conf.proxy_storage, &container_ref, &manifest_ref);
        // If the client requests a particular hash manifest, it will exist since we have already dumped it into its own file
        // before copying it to the named tag.
        tokio::fs::copy(&proxy_manifest_hash_path, &proxy_manifest_tag_path).await?;

        info!("Writing metadata");
        // Create all the files related to metadata saved in the proxy repository
        let proxy_manifest_meta_hash_path = RegistryPathsHelper::manifest_meta(&app.conf.proxy_storage, &container_ref, &proxy_manifest_head.hash);
        let proxy_manifest_meta_tag_path = RegistryPathsHelper::manifest_meta(&app.conf.proxy_storage, &container_ref, &manifest_ref);
        tokio::fs::create_dir_all(proxy_manifest_meta_hash_path.parent().unwrap()).await?;
        let manifest_metadata = ManifestMetadata { content_type: &proxy_manifest.content_type, hash: &&proxy_manifest.hash };
        let manifest_metadata = serde_json::to_string(&manifest_metadata).unwrap();
        let mut manifest_metadata_file = tokio::fs::File::create(&proxy_manifest_meta_hash_path).await?;
        manifest_metadata_file.write_all(manifest_metadata.as_bytes()).await?;
        manifest_metadata_file.flush().await?;
        drop(manifest_metadata_file);
        tokio::fs::copy(&proxy_manifest_meta_hash_path, &proxy_manifest_meta_tag_path).await?;
    } else {
        // Else ­— if the hash file exists —, we do nothing.
        info!("Hash is already cached")
    }

    let body = StreamBody::new(ReaderStream::new(tokio::fs::File::open(&proxy_manifest_hash_path).await?));

    Ok((
        StatusCode::OK,
        [
            ("Content-Type", proxy_manifest_head.content_type.clone()),
            ("Docker-Content-Digest", proxy_manifest_head.hash.clone()),
            ("Content-Length", proxy_manifest_head.content_length.to_string())
        ],
        body
    ).into_response())
}