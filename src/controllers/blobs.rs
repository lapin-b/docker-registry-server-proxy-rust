use axum::{http::StatusCode, extract::{Path, State}, response::IntoResponse};

use crate::{data::incomplete_upload::UploadInProgress, ApplicationState};

#[tracing::instrument(skip_all)]
pub async fn initiate_upload(
    Path(container_ref): Path<String>, 
    State(application): State<ApplicationState>,
) -> impl IntoResponse {
    let mut uploads = application.uploads.write().await;
    let upload = UploadInProgress::new(&container_ref, &application.configuration.registry_storage);
    let upload_id = upload.id;
    tracing::info!("Initiating upload for {} blob {}", container_ref, upload_id);
    uploads.insert(upload_id, upload);
    drop(uploads);

    let uploads = application.uploads.read().await;
    let upload = uploads.get(&upload_id).expect("Key that just has been inserted doesn't exist");
    upload.create_containing_directory().await.unwrap();

    (
        StatusCode::CREATED,
        [
            ("Location", upload.http_upload_uri()),
            ("Range", "0-0".to_string()),
            ("Docker-Upload-UUID", upload_id.to_string())
        ]
    )
}