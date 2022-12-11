use std::path::{PathBuf, Path};

use once_cell::sync::Lazy;
use regex::Regex;
use sha2::{Sha256, Digest};
use uuid::Uuid;

use crate::controllers::RegistryHttpError;

static REGISTRY_CONTAINER_SEPARATION_REGEX: Lazy<Regex> = Lazy::new(|| {
    // The registry part is mandatory, I don't want to deal with "rust:latest"
    // aliasing to "registry.docker.io/library/rust:latest"
    Regex::new("(?P<registry>[a-zA-z.]+(?::[0-9]{1,6})?)/(?P<container>[a-zA-Z0-9-./]+)$").unwrap()
});

pub struct RegistryPathsHelper;

impl RegistryPathsHelper {
    pub fn blob_path(registry_path: &Path, container_ref: &str, hash: &str) -> PathBuf {
        registry_path
            .join(container_ref)
            .join("_repository")
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
            .join("_repository")
            .join("manifests")
            .join(manifest_ref)
    }

    pub fn manifest_meta(registry_path: &Path, container_ref: &str, manifest_ref: &str) -> PathBuf {
        registry_path
            .join(container_ref)
            .join("_repository")
            .join("meta")
            .join(manifest_ref)
    }
}

pub fn reject_invalid_container_refs(container_ref: &str) -> Result<(), RegistryHttpError> {
    if !ref_is_valid(container_ref) {
        Err(RegistryHttpError::invalid_repository_name(container_ref))
    } else{
        Ok(())
    }
}

pub fn reject_invalid_tags_refs(tag: &str) -> Result<(), RegistryHttpError> {
    if !ref_is_valid(tag) {
        Err(RegistryHttpError::invalid_tag_name(tag))
    } else{
        Ok(())
    }
}

pub fn file256sum(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(base16ct::lower::encode_string(&hash))
}

pub fn file256sum_async(path: PathBuf) -> tokio::task::JoinHandle<std::io::Result<String>> {
    tokio::task::spawn_blocking(move || {
        file256sum(path.as_path())
    })
}

pub fn split_registry_and_container(registry_container: &str) -> (&str, &str) {
    let components = REGISTRY_CONTAINER_SEPARATION_REGEX.captures(registry_container).unwrap();

    let registry = components.name("registry").unwrap().as_str();
    let container = components.name("container").unwrap().as_str();

    (registry, container)
}

fn ref_is_valid(rref: &str) -> bool {
    !rref.contains("..") && !rref.trim().is_empty()
}