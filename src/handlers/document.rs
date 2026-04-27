//! 文档管理处理函数

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use axum::{
    extract::{State, Path},
    Json,
};
use std::sync::Arc;

pub async fn list_documents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DocumentInfo>>, ApiErrorResponse> {
    let documents = state.api.list_documents().await?;
    Ok(Json(documents))
}

pub async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path(parent_id): Path<String>,
    Json(request): Json<DeleteDocumentRequest>,
) -> Result<Json<DeleteDocumentResponse>, ApiErrorResponse> {
    let result = state.api.delete_document(&parent_id, &request.filename).await?;
    Ok(Json(result))
}