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
    docker_tag: String,
    container_ref: String,
    registry_root: PathBuf,
    registry_temp_root: PathBuf,
}

impl Manifest {
    pub fn new(registry_root: &Path, registry_temp_root: &Path, container_ref: &str, docker_tag: &str) -> Self {
        Self {
            docker_hash: None,
            docker_tag: docker_tag.to_string(),
            container_ref: container_ref.to_string(),
            registry_root: registry_root.to_path_buf(),
            registry_temp_root: registry_temp_root.to_path_buf()
        }
    }

    pub async fn save_manifest(&mut self, manifest_content: &mut BodyStream) -> eyre::Result<()> {
        let manifest_temporary_file_path = self.registry_temp_root.join(Uuid::new_v4().to_string());

        let mut manifest_temporary_file = tokio::fs::File::create(&manifest_temporary_file_path).await?;
        while let Some(chunk) = manifest_content.next().await {
            let chunk = chunk?;
            manifest_temporary_file.write_all(&chunk).await?;
        }
        let docker_hash = file256sum_async(manifest_temporary_file_path.clone()).await??;
        self.docker_hash = Some(docker_hash.clone());

        // Paths for the manifest and tag
        let manifest_tag_path = RegistryPathsHelper::manifest_path(&self.registry_root, &self.container_ref, &self.docker_tag);
        let manifest_hash_path = RegistryPathsHelper::manifest_path(&self.registry_root, &self.container_ref, &format!("sha256:{}", docker_hash));
        let manifest_hash_parent = manifest_hash_path.parent().unwrap();

        if !manifest_hash_parent.is_dir() {
            tokio::fs::create_dir_all(&manifest_hash_parent).await?;
        }

        // Dump the manifest into its destination file, then symlink it
        tokio::fs::rename(&manifest_temporary_file_path, &manifest_hash_path).await?;
        tokio::fs::copy(&manifest_hash_path, &manifest_tag_path).await?;

        Ok(())
    }

    pub async fn save_manifest_metadata(&self, content_type: &str) -> eyre::Result<()> {
        let docker_hash = self.docker_hash.as_ref().context("Docker container hash has not been yet calculated")?;

        // Paths for the metadata files
        let manifest_metadata_tag_path = RegistryPathsHelper::manifest_meta(&self.registry_root, &self.container_ref, &self.docker_tag);
        let manifest_metadata_hash_path = RegistryPathsHelper::manifest_meta(&self.registry_root, &self.container_ref, &format!("sha256:{}", docker_hash));
        let manifest_metadata_parent = manifest_metadata_hash_path.parent().unwrap();

        if !manifest_metadata_parent.is_dir() {
            tokio::fs::create_dir_all(&manifest_metadata_parent).await?;
        }

        let manifest_metadata = ManifestMetadata {
            hash: &docker_hash,
            content_type,
        };

        let manifest_metadata_content = serde_json::to_string(&manifest_metadata)?;
        let mut manifest_metadata_file = tokio::fs::File::create(&manifest_metadata_hash_path).await?;
        manifest_metadata_file.write_all(manifest_metadata_content.as_bytes()).await?;

        tokio::fs::copy(&manifest_metadata_hash_path, &manifest_metadata_tag_path).await?;

        Ok(())
    }

    pub fn docker_hash(&self) -> eyre::Result<&String> {
        self.docker_hash.as_ref()
            .context("Hash for the manifest has not been calculated yet")
    }
}