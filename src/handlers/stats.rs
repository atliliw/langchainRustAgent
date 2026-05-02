//! API 调用统计监控
//!
//! GET /api/monitor/stats
//! 返回每种 API 的调用次数、token 消耗、耗时等统计数据
//! 数据从 SQLite 的 api_stats 表中查询

use crate::handlers::{AppState, ApiErrorResponse};
use axum::{
    extract::State,
    Json,
};
use std::sync::Arc;

/// 获取 API 调用统计
/// 包含: 总调用数、成功/失败、今日/本周趋势、各API明细、最近20次调用
pub async fn get_api_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::stores::ApiStatsSummary>, ApiErrorResponse> {
    let stats = state.api.get_api_stats().await?;
    Ok(Json(stats))
}
