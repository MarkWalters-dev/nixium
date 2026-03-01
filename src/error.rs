use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
}

impl ApiError {
    pub fn response(status: StatusCode, msg: impl Into<String>) -> Response {
        (status, Json(ApiError { error: msg.into() })).into_response()
    }
}
