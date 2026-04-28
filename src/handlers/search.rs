//! 搜索处理函数

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use axum::{
    extract::State,
    Json,
};
use std::sync::Arc;

pub async fn search_vector(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiErrorResponse> {
    let response = state.api.search_vector(request).await?;
    Ok(Json(response))
}

pub async fn search_bm25(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiErrorResponse> {
    let response = state.api.search_bm25(request)?;
    Ok(Json(response))
}

pub async fn search_hybrid(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiErrorResponse> {
    let response = state.api.search_hybrid(request).await?;
    Ok(Json(response))
}

pub async fn compare_search(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CompareRequest>,
) -> Result<Json<CompareResponse>, ApiErrorResponse> {
    let response = state.api.compare_search(request.query, request.top_k).await?;
    Ok(Json(response))
}

pub async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatsResponse>, ApiErrorResponse> {
    let response = state.api.get_stats().await?;
    Ok(Json(response))
}

pub async fn clear_all(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    state.api.clear_all().await?;
    
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "所有文档已清空"
    })))
}