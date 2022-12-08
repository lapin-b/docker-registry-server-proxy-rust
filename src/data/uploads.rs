use std::{collections::HashMap, path::{PathBuf, Path}, time::Instant, sync::Arc};

use axum::extract::BodyStream;
use futures_util::StreamExt;
use tokio::{sync::RwLock, io::AsyncWriteExt};
use tokio::io::AsyncSeekExt;
use uuid::Uuid;

use super::helpers::RegistryPathsHelper;

type UploadStoreItem = Arc<RwLock<Upload>>;

#[derive(Debug)]
pub struct Upload {
    pub id: Uuid,
    pub temporary_file_path: PathBuf,
    pub last_interacted_with: Instant,
    container_reference: String,
    registry_root: PathBuf
}

impl Upload {
    pub fn new(container_reference: &str, temporary_root: &Path, registry_root: &Path) -> Self {
        let id = Uuid::new_v4();

        Self {
            id,
            temporary_file_path: RegistryPathsHelper::temporary_blob_path(temporary_root, id),
            container_reference: container_reference.to_string(),
            last_interacted_with: Instant::now(),
            registry_root: registry_root.to_path_buf()
        }
    }

    pub async fn create_parent_directory(&self) -> Result<(), std::io::Error> {
        let parent = self.temporary_file_path
            .parent()
            .expect("Expected parent of the file");

        if parent.is_dir() {
            return Ok(())
        }

        tokio::fs::create_dir_all(parent).await
    }

    pub async fn write_blob(&self, layer: &mut BodyStream) -> eyre::Result<u64> {
        let mut file = if self.temporary_file_path.is_file() {
            tokio::fs::File::open(&self.temporary_file_path).await?
        } else {
            tokio::fs::File::create(&self.temporary_file_path).await?
        };

        file.seek(std::io::SeekFrom::End(0)).await?;

        while let Some(chunk) = layer.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
        }

        let position = file.seek(std::io::SeekFrom::End(0)).await?;

        Ok(position)
    }

    pub async fn cleanup_upload(&self) -> std::io::Result<()> {
        if self.temporary_file_path.is_file() {
            tokio::fs::remove_file(&self.temporary_file_path).await?;
        }

        Ok(())
    }

    pub async fn finalize_upload(&self, hash: &str) -> std::io::Result<()> {
        // Move this blob to its final resting place.
        let final_blob_path = RegistryPathsHelper::blob_path(&self.registry_root, &self.container_reference, hash);
        let blob_parent = final_blob_path.parent().unwrap();
        if !blob_parent.is_dir() {
            tokio::fs::create_dir_all(blob_parent).await?;
        }

        tokio::fs::rename(&self.temporary_file_path, &final_blob_path).await?;

        Ok(())
    }

    pub fn http_upload_uri(&self) -> String {
        format!("/v2/{}/blobs/uploads/{}", self.container_reference, self.id)
    }

    pub fn update_last_interacted(&mut self) {
        self.last_interacted_with = Instant::now();
    }
}

#[derive(Clone)]
pub struct UploadsStore {
    inner: Arc<RwLock<HashMap<Uuid, UploadStoreItem>>>
}

impl UploadsStore {
    pub fn new() -> Self {
        Self {
            inner: Default::default()
        }
    }

    pub async fn create_upload(&self, container_ref: &str, temporary_files_root: &Path, registry_root: &Path) -> UploadStoreItem {
        let upload = Upload::new(container_ref, temporary_files_root, registry_root);
        let id = upload.id;

        let upload = Arc::new(RwLock::new(upload));
        let mut lock = self.inner.write().await;
        lock.insert(id, Arc::clone(&upload));

        upload
    }

    pub async fn fetch_upload(&self, upload: Uuid) -> Option<UploadStoreItem> {
        let lock = self.inner.read().await;

        lock.get(&upload).cloned()
    }

    pub async fn fetch_upload_string_uuid(&self, upload: &str) -> Result<Option<UploadStoreItem>, uuid::Error> {
        let uuid = upload.parse()?;
        Ok(self.fetch_upload(uuid).await)
    }

    pub async fn delete_upload(&self, upload: Uuid) {
        let mut lock = self.inner.write().await;
        lock.remove(&upload);
    }

    pub async fn delete_upload_uuid(&self, upload: &str) -> Result<(), uuid::Error> {
        let uuid = upload.parse()?;
        self.delete_upload(uuid).await;
        Ok(())
    }
}

impl Default for UploadsStore {
    fn default() -> Self {
        Self::new()
    }
}
