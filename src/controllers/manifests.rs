use axum::{response::IntoResponse, extract::{Path, BodyStream, State}, TypedHeader, headers, http::StatusCode};
use futures_util::StreamExt;
use serde::{Serialize, Deserialize};
use tokio::io::AsyncWriteExt;
use tracing::info;

use crate::{data::helpers::{reject_invalid_refrence_names, RegistryPathsHelper}, ApplicationState};
use crate::controllers::RegistryHttpResult;

#[derive(Serialize, Deserialize)]
pub struct ManifestMetadata {
    content_type: String
}

#[tracing::instrument(skip_all, fields(container_ref = container_ref, manifest_ref = manifest_ref))]
pub async fn upload_manifest(
    Path((container_ref, manifest_ref)): Path<(String, String)>,
    TypedHeader(content_type): TypedHeader<headers::ContentType>,
    State(app): State<ApplicationState>,
    mut body: BodyStream
) -> RegistryHttpResult {
    reject_invalid_refrence_names(&container_ref)?;
    reject_invalid_refrence_names(&manifest_ref)?;

    let manifest_path = RegistryPathsHelper::manifest_path(&app.conf.registry_storage, &container_ref, &manifest_ref);
    let manifest_meta_path = RegistryPathsHelper::manifest_meta(&app.conf.registry_storage, &container_ref, &manifest_ref);

    tokio::fs::create_dir_all(manifest_path.parent().unwrap()).await?;
    tokio::fs::create_dir_all(manifest_meta_path.parent().unwrap()).await?;

    info!("Writing manifest to {:?}", manifest_path);
    let mut manifest_file = tokio::fs::File::create(&manifest_path).await?;
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        manifest_file.write_all(&chunk).await?;
    }

    info!("Writing manifest metadata to {:?}", manifest_meta_path);
    let manifest_meta = ManifestMetadata { content_type: content_type.to_string() };
    let mut manifest_meta_file = tokio::fs::File::create(&manifest_meta_path).await?;
    manifest_meta_file.write_all(serde_json::to_string_pretty(&manifest_meta).unwrap().as_bytes()).await?;

    drop(manifest_file);
    drop(manifest_meta);

    let manifest_sha256 = tokio::task::spawn_blocking(move || {
        sha256::try_digest(manifest_path.as_path())
    }).await??;

    Ok((
        StatusCode::CREATED,
        [
            ("Location", format!("/v2/{}/manifests/{}", container_ref, manifest_ref)),
            ("Docker-Content-Digest", format!("sha256:{}", manifest_sha256))
        ]
    ).into_response())
}