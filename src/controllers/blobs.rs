use axum::{http::StatusCode, extract::{Path, State}};

use crate::{data::incomplete_upload::UploadInProgress, ApplicationState};

#[tracing::instrument(skip_all)]
pub async fn initiate_upload(
    Path(container_ref): Path<String>, 
    State(application): State<ApplicationState>,
) -> (StatusCode, &'static str) {
    tracing::info!("Initiating upload for {}", container_ref);
    let mut uploads = application.uploads.write().await;
    let upload = UploadInProgress::new(&container_ref, &application.configuration.registry_storage);

    upload.create_containing_directory().await.unwrap();
    uploads.insert(upload.id, upload);

    (StatusCode::ACCEPTED, "")
}