use std::sync::Arc;

use axum::{http::StatusCode, extract::{Path, State}};

use crate::data::incomplete_upload::UploadInProgressStore;

#[tracing::instrument(skip_all)]
pub async fn initiate_upload(
    Path(container_ref): Path<String>, 
    State(_uploads): State<Arc<UploadInProgressStore>>
) -> (StatusCode, &'static str) {
    tracing::info!("Initiating upload for {}", container_ref);
    (StatusCode::NOT_IMPLEMENTED, "")
}