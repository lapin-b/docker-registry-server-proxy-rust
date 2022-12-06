use std::{collections::HashMap, path::{PathBuf, Path}, time::Instant, sync::Arc};

use tokio::sync::RwLock;
use uuid::Uuid;

use super::helpers::RegistryPathsHelper;

type UploadStoreItem = Arc<RwLock<Upload>>;

#[derive(Debug)]
pub struct Upload {
    pub id: Uuid,
    pub temporary_file_path: PathBuf,
    pub container_reference: String,
    pub last_interacted_with: Instant
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

    pub async fn create_upload(&self, container_ref: &str, temporary_files_root: &Path) -> UploadStoreItem {
        let upload = Upload::new(container_ref, temporary_files_root);
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
        Ok(self.delete_upload(uuid).await)
    }
}

impl Upload {
    pub fn new(container_reference: &str, temporary_root: &Path) -> Self {
        let id = Uuid::new_v4();

        Self {
            id,
            temporary_file_path: RegistryPathsHelper::temporary_blob_path(temporary_root, id),
            container_reference: container_reference.to_string(),
            last_interacted_with: Instant::now(),
        }
    }

    pub fn create_containing_directory(&self) -> impl std::future::Future<Output = Result<(), std::io::Error>> + '_ {
        let parent = self.temporary_file_path
            .parent()
            .expect("Expected parent of the file");

        tokio::fs::create_dir_all(parent)
    }

    pub async fn create_or_open_upload_file(&self) -> std::io::Result<tokio::fs::File> {
        if self.temporary_file_path.is_file() {
            tokio::fs::File::open(&self.temporary_file_path).await
        } else {
            tokio::fs::File::create(&self.temporary_file_path).await
        }
    }

    pub fn http_upload_uri(&self) -> String {
        format!("/v2/{}/blobs/uploads/{}", self.container_reference, self.id)
    }

    pub fn update_last_interacted(&mut self) {
        self.last_interacted_with = Instant::now();
    }
}