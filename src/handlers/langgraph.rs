//! LangGraph 状态图演示处理函数
//!
//! LangGraph 是一个有向图执行引擎，用来编排多个 Agent 的执行流程。
//!
//! 三种演示模式：
//!   /api/langgraph/parallel     并行执行（FanOut → 多个任务同时跑）
//!   /api/langgraph/conditional  条件路由（根据输入动态选路径）
//!   /api/langgraph/stream       流式执行（实时推送执行进度）
//!
//! 本质：展示"多个Agent/任务怎么协作"，这是面试考察的重点

use crate::handlers::{ApiErrorResponse, AppState};
use crate::models::*;
use axum::{extract::State, Json};
use std::sync::Arc;

/// 获取 LangGraph 演示信息
/// GET /api/langgraph/info
/// 返回三种演示模式的说明（节点、边、特性）
pub async fn get_langgraph_info() -> Json<serde_json::Value> {
    Json(crate::services::ApiService::get_langgraph_info())
}

/// 并行执行演示
/// POST /api/langgraph/parallel
/// 演示: 1个分发器 → 3个并行任务 → 完成
/// 关键点: 3个任务同时跑，总耗时=最慢那个
pub async fn run_langgraph_parallel(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<ParallelDemoResult>, ApiErrorResponse> {
    let result = crate::services::ApiService::run_langgraph_parallel(request.input).await?;
    Ok(Json(result))
}

/// 条件路由演示
/// POST /api/langgraph/conditional
/// 演示: 根据输入长度(>10)自动选择"快速处理"或"详细分析"
/// 关键点: Agent 根据当前状态决定下一步
pub async fn run_langgraph_conditional(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<ConditionalDemoResult>, ApiErrorResponse> {
    let result = crate::services::ApiService::run_langgraph_conditional(request.input).await?;
    Ok(Json(result))
}

/// 流式执行演示
/// POST /api/langgraph/stream
/// 演示: 逐步执行 step1→step2→step3，实时推送事件
/// 关键点: 可以实时看到每个节点的执行结果
pub async fn run_langgraph_stream(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<Vec<StreamDemoEvent>>, ApiErrorResponse> {
    let result = crate::services::ApiService::run_langgraph_stream(request.input).await?;
    Ok(Json(result))
}

/// 获取图结构（含 Mermaid 可视化语法）
/// POST /api/langgraph/structure
/// 请求: { mode: "parallel" | "conditional" | "stream" }
/// 返回: { mode, mermaid, structure }
pub async fn get_langgraph_structure(
    Json(request): Json<LangGraphStructureRequest>,
) -> Result<Json<LangGraphStructureResponse>, ApiErrorResponse> {
    let result = crate::services::ApiService::get_langgraph_structure(request.mode)?;
    Ok(Json(result))
}

/// AI 任务拆解（不含执行）
/// POST /api/langgraph/decompose
/// 请求: { task: "用户任务" }
/// 返回: 图结构 + 子任务定义（无执行结果）
pub async fn decompose_task(
    State(state): State<Arc<AppState>>,
    Json(request): Json<TaskDecomposeRequest>,
) -> Result<Json<TaskDecomposeResult>, ApiErrorResponse> {
    let result = state.api.decompose_task(request.task).await?;
    Ok(Json(result))
}

/// 执行子任务
/// POST /api/langgraph/execute
/// 请求: { task: "原始任务", sub_tasks: [...] }
/// 返回: 每个子任务的执行结果 + token 统计
pub async fn execute_sub_tasks(
    State(state): State<Arc<AppState>>,
    Json(request): Json<TaskExecuteRequest>,
) -> Result<Json<TaskExecuteResult>, ApiErrorResponse> {
    let result = state.api.execute_sub_tasks(request.task, request.sub_tasks).await?;
    Ok(Json(result))
}

/// ──────── 真实 Agent 系统 ────────

/// Agent 规划
/// POST /api/agent/plan
pub async fn agent_plan(
    State(state): State<Arc<AppState>>,
    Json(request): Json<TaskDecomposeRequest>,
) -> Result<Json<AgentPlan>, ApiErrorResponse> {
    let result = state.api.agent_plan(request.task).await?;
    Ok(Json(result))
}

/// Agent 执行
/// POST /api/agent/execute
pub async fn agent_execute(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<AgentExecResponse>, ApiErrorResponse> {
    let task = request["task"].as_str().unwrap_or("").to_string();
    let tasks: Vec<AgentTask> = serde_json::from_value(request["agent_tasks"].clone())
        .map_err(|_| ApiErrorResponse(
            axum::http::StatusCode::BAD_REQUEST,
            "agent_tasks 格式错误".to_string(),
        ))?;
    let result = state.api.agent_execute(task, tasks).await?;
    Ok(Json(result))
}
