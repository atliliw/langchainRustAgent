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

pub async fn batch_delete_documents(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchDeleteRequest>,
) -> Result<Json<BatchDeleteResponse>, ApiErrorResponse> {
    let result = state.api.batch_delete_documents(request.parent_ids).await?;
    Ok(Json(result))
}

pub async fn add_document_tags(
    State(state): State<Arc<AppState>>,
    Json(request): Json<DocumentTagRequest>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    state.api.add_document_tags(&request.parent_id, &request.tags).await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "标签已添加"
    })))
}

pub async fn get_documents_by_tag(
    State(state): State<Arc<AppState>>,
    Path(tag): Path<String>,
) -> Result<Json<Vec<DocumentInfo>>, ApiErrorResponse> {
    let documents = state.api.get_documents_by_tag(&tag).await?;
    Ok(Json(documents))
}