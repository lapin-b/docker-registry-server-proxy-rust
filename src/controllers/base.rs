use axum::http::StatusCode;

pub async fn root() -> StatusCode {
    StatusCode::OK
}

pub async fn registry_base() -> &'static str {
    "{}"
}