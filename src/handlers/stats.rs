//! API 统计监控处理函数

use crate::handlers::{AppState, ApiErrorResponse};
use axum::{
    extract::State,
    Json,
};
use std::sync::Arc;

pub async fn get_api_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::stores::ApiStatsSummary>, ApiErrorResponse> {
    let stats = state.api.get_api_stats().await?;
    Ok(Json(stats))
}