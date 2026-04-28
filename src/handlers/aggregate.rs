//! 聚合内容 API 处理函数

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use crate::services::AggregateService;
use crate::errors::AgentError;
use axum::{
    extract::{State, Query, Json},
    response::Json as AxumJson,
};
use std::sync::Arc;

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

pub async fn stats(
    State(state): State<Arc<AppState>>,
) -> Result<AxumJson<AggregateStatsResponse>, ApiErrorResponse> {
    let service = AggregateService::new(state.config.clone()).await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let response = service.stats().await
        .map_err(|e: AgentError| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(AxumJson(response))
}