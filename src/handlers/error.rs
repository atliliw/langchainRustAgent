//! 统一错误响应格式
//!
//! 当 API 处理出错时，返回统一格式的 JSON 错误：
//!   { "success": false, "error": "错误信息" }

use crate::errors::{ApiError, TestError};
use axum::{
    response::{IntoResponse, Response},
    Json,
};
use std::fmt;

/// 统一错误响应包装
/// 用法: return Err(ApiErrorResponse(StatusCode::错误码, "错误描述".to_string()))
pub struct ApiErrorResponse(pub axum::http::StatusCode, pub String);

// 自动从 ApiError 转换
// 这样 handlers 里用 ? 时，ApiError 自动转成 ApiErrorResponse
impl From<ApiError> for ApiErrorResponse {
    fn from(e: ApiError) -> Self {
        use axum::http::StatusCode;
        let msg = e.to_string();
        match &e {
            ApiError::UploadError(_) => ApiErrorResponse(StatusCode::BAD_REQUEST, msg),
            ApiError::SearchError(_) => ApiErrorResponse(StatusCode::INTERNAL_SERVER_ERROR, msg),
            _ => ApiErrorResponse(StatusCode::INTERNAL_SERVER_ERROR, msg),
        }
    }
}

impl From<TestError> for ApiErrorResponse {
    fn from(e: TestError) -> Self {
        use axum::http::StatusCode;
        ApiErrorResponse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

// 把错误转成 HTTP 响应，返回 JSON 格式
impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "success": false,
            "error": self.1
        });
        (self.0, Json(body)).into_response()
    }
}

// 让标准库的 Display trait 可以格式化这个错误
impl fmt::Display for ApiErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.0, self.1)
    }
}

// 让这个错误可以被 ? 操作符传播
impl fmt::Debug for ApiErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ApiError({}, {})", self.0, self.1)
    }
}
