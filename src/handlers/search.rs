//! 搜索处理函数
//! 
//! 提供 4 种搜索 API：
//!   /api/search/vector    向量检索（语义匹配）
//!   /api/search/bm25      BM25 检索（关键词匹配）
//!   /api/search/hybrid    混合检索（RRF 融合）
//!   /api/search/compare   三种搜索对比测试
//!
//! 以及统计和清空功能

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use axum::{
    extract::State,
    Json,
};
use std::sync::Arc;

/// 向量检索
/// POST /api/search/vector
/// 用 Embedding 把用户问题转成向量，在 Qdrant 中找最相似的文档
pub async fn search_vector(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiErrorResponse> {
    let response = state.api.search_vector(request).await?;
    Ok(Json(response))
}

/// BM25 检索
/// POST /api/search/bm25
/// 用关键词匹配，在 MongoDB BM25 索引中搜索
pub async fn search_bm25(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiErrorResponse> {
    let response = state.api.search_bm25(request)?;
    Ok(Json(response))
}

/// 混合检索
/// POST /api/search/hybrid
/// 同时跑向量 + BM25，RRF 算法融合排名
pub async fn search_hybrid(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiErrorResponse> {
    let response = state.api.search_hybrid(request).await?;
    Ok(Json(response))
}

/// 对比三种检索
/// POST /api/search/compare
/// 同时跑向量、BM25、混合三种检索，对比结果差异
/// 返回每种检索的结果 + 重叠文档数 + 分数对比
pub async fn compare_search(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CompareRequest>,
) -> Result<Json<CompareResponse>, ApiErrorResponse> {
    let response = state.api.compare_search(request.query, request.top_k).await?;
    Ok(Json(response))
}

/// PageIndex 全文检索
/// POST /api/search/pageindex
pub async fn search_pageindex(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<Vec<serde_json::Value>>, ApiErrorResponse> {
    let results = state.api.search_pageindex(&request.query, request.top_k).await?;
    Ok(Json(results))
}

/// 获取统计信息
/// GET /api/stats
/// 返回: 文档总数、向量维度、对话数等
pub async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatsResponse>, ApiErrorResponse> {
    let response = state.api.get_stats().await?;
    Ok(Json(response))
}

/// 清空所有数据
/// POST /api/clear
/// 删除向量库 + BM25 索引 + 对话历史
pub async fn clear_all(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    state.api.clear_all().await?;
    
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "所有文档已清空"
    })))
}
