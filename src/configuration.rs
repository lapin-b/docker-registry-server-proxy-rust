use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Configuration {
    pub registry_storage: String,
}