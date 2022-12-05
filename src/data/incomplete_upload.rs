use std::collections::HashMap;

use axum::extract::FromRef;

pub type UploadInProgressStore = HashMap<String, UploadInProgress>;

pub struct UploadInProgress {

}

impl UploadInProgress {

}