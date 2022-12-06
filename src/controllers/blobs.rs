use std::borrow::Cow;

use axum::{http::StatusCode, extract::{Path, State}, response::IntoResponse};
use eyre::ContextCompat;

use crate::{data::upload_in_progress::UploadInProgress, ApplicationState, controllers::RegistryHttpError};
use crate::controllers::RegistryHttpResult;

#[tracing::instrument(skip_all)]
pub async fn initiate_upload(
    Path(container_ref): Path<String>, 
    State(application): State<ApplicationState>,
) -> RegistryHttpResult {
    if container_ref.contains("..") {
        return Err(RegistryHttpError::InvalidName(Cow::from(container_ref)));
    } else if container_ref == "" {
        return Err(RegistryHttpError::InvalidName(Cow::from("<empty name>")));
    }

    let mut uploads = application.uploads.write().await;
    let upload = UploadInProgress::new(&container_ref, &application.configuration.registry_storage);
    let upload_id = upload.id;
    tracing::info!("Initiating upload for [{}] blob {}", container_ref, upload_id);
    uploads.insert(upload_id, upload);
    drop(uploads);

    let uploads = application.uploads.read().await;
    let upload = uploads.get(&upload_id)
        .context("Upload key that just has been inserted doesn't exist")?;
    upload.create_containing_directory().await?;

    Ok((
        StatusCode::CREATED,
        [
            ("Location", upload.http_upload_uri()),
            ("Range", "0-0".to_string()),
            ("Docker-Upload-UUID", upload_id.to_string())
        ]
    ).into_response())
}

#[tracing::instrument]
async fn process_upload(

) -> impl IntoResponse {

}
