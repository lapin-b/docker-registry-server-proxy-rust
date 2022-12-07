use axum::{response::{Response, IntoResponse}, http::StatusCode};
use crate::data::json_registry_error::RegistryJsonErrorReprWrapper;

pub mod base;
pub mod blobs;
pub mod manifests;

pub type RegistryHttpResult = Result<Response, RegistryHttpError>;

#[derive(thiserror::Error, Debug)]
pub enum RegistryHttpError {
    #[error("Invalid repository name {0}")]
    InvalidRepositoryName(String),

    #[allow(dead_code)]
    #[error("Invalid tag name {0}")]
    InvalidTagName(String),
    
    #[error("Invalid hash format {0}")]
    InvalidHashFormat(String),
    
    #[error("Upload ID {0} not found or invalid")]
    UploadIdNotFound(String),
    
    // #[error("Multiple registry errors: {0:?}")]
    // MultipleErrors(Vec<Self>),
    
    #[error("Internal server error: {0}")]
    RegistryInternalError(eyre::Report),
}

impl IntoResponse for RegistryHttpError {
    fn into_response(self) -> Response {
        let (http_code, registry_error) = match self {
            RegistryHttpError::InvalidRepositoryName(_) => (StatusCode::BAD_REQUEST, "NAME_INVALID"),
            RegistryHttpError::InvalidTagName(_) => (StatusCode::BAD_REQUEST, "TAG_INVALID"),
            RegistryHttpError::InvalidHashFormat(_) => (StatusCode::BAD_REQUEST, "UNSUPPORTED"),
            RegistryHttpError::UploadIdNotFound(_) => (StatusCode::NOT_FOUND, "UNSUPPORTED"),
            RegistryHttpError::RegistryInternalError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "UNKNOWN"),
            // RegistryHttpError::MultipleErrors(_) => (StatusCode::BAD_REQUEST, ""),
        };

        let json_representaiton = match self {
            // RegistryHttpError::MultipleErrors(errors) => {
                // RegistryJsonErrorReprWrapper::multiple(errors.as_slice())
            // }
            RegistryHttpError::InvalidRepositoryName(error) => RegistryJsonErrorReprWrapper::single(registry_error, error, ""),
            RegistryHttpError::InvalidTagName(error) => RegistryJsonErrorReprWrapper::single(registry_error, error, ""),
            RegistryHttpError::InvalidHashFormat(error) => RegistryJsonErrorReprWrapper::single(registry_error, error, ""),
            RegistryHttpError::UploadIdNotFound(error) => RegistryJsonErrorReprWrapper::single(registry_error, error, ""),
            RegistryHttpError::RegistryInternalError(error) => RegistryJsonErrorReprWrapper::single(registry_error, error, ""),
        };

        let body = serde_json::to_string_pretty(&json_representaiton).unwrap();

        (
            http_code,
            body
        ).into_response()
    }
}

macro_rules! impl_from {
    ($from:ty) => {
        impl From<$from> for RegistryHttpError {
            fn from(e: $from) -> Self {
                Self::RegistryInternalError(e.into())
            }
        }
    };
}

impl From<uuid::Error> for RegistryHttpError {
    fn from(value: uuid::Error) -> Self {
        Self::UploadIdNotFound(value.to_string())
    }
}

impl_from!(std::io::Error);
impl_from!(axum::Error);
impl_from!(tokio::task::JoinError);
impl_from!(eyre::Report);
