//! LangGraph 演示处理函数

use crate::handlers::ApiErrorResponse;
use crate::models::*;
use axum::Json;

pub async fn get_langgraph_info() -> Json<serde_json::Value> {
    Json(crate::services::ApiService::get_langgraph_info())
}

pub async fn run_langgraph_parallel(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<ParallelDemoResult>, ApiErrorResponse> {
    let result = crate::services::ApiService::run_langgraph_parallel(request.input).await?;
    Ok(Json(result))
}

pub async fn run_langgraph_conditional(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<ConditionalDemoResult>, ApiErrorResponse> {
    let result = crate::services::ApiService::run_langgraph_conditional(request.input).await?;
    Ok(Json(result))
}

pub async fn run_langgraph_stream(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<Vec<StreamDemoEvent>>, ApiErrorResponse> {
    let result = crate::services::ApiService::run_langgraph_stream(request.input).await?;
    Ok(Json(result))
}