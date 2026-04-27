//! HTTP 错误响应

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

pub struct ApiErrorResponse(StatusCode, String);

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(serde_json::json!({
                "success": false,
                "error": self.1
            })),
        )
            .into_response()
    }
}

impl<E: std::fmt::Display> From<E> for ApiErrorResponse {
    fn from(err: E) -> Self {
        ApiErrorResponse(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    }
}
