use crate::handlers::{ApiErrorResponse, AppState};
use crate::services::tool_calling::{ToolCallingEngine, ChatMessage};
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

/// POST /api/v2/chat
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

/// GET /api/v2/tools
pub async fn v2_tools(
) -> Json<serde_json::Value> {
    let registry = crate::services::tools::ToolRegistry::default_registry();
    let tools: Vec<serde_json::Value> = registry.list_descriptions().iter().map(|(name, desc)| {
        serde_json::json!({
            "name": name,
            "description": desc,
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": desc }
                },
                "required": ["query"]
            }
        })
    }).collect();
    Json(serde_json::json!({"tools": tools}))
}
