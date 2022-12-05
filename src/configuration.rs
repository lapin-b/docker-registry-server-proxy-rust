use std::path::PathBuf;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Configuration {
    pub registry_storage: PathBuf
}
