//! 对话处理函数

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use crate::stores::HybridSearchResult;
use axum::{
    extract::{State, Path},
    Json,
    response::{sse::{Event, Sse}},
};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Instant;

pub async fn chat(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ApiErrorResponse> {
    let start = Instant::now();
    let result = state.api.chat(request).await;
    
    match result {
        Ok(response) => {
            let duration = start.elapsed().as_millis() as i64;
            let tokens = crate::stores::estimate_tokens(&response.reply) as i64;
            state.api.record_api_call("chat", tokens, duration, true).await.ok();
            Ok(Json(response))
        },
        Err(e) => {
            state.api.record_api_call("chat", 0, start.elapsed().as_millis() as i64, false).await.ok();
            Err(ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn chat_stream(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiErrorResponse> {
    use crate::models::SearchMode;
    
    let start = Instant::now();
    let search_mode = SearchMode::from_flags(request.use_vector, request.use_bm25);
    
    let rag_sources = match search_mode {
        SearchMode::None => Vec::new(),
        SearchMode::Vector => {
            let results = state.api.vector_store.search(&request.message, request.top_k).await
                .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            results.into_iter().map(|r| SourceInfo {
                content: r.document.content.clone(),
                score: r.score,
                source: "vector".to_string(),
            }).collect()
        },
        SearchMode::BM25 => {
            let results = state.api.bm25_store.search(&request.message, request.top_k)
                .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            results.into_iter().map(|r| SourceInfo {
                content: r.content.clone(),
                score: r.score,
                source: "bm25".to_string(),
            }).collect()
        },
        SearchMode::Hybrid => {
            let results = state.api.hybrid_store.search(&request.message, request.top_k).await
                .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            results.into_iter().map(|r: HybridSearchResult| SourceInfo {
                content: r.content.clone(),
                score: r.rrf_score,
                source: r.source.clone(),
            }).collect()
        },
    };
    
    let (session_id, mut token_stream) = state.api.conversation_store.chat_stream(request.clone(), rag_sources).await
        .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let session_id_clean = session_id.trim().to_string();
    
    let user_msg = request.message.clone();
    let store = state.api.conversation_store.clone();
    let api = state.api.clone();
    let start_time = start;
    
    let stream = async_stream::stream! {
        let mut full_reply = String::new();
        let sid = session_id_clean.clone();
        let mut success = true;
        
        yield Ok(Event::default().event("session").data(&sid));
        
        yield Ok(Event::default().event("mode").data(format!("{},{},{},{}", request.use_vector, request.use_bm25, match search_mode {
            SearchMode::None => "none",
            SearchMode::Vector => "vector",
            SearchMode::BM25 => "bm25",
            SearchMode::Hybrid => "hybrid",
        }, request.compress_mode)));
        
        while let Some(token_result) = token_stream.next().await {
            match token_result {
                Ok(token) => {
                    full_reply.push_str(&token);
                    yield Ok(Event::default().event("token").data(&token));
                }
                Err(e) => {
                    success = false;
                    yield Ok(Event::default().event("error").data(e.to_string()));
                    break;
                }
            }
        }
        
        store.save_full_message(&sid, &user_msg, &full_reply).await.ok();
        
        let duration = start_time.elapsed().as_millis() as i64;
        let tokens = crate::stores::estimate_tokens(&full_reply) as i64;
        api.record_api_call("chat_stream", tokens, duration, success).await.ok();
        
        yield Ok(Event::default().event("done").data("[DONE]"));
    };
    
Ok(Sse::new(stream))
}

pub async fn get_chat_history(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<ConversationMessage>>, ApiErrorResponse> {
    let history = state.api.get_conversation_history(&session_id).await?;
    Ok(Json(history))
}

pub async fn get_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SessionInfo>>, ApiErrorResponse> {
    let sessions = state.api.get_sessions().await?;
    Ok(Json(sessions))
}

pub async fn get_compress_modes() -> Json<Vec<CompressModeInfo>> {
    Json(crate::stores::ConversationStore::get_compress_modes())
}

pub async fn clear_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    state.api.clear_session(&session_id).await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("会话 {} 已清空", session_id)
    })))
}

pub async fn edit_message(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
    Json(request): Json<EditMessageRequest>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    state.api.edit_message(&message_id, &request.content).await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "消息已更新"
    })))
}

pub async fn delete_message(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    state.api.delete_message(&message_id).await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "消息已删除"
    })))
}

pub async fn regenerate_message(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<RegenerateResponse>, ApiErrorResponse> {
    let start = Instant::now();
    let result = state.api.regenerate_message(&message_id).await;
    
    match result {
        Ok(response) => {
            let duration = start.elapsed().as_millis() as i64;
            let tokens = crate::stores::estimate_tokens(&response.reply) as i64;
            state.api.record_api_call("regenerate", tokens, duration, true).await.ok();
            Ok(Json(response))
        },
        Err(e) => {
            state.api.record_api_call("regenerate", 0, start.elapsed().as_millis() as i64, false).await.ok();
            Err(ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn export_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionExport>, ApiErrorResponse> {
    let export = state.api.export_session(&session_id).await?;
    Ok(Json(export))
}

pub async fn import_session(
    State(state): State<Arc<AppState>>,
    Json(import): Json<SessionImport>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let session_id = state.api.import_session(import).await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "session_id": session_id,
        "message": "会话导入成功"
    })))
}

pub async fn search_sessions(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SessionSearchRequest>,
) -> Result<Json<Vec<SessionInfo>>, ApiErrorResponse> {
    let sessions = state.api.search_sessions(&request.query).await?;
    Ok(Json(sessions))
}

pub async fn branch_session(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BranchRequest>,
) -> Result<Json<BranchResponse>, ApiErrorResponse> {
    let response = state.api.branch_session(&request.session_id, &request.from_message_id).await?;
    Ok(Json(response))
}