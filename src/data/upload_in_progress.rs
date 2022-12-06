use std::{collections::HashMap, path::{PathBuf, Path}, time::Instant};

use uuid::Uuid;

pub type UploadInProgressStore = HashMap<Uuid, UploadInProgress>;

#[derive(Debug)]
pub struct UploadInProgress {
    pub id: Uuid,
    pub container_reference: String,
    pub temporary_file: PathBuf,
    pub last_interacted_with: Instant
}

impl UploadInProgress {
    pub fn new(container_reference: &str, registry_root: &Path) -> Self {
        let id = Uuid::new_v4();
        let temporary_file = registry_root.join(format!("blobs/{}", id));

        Self {
            id: Uuid::new_v4(),
            container_reference: container_reference.to_string(),
            temporary_file,
            last_interacted_with: Instant::now(),
        }
    }

    pub fn create_containing_directory(&self) -> impl std::future::Future<Output = Result<(), std::io::Error>> + '_ {
        let parent = self.temporary_file
            .parent()
            .expect("Expected parent of the file");

        tokio::fs::create_dir_all(parent)
    }

    pub fn http_upload_uri(&self) -> String {
        format!("/v2/{}/blobs/uploads/{}", self.container_reference, self.id)
    }
}