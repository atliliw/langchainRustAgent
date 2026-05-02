//! 文档管理处理函数
//! 
//! 提供文档的增删改查 API：
//!   GET    /api/documents             列表
//!   POST   /api/documents/:id         删除
//!   POST   /api/documents/batch-delete 批量删除
//!   POST   /api/documents/tags        加标签
//!   GET    /api/documents/tag/:tag    按标签查

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use axum::{
    extract::{State, Path},
    Json,
};
use std::sync::Arc;

/// 获取文档列表
/// GET /api/documents
pub async fn list_documents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DocumentInfo>>, ApiErrorResponse> {
    let documents = state.api.list_documents().await?;
    Ok(Json(documents))
}

/// 删除单个文档
/// POST /api/documents/:parent_id
pub async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path(parent_id): Path<String>,
    Json(request): Json<DeleteDocumentRequest>,
) -> Result<Json<DeleteDocumentResponse>, ApiErrorResponse> {
    let result = state.api.delete_document(&parent_id, &request.filename).await?;
    Ok(Json(result))
}

/// 批量删除文档
/// POST /api/documents/batch-delete
pub async fn batch_delete_documents(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchDeleteRequest>,
) -> Result<Json<BatchDeleteResponse>, ApiErrorResponse> {
    let result = state.api.batch_delete_documents(request.parent_ids).await?;
    Ok(Json(result))
}

/// 给文档添加标签
/// POST /api/documents/tags
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

/// 按标签查询文档
/// GET /api/documents/tag/:tag
pub async fn get_documents_by_tag(
    State(state): State<Arc<AppState>>,
    Path(tag): Path<String>,
) -> Result<Json<Vec<DocumentInfo>>, ApiErrorResponse> {
    let documents = state.api.get_documents_by_tag(&tag).await?;
    Ok(Json(documents))
}
