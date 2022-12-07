use std::path::{PathBuf, Path};

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

pub fn reject_invalid_container_refs(container_ref: &str) -> Result<(), RegistryHttpError> {
    if !ref_is_valid(container_ref) {
        Err(RegistryHttpError::invalid_repository_name(&container_ref))
    } else{
        Ok(())
    }
}

pub fn reject_invalid_tags_refs(tag: &str) -> Result<(), RegistryHttpError> {
    if !ref_is_valid(tag) {
        Err(RegistryHttpError::invalid_tag_name(&tag))
    } else{
        Ok(())
    }
}

fn ref_is_valid(rref: &str) -> bool {
    !rref.contains("..") && !rref.trim().is_empty()
}