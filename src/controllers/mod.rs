use std::{borrow::Cow};

use axum::{response::{Response, IntoResponse}, Json, http::StatusCode};

use crate::data::json_registry_error::RegistryJsonError;

pub mod base;
pub mod blobs;

pub type RegistryHttpResult = Result<Response, RegistryHttpError>;

pub trait IntoRegistryHttpResult {
    fn into_registry_result(self) -> RegistryHttpResult;
}

impl <T: IntoResponse> IntoRegistryHttpResult for T {
    fn into_registry_result(self) -> RegistryHttpResult {
        Ok(self.into_response())
    }
}

#[derive(Debug)]
pub enum RegistryHttpError {
    InvalidName(Cow<'static, str>),
    InvalidHashFormat(Cow<'static, str>),
    RegistryInternalError(String),
}

impl RegistryHttpError {
    fn from_report(err: eyre::Report) -> Self {
        tracing::error!("HTTP handler error: {:?}", err.root_cause());
        Self::RegistryInternalError(format!("Registry internal error: {:#}", err))
    }
}

impl IntoResponse for RegistryHttpError {
    fn into_response(self) -> Response {
        match self {
            RegistryHttpError::InvalidName(name) => {
                (
                    StatusCode::BAD_REQUEST,
                    Json(RegistryJsonError::new("NAME_INVALID", &format!("Name {} is invalid", name), ""))
                ).into_response()
            },
            RegistryHttpError::RegistryInternalError(description) => {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(RegistryJsonError::new("INTERNAL_ERROR", "An internal server error occured", &description))
                ).into_response()
            },
            RegistryHttpError::InvalidHashFormat(hash) => {
                (
                    StatusCode::BAD_REQUEST,
                    Json(RegistryJsonError::new("BLOB_UNKNOWN", "Invalid hash format", &hash))
                ).into_response()
            },
        }
    }
}

macro_rules! impl_into_registry_error {
    ($from:ty) => {
        impl From<$from> for RegistryHttpError {
            fn from(err: $from) -> Self {
                Self::from_report(err.into())
            }
        }
    };
}

impl_into_registry_error!(std::io::Error);
impl_into_registry_error!(eyre::Report);
