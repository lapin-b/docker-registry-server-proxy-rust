use std::{borrow::Cow, path::{PathBuf, Path}};

use uuid::Uuid;

use crate::controllers::RegistryHttpError;

struct RegistryPathsHelper;

impl RegistryPathsHelper {
    // pub fn temporary_file(base: &Path, upload_id: Uuid) -> PathBuf {
        
    // }
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