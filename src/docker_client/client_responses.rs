use serde::Deserialize;

#[derive(Debug)]
pub struct ProxyManifestResponse {
    // pub container: String,
    // pub manifest_ref: String,
    pub hash: String,
    pub content_type: String,
    pub raw_response: reqwest::Response
}