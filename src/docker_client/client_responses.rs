
pub struct ProxyManifestResponse {
    // pub container: String,
    // pub manifest_ref: String,
    pub hash: String,
    pub content_type: String,
    pub content_length: u32,
    pub raw_response: reqwest::Response
}

pub struct ProxyBlobResponse {
    pub hash: Option<String>,
    pub content_length: u32,
    pub raw_response: reqwest::Response
}