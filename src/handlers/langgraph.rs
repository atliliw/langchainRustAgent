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
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::{self, Stream, StreamExt};
use std::convert::Infallible;
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

/// 子图演示
/// POST /api/langgraph/subgraph
pub async fn run_langgraph_subgraph(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<SubgraphDemoResult>, ApiErrorResponse> {
    let result = crate::services::LangGraphDemoService::run_subgraph_demo(request.input).await
        .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(result))
}

/// LLM 条件路由演示
/// POST /api/langgraph/llm_conditional
pub async fn run_langgraph_llm_conditional(
    State(state): State<Arc<AppState>>,
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<LLMConditionalResult>, ApiErrorResponse> {
    let result = crate::services::LangGraphDemoService::run_llm_conditional_demo(
        &state.config, request.input,
    ).await.map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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

/// ──────── PageIndex ────────

pub async fn pageindex_build(
    State(state): State<Arc<AppState>>,
    Json(request): Json<crate::services::pageindex::BuildRequest>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let idx = crate::services::pageindex::PageIndex::build_from_text(
        &state.api.pageindex_store, &request.doc_id, &request.title, &request.text, None,
    ).await
        .map_err(|e| ApiErrorResponse(axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::json!({"success":true,"doc_id":idx.doc_id,"node_count":idx.node_count})))
}

pub async fn pageindex_search(
    State(state): State<Arc<AppState>>,
    Json(request): Json<crate::services::pageindex::SearchRequest>,
) -> Result<Json<crate::services::pageindex::SearchResponse>, ApiErrorResponse> {
    let result = crate::services::pageindex::PageIndex::search(&state.config, &state.api.pageindex_store, &request).await
        .map_err(|e| ApiErrorResponse(axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
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
/// 请求: { task: "用户任务", use_rag: true/false }
pub async fn agent_plan(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<AgentPlan>, ApiErrorResponse> {
    let task = request["task"].as_str().unwrap_or("").to_string();
    let use_rag = request["use_rag"].as_bool().unwrap_or(false);
    let use_routing = request["use_routing"].as_bool().unwrap_or(false);
    let use_subgraph = request["use_subgraph"].as_bool().unwrap_or(false);
    let result = state.api.agent_plan(task, use_rag, use_routing, use_subgraph).await?;
    Ok(Json(result))
}

pub async fn agent_execute(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let task = request["task"].as_str().unwrap_or("").to_string();
    let tasks: Vec<AgentTask> = serde_json::from_value(request["agent_tasks"].clone())
        .unwrap_or_default();
    let use_rag = request["use_rag"].as_bool().unwrap_or(false);
    let use_verify = request["use_verify"].as_bool().unwrap_or(false);
    let use_subgraph = request["use_subgraph"].as_bool().unwrap_or(false);
    let (sid, results, has_next) = state.api.agent_batch_start(task, tasks, use_rag, use_verify, use_subgraph).await?;
    Ok(Json(serde_json::json!({"session_id":sid,"results":results,"has_next":has_next})))
}

pub async fn agent_next(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let sid = request["session_id"].as_str().unwrap_or("");
    let (results, has_next) = state.api.agent_batch_next(sid).await?;
    Ok(Json(serde_json::json!({"results":results,"has_next":has_next})))
}

/// 查询执行日志
/// GET /api/agent/sessions/:id/logs
pub async fn agent_session_logs(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let result = state.api.agent_session_logs(&session_id).await?;
    Ok(Json(result))
}

/// Agent 执行进度推送（SSE）
/// GET /api/agent/progress/:session_id
pub async fn agent_progress(
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    use tokio::sync::broadcast;
    use futures_util::stream::BoxStream;

    let rx = crate::services::agent_executor::AgentEngine::get_progress_receiver(&session_id);

    let stream: BoxStream<Result<Event, Infallible>> = match rx {
        Ok(rx) => {
            let init = futures_util::stream::once(async {
                Ok(Event::default().data("connected").event("connected"))
            }).boxed();
            let events = futures_util::stream::unfold(rx, |mut rx| async {
                match rx.recv().await {
                    Ok(msg) => {
                        Some((Ok(Event::default().data(msg).event("progress")), rx))
                    }
                    Err(broadcast::error::RecvError::Closed) => None,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        Some((Ok(Event::default().data("lagged").event("error")), rx))
                    }
                }
            }).boxed();
            init.chain(events).boxed()
        }
        Err(_) => {
            futures_util::stream::once(async {
                Ok(Event::default().data("session not found").event("error"))
            }).boxed()
        }
    };

    Sse::new(stream)
}

/// Token 用量统计
/// GET /api/agent/stats
pub async fn agent_token_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let result = state.api.agent_token_stats().await?;
    Ok(Json(result))
}

/// 历史执行列表
/// GET /api/agent/sessions
pub async fn agent_list_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<serde_json::Value>>, ApiErrorResponse> {
    let result = state.api.agent_list_sessions().await?;
    Ok(Json(result))
}

/// 取消 Agent 执行
/// POST /api/agent/cancel
/// 请求: { session_id }
pub async fn agent_cancel(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let session_id = request["session_id"].as_str().unwrap_or("").to_string();
    state.api.agent_cancel(&session_id).await?;
    Ok(Json(serde_json::json!({"success": true})))
}

/// 查询待审核任务
/// POST /api/agent/pending
/// 请求: { session_id }
pub async fn agent_pending(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let session_id = request["session_id"].as_str().unwrap_or("").to_string();
    let pending = state.api.agent_pending_reviews(&session_id).await?;
    Ok(Json(serde_json::json!({"pending": pending})))
}

/// 人工审批
/// POST /api/agent/review
/// 请求: { session_id, task_name, approved, feedback? }
pub async fn agent_review(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let session_id = request["session_id"].as_str().unwrap_or("").to_string();
    let task_name = request["task_name"].as_str().unwrap_or("").to_string();
    let approved = request["approved"].as_bool().unwrap_or(false);
    let feedback = request["feedback"].as_str().unwrap_or("");
    state.api.agent_review(&session_id, &task_name, approved, feedback).await?;
    Ok(Json(serde_json::json!({"success": true})))
}

/// 执行所有任务（真正并行）
/// POST /api/agent/execute_all
pub async fn agent_execute_all(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<AgentExecResponse>, ApiErrorResponse> {
    let task = request["task"].as_str().unwrap_or("").to_string();
    let tasks: Vec<AgentTask> = serde_json::from_value(request["agent_tasks"].clone())
        .unwrap_or_default();
    let use_rag = request["use_rag"].as_bool().unwrap_or(false);
    let use_verify = request["use_verify"].as_bool().unwrap_or(false);
    let use_subgraph = request["use_subgraph"].as_bool().unwrap_or(false);
    let result = state.api.agent_execute_all(task, tasks, use_rag, use_verify, use_subgraph).await?;
    Ok(Json(result))
}
