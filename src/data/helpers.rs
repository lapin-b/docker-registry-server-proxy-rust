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

    pub fn manifest_path(registry_path: &Path, container_ref: &str, manifest_ref: &str) -> PathBuf {
        registry_path
            .join(container_ref)
            .join("manifests")
            .join(manifest_ref)
    }

    pub fn manifest_meta(registry_path: &Path, container_ref: &str, manifest_ref: &str) -> PathBuf {
        registry_path
            .join(container_ref)
            .join("meta")
            .join(manifest_ref)
    }
}

pub fn reject_invalid_refrence_names<'a>(container_ref: &'a str) -> Result<(), RegistryHttpError> {
    if container_ref.contains("..") {
        Err(RegistryHttpError::InvalidRepositoryName(container_ref.to_string()))
    } else if container_ref.is_empty() {
        Err(RegistryHttpError::InvalidRepositoryName(container_ref.to_string()))
    } else {
        Ok(())
    }
}