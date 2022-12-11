use std::path::{PathBuf, Path};

use axum::extract::BodyStream;
use eyre::ContextCompat;
use futures_util::StreamExt;
use serde::{Serialize, Deserialize};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::helpers::{RegistryPathsHelper, file256sum_async};

#[derive(Serialize, Deserialize)]
pub struct ManifestMetadata<'a> {
    pub hash: &'a str,
    pub content_type: &'a str,
}

pub struct Manifest {
    docker_hash: Option<String>,
    manifest_reference: String,
    container_ref: String,
    registry_root: PathBuf,
    registry_temp_root: PathBuf,
}

pub enum ManifestContentSources<'a> {
    ServerRequest(&'a mut BodyStream),
    ProxyResponse(&'a mut reqwest::Response)
}

impl<'a> From<&'a mut BodyStream> for ManifestContentSources<'a> {
    fn from(value: &'a mut BodyStream) -> Self {
        ManifestContentSources::ServerRequest(value)
    }
}

impl<'a> From<&'a mut reqwest::Response> for ManifestContentSources<'a> {
    fn from(value: &'a mut reqwest::Response) -> Self {
        ManifestContentSources::ProxyResponse(value)
    }
}

impl Manifest {
    pub fn new(registry_root: &Path, registry_temp_root: &Path, container_ref: &str, manifest_reference: &str) -> Self {
        let docker_hash = if manifest_reference.starts_with("sha256:") {
            Some(manifest_reference.to_string())
        } else {
            None
        };

        Self {
            docker_hash,
            manifest_reference: manifest_reference.to_string(),
            container_ref: container_ref.to_string(),
            registry_root: registry_root.to_path_buf(),
            registry_temp_root: registry_temp_root.to_path_buf()
        }
    }

    pub async fn save_manifest(&mut self, manifest_content_source: ManifestContentSources<'_>) -> eyre::Result<()> {
        // Chicken and egg problem if the manifest reference is not a hash.
        // To make a hash, we need the file content to be saved on disk. To save on disk, we need a path.
        // but to make a path, we need the hash.

        // To solve this problem, make a temporary file regardless of the manifest reference being a Docker
        // hash or not.
        let manifest_temporary_file_path = self.registry_temp_root.join(Uuid::new_v4().to_string());
        let manifest_is_a_docker_hash = self.docker_hash.is_some();

        let mut manifest_temporary_file = tokio::fs::File::create(&manifest_temporary_file_path).await?;
        match manifest_content_source {
            ManifestContentSources::ServerRequest(body_stream) => {
                while let Some(chunk) = body_stream.next().await {
                    let chunk = chunk?;
                    manifest_temporary_file.write_all(&chunk).await?;
                }
            },
            ManifestContentSources::ProxyResponse(proxy_response) => {
                while let Some(chunk) = proxy_response.chunk().await? {
                    manifest_temporary_file.write_all(&chunk).await?;
                }
            },
        }

        let docker_hash = match &self.docker_hash {
            Some(hash) => hash,
            None => {
                let docker_hash = file256sum_async(manifest_temporary_file_path.clone()).await??;
                let docker_hash = format!("sha256:{}", docker_hash);
                self.docker_hash = Some(docker_hash);
                self.docker_hash.as_ref().unwrap()
            }
        };

        // Paths for the manifest hash file and its named tag.
        let manifest_hash_path = RegistryPathsHelper::manifest_path(&self.registry_root, &self.container_ref, docker_hash);
        let manifest_hash_parent = manifest_hash_path.parent().unwrap();
        if !manifest_hash_parent.is_dir() {
            tokio::fs::create_dir_all(&manifest_hash_parent).await?;
        }

        // Move the manifest to its destination file
        tokio::fs::rename(&manifest_temporary_file_path, &manifest_hash_path).await?;

        // If the tag originally supplied by the caller was not a hash (see the first few lines of this function),
        // then we copy the hash file as the current tag.

        // This verification prevents overwriting the manifest file if it's a docker hash, 
        // because the hash path and the tag one would be the same.
        if !manifest_is_a_docker_hash {
            let manifest_tag_path = RegistryPathsHelper::manifest_path(&self.registry_root, &self.container_ref, &self.manifest_reference);
            tokio::fs::copy(&manifest_hash_path, &manifest_tag_path).await?;
        }

        Ok(())
    }

    pub async fn save_manifest_metadata(&self, content_type: &str) -> eyre::Result<()> {
        let docker_hash = self.docker_hash.as_ref().context("Docker container hash has not been yet calculated")?;

        let manifest_metadata_hash_path = RegistryPathsHelper::manifest_meta(&self.registry_root, &self.container_ref, docker_hash);
        let manifest_metadata_parent = manifest_metadata_hash_path.parent().unwrap();
        if !manifest_metadata_parent.is_dir() {
            tokio::fs::create_dir_all(&manifest_metadata_parent).await?;
        }

        let manifest_metadata = ManifestMetadata {
            hash: &docker_hash.replace("sha256:", ""),
            content_type,
        };

        let manifest_metadata_content = serde_json::to_string(&manifest_metadata)?;
        let mut manifest_metadata_file = tokio::fs::File::create(&manifest_metadata_hash_path).await?;
        manifest_metadata_file.write_all(manifest_metadata_content.as_bytes()).await?;

        if !self.manifest_reference.starts_with("sha256:") {
            let manifest_metadata_tag_path = RegistryPathsHelper::manifest_meta(&self.registry_root, &self.container_ref, &self.manifest_reference);
            tokio::fs::copy(&manifest_metadata_hash_path, &manifest_metadata_tag_path).await?;
        }

        Ok(())
    }

    pub fn docker_hash(&self) -> eyre::Result<&String> {
        self.docker_hash
            .as_ref()
            .context("Hash for the manifest has not been calculated yet")
    }
}