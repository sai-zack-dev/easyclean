use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// An API error that renders as `{ "error": "..." }` with a status code.
pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn not_found(msg: &str) -> Self {
        ApiError(StatusCode::NOT_FOUND, msg.to_string())
    }
    pub fn bad(msg: &str) -> Self {
        ApiError(StatusCode::BAD_REQUEST, msg.to_string())
    }
    pub fn internal<E: std::fmt::Display>(e: E) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<easyclean_core::Error> for ApiError {
    fn from(e: easyclean_core::Error) -> Self {
        ApiError::internal(e)
    }
}
