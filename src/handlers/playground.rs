use crate::handlers::{ApiErrorResponse, AppState};
use crate::services::tool_calling::{ToolCallingEngine, ChatMessage};
use crate::services::mcp::mcp_client::{McpClient, McpServerConfig};
use crate::services::evaluate::EvaluateEngine;
use crate::services::vision::{VisionService, VisionRequest};
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::{Stream, StreamExt};
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub messages: Option<Vec<ChatMessage>>,
    pub message: Option<String>,
    pub tools: Option<bool>,
    pub stream: Option<bool>,
    pub rag: Option<String>,
}

// ── Chat ──

pub async fn v2_chat(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiErrorResponse> {
    let messages = if let Some(msgs) = request.messages {
        msgs
    } else if let Some(msg) = request.message {
        vec![ChatMessage { role: "user".into(), content: msg }]
    } else {
        return Err(ApiErrorResponse(
            axum::http::StatusCode::BAD_REQUEST,
            "缺少 messages 或 message".to_string(),
        ));
    };
    let rag_context = request.rag.unwrap_or_default();
    let config = state.config.clone();
    let stream = ToolCallingEngine::chat(config, messages, rag_context);
    let sse_stream = stream.map(|s| Ok(Event::default().data(s)));
    Ok(Sse::new(sse_stream))
}

// ── Tools ──

pub async fn v2_tools() -> Json<serde_json::Value> {
    let registry = crate::services::tools::ToolRegistry::default_registry();
    let tools: Vec<serde_json::Value> = registry.list_descriptions().iter().map(|(name, desc)| {
        serde_json::json!({"name": name, "description": desc, "parameters": {
            "type": "object", "properties": {"query": {"type": "string", "description": desc}},
            "required": ["query"]
        }})
    }).collect();
    Json(serde_json::json!({"tools": tools}))
}

// ── MCP ──

pub async fn mcp_connect(
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let name = request["name"].as_str().unwrap_or("").to_string();
    let url = request["url"].as_str().unwrap_or("").to_string();
    let api_key = request["api_key"].as_str().map(|s| s.to_string());
    if name.is_empty() || url.is_empty() {
        return Err(ApiErrorResponse(axum::http::StatusCode::BAD_REQUEST, "缺少 name 或 url".into()));
    }
    let config = McpServerConfig { name: name.clone(), url, api_key };
    match McpClient::list_tools(&config).await {
        Ok(tools) => Ok(Json(serde_json::json!({
            "success": true, "server": name, "tools": tools.iter().map(|t| serde_json::json!({
                "name": t.name, "description": t.description
            })).collect::<Vec<_>>()
        }))),
        Err(e) => Err(ApiErrorResponse(axum::http::StatusCode::BAD_GATEWAY, e)),
    }
}

pub async fn mcp_list_tools(
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let url = request["url"].as_str().unwrap_or("").to_string();
    let api_key = request["api_key"].as_str().map(|s| s.to_string());
    if url.is_empty() {
        return Err(ApiErrorResponse(axum::http::StatusCode::BAD_REQUEST, "缺少 url".into()));
    }
    let config = McpServerConfig { name: "remote".into(), url, api_key };
    match McpClient::list_tools(&config).await {
        Ok(tools) => Ok(Json(serde_json::json!({"tools": tools}))),
        Err(e) => Err(ApiErrorResponse(axum::http::StatusCode::BAD_GATEWAY, e)),
    }
}

pub async fn mcp_call_tool(
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let url = request["url"].as_str().unwrap_or("").to_string();
    let tool = request["tool"].as_str().unwrap_or("").to_string();
    let args = request["args"].clone();
    let api_key = request["api_key"].as_str().map(|s| s.to_string());
    let config = McpServerConfig { name: "remote".into(), url, api_key };
    match McpClient::call_tool(&config, &tool, args).await {
        Ok(result) => Ok(Json(serde_json::json!({"result": result}))),
        Err(e) => Err(ApiErrorResponse(axum::http::StatusCode::BAD_GATEWAY, e)),
    }
}

// ── Evaluate ──

pub async fn evaluate_run(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let question = request["question"].as_str().unwrap_or("");
    let answer = request["answer"].as_str().unwrap_or("");
    let context = request["context"].as_str().unwrap_or("");
    if question.is_empty() || answer.is_empty() {
        return Err(ApiErrorResponse(axum::http::StatusCode::BAD_REQUEST, "缺少 question 或 answer".into()));
    }
    let result = EvaluateEngine::full_evaluation(question, answer, context, &state.config).await;
    Ok(Json(result))
}

pub async fn evaluate_compare(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let question = request["question"].as_str().unwrap_or("");
    let base = request["base_answer"].as_str().unwrap_or("");
    let new = request["new_answer"].as_str().unwrap_or("");
    let result = EvaluateEngine::compare_evaluation(base, new, question, &state.config).await;
    Ok(Json(result))
}

// ── Vision ──

pub async fn vision_analyze(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let image_url = request["image_url"].as_str().unwrap_or("").to_string();
    let question = request["question"].as_str().unwrap_or("").to_string();
    if image_url.is_empty() || question.is_empty() {
        return Err(ApiErrorResponse(axum::http::StatusCode::BAD_REQUEST, "缺少 image_url 或 question".into()));
    }
    let req = VisionRequest { image_url, question, model: request["model"].as_str().map(|s| s.to_string()) };
    match VisionService::analyze(req, &state.config).await {
        Ok(result) => Ok(Json(serde_json::json!({"result": result}))),
        Err(e) => Err(ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

// ── Cost / Stats ──

pub async fn v2_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let pool = state.api.conversation_store.pool();
    let tracker = crate::services::cost_tracker::CostTracker::new(pool);
    match tracker.get_stats().await {
        Ok(stats) => Ok(Json(stats)),
        Err(e) => Ok(Json(serde_json::json!({"error": e.to_string(), "total_tokens": 0, "total_cost": 0.0}))),
    }
}

pub async fn v2_record_cost(
    State(state): State<Arc<AppState>>,
    Json(request): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let pool = state.api.conversation_store.pool();
    let tracker = crate::services::cost_tracker::CostTracker::new(pool);
    let _ = tracker.ensure_table().await;
    let model = request["model"].as_str().unwrap_or("unknown");
    let input_tokens = request["input_tokens"].as_u64().unwrap_or(0);
    let output_tokens = request["output_tokens"].as_u64().unwrap_or(0);
    let endpoint = request["endpoint"].as_str().unwrap_or("unknown");
    let _ = tracker.record(model, input_tokens, output_tokens, endpoint).await;
    Ok(Json(serde_json::json!({"success": true})))
}
