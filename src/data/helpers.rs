use std::borrow::Cow;

use crate::controllers::RegistryHttpError;


pub fn reject_invalid_container_names(container_ref: &str) -> Result<(), RegistryHttpError> {
    if container_ref.contains("..") {
        Err(RegistryHttpError::InvalidName(Cow::Owned(container_ref.to_string())))
    } else if container_ref.is_empty() || container_ref == "" {
        Err(RegistryHttpError::InvalidName(Cow::from("<empty name>")))
    } else {
        Ok(())
    }
}