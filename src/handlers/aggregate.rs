//! 聚合内容 API 处理函数
//!
//! Agent 数据采集相关：
//!   POST /api/aggregate/collect    从多个数据源采集信息
//!   GET  /api/aggregate/list       查看已采集的数据
//!   POST /api/aggregate/search     在采集的数据中搜索
//!   GET  /api/aggregate/stats      采集统计

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use crate::services::AggregateService;
use crate::errors::AgentError;
use axum::{
    extract::{State, Query, Json},
    response::Json as AxumJson,
};
use std::sync::Arc;

/// 采集数据
/// POST /api/aggregate/collect
/// 从 GitHub、HackerNews、RSS 订阅、ArXiv 论文等渠道抓取最新 AI 相关资讯
/// sources 参数指定要采集的渠道，不传则全部采集
pub async fn collect(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CollectRequest>,
) -> Result<AxumJson<CollectResponse>, ApiErrorResponse> {
    let service = AggregateService::new(state.config.clone()).await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let response = service.collect(request).await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(AxumJson(response))
}

/// 查看采集列表
/// GET /api/aggregate/list
/// 可选 source 过滤（如 ?source=github），limit/offset 分页
pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AggregateListQuery>,
) -> Result<AxumJson<AggregateListResponse>, ApiErrorResponse> {
    let service = AggregateService::new(state.config.clone()).await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);
    
    let response = service.list(query.source.as_deref(), limit, offset).await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(AxumJson(response))
}

/// 在采集数据中搜索
/// POST /api/aggregate/search
/// 用简单的关键词匹配搜索已采集的文章
pub async fn search(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AggregateSearchRequest>,
) -> Result<AxumJson<AggregateSearchResponse>, ApiErrorResponse> {
    let service = AggregateService::new(state.config.clone()).await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let response = service.search(request).await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(AxumJson(response))
}

/// 获取采集统计
/// GET /api/aggregate/stats
/// 返回: 总量、按来源分布、最后一次采集时间
pub async fn stats(
    State(state): State<Arc<AppState>>,
) -> Result<AxumJson<AggregateStatsResponse>, ApiErrorResponse> {
    let service = AggregateService::new(state.config.clone()).await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let response = service.stats().await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(AxumJson(response))
}
