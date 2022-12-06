use std::{borrow::Cow, path::{PathBuf, Path}};

use uuid::Uuid;

use crate::controllers::RegistryHttpError;

pub struct RegistryPathsHelper;

impl RegistryPathsHelper {
    pub fn blob_path(registry_path: &Path, container_ref: &str, hash: &str) -> PathBuf {
        registry_path
            .join(container_ref)
            .join("blobs")
            .join(hash)
    }

    pub fn temporary_blob_path(temp_path: &Path, upload_id: Uuid) -> PathBuf {
        temp_path
            .join("blobs")
            .join(upload_id.to_string())
    }
}

pub fn reject_invalid_container_names(container_ref: &str) -> Result<(), RegistryHttpError> {
    if container_ref.contains("..") {
        Err(RegistryHttpError::InvalidName(Cow::Owned(container_ref.to_string())))
    } else if container_ref.is_empty() {
        Err(RegistryHttpError::InvalidName(Cow::from("<empty name>")))
    } else {
        Ok(())
    }
}