use axum::{http::StatusCode, extract::Path};

#[tracing::instrument]
pub async fn initiate_upload(Path(container_ref): Path<String>) -> (StatusCode, &'static str) {
    tracing::info!("Initiating upload for {}", container_ref);
    (StatusCode::NOT_IMPLEMENTED, "")
}