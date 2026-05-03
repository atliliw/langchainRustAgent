//! 对话处理函数（Chat API）
//!
//! 本文件把 HTTP 请求转成业务调用，主要做 3 件事：
//! 1. 解析 HTTP 请求参数
//! 2. 调 ApiService 处理业务
//! 3. 包装响应返回给前端
//!
//! 包含: 普通对话、流式对话、历史查询、会话管理、消息编辑/删除/重生成、导入/导出、分支

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use crate::stores::HybridSearchResult;
use axum::{
    extract::{State, Path},
    Json,
    response::{sse::{Event, Sse}},
};
use serde::Deserialize;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Instant;

/// 普通对话（非流式）
/// POST /api/chat
/// 发消息 → 等全部回答生成完 → 一次性返回
pub async fn chat(
    State(state): State<Arc<AppState>>,        // 从 Axum 提取全局状态
    Json(request): Json<ChatRequest>,          // 从 HTTP Body 解析请求
) -> Result<Json<ChatResponse>, ApiErrorResponse> {
    let start = Instant::now();
    // 调 API 服务处理对话
    let result = state.api.chat(request).await;
    
    match result {
        Ok(response) => {
            // 记录这次调用的统计数据（成功）
            let duration = start.elapsed().as_millis() as i64;
            let tokens = crate::stores::estimate_tokens(&response.reply) as i64;
            if let Err(e) = state.api.record_api_call("chat", tokens, duration, true).await {
                tracing::error!("记录API统计失败: {}", e);
            }
            Ok(Json(response))
        },
        Err(e) => {
            // 记录这次调用的统计数据（失败）
            let duration = start.elapsed().as_millis() as i64;
            if let Err(e2) = state.api.record_api_call("chat", 0, duration, false).await {
                tracing::error!("记录API统计失败: {}", e2);
            }
            Err(ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

/// 流式对话（SSE 逐 token 返回）
/// POST /api/chat/stream
/// 发消息 → 逐 token 返回（打字机效果）→ 最后发 [DONE]
///
/// SSE = Server-Sent Events，服务端向客户端推送事件
/// 每个事件格式:
///   event: token     ← 事件类型
///   data: 你好       ← 事件内容（每一条数据）
pub async fn chat_stream(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiErrorResponse> {
    use crate::models::SearchMode;
    
    let start = Instant::now();
    // 根据前端传的 use_vector / use_bm25 决定搜索模式
    let search_mode = SearchMode::from_flags(request.use_vector, request.use_bm25);
    
    // 根据搜索模式查询知识库，获取 RAG 上下文
    let rag_sources = match search_mode {
        SearchMode::None => Vec::new(),                    // 不检索
        SearchMode::Vector => {                            // 向量检索
            let results = state.api.vector_store.search(&request.message, request.top_k).await
                .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            results.into_iter().map(|r| SourceInfo {
                content: r.document.content.clone(),
                score: r.score,
                source: "vector".to_string(),
            }).collect()
        },
        SearchMode::BM25 => {                              // BM25 关键词检索
            let results = state.api.bm25_store.search(&request.message, request.top_k)
                .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            results.into_iter().map(|r| SourceInfo {
                content: r.content.clone(),
                score: r.score,
                source: "bm25".to_string(),
            }).collect()
        },
        SearchMode::Hybrid => {                            // 混合检索（RRF）
            let results = state.api.hybrid_store.search(&request.message, request.top_k).await
                .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            results.into_iter().map(|r: HybridSearchResult| SourceInfo {
                content: r.content.clone(),
                score: r.rrf_score,
                source: r.source.clone(),
            }).collect()
        },
    };
    
    // 调对话引擎开始流式生成
    let (session_id, mut token_stream) = state.api.conversation_store.chat_stream(request.clone(), rag_sources).await
        .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let session_id_clean = session_id.trim().to_string();
    
    let user_msg = request.message.clone();
    let store = state.api.conversation_store.clone();
    let api = state.api.clone();
    let start_time = start;
    let compress_mode_str = request.compress_mode.clone();
    
    // 创建一个异步流，逐 token 返回给前端
    let stream = async_stream::stream! {
        let mut full_reply = String::new();
        let sid = session_id_clean.clone();
        let mut success = true;
        
        // 事件 1: 发送 session_id（前端拿这个查历史）
        yield Ok(Event::default().event("session").data(&sid));
        
        // 事件 2: 发送当前使用的模式（前端显示用）
        yield Ok(Event::default().event("mode").data(format!("{},{},{},{}", request.use_vector, request.use_bm25, match search_mode {
            SearchMode::None => "none",
            SearchMode::Vector => "vector",
            SearchMode::BM25 => "bm25",
            SearchMode::Hybrid => "hybrid",
        }, request.compress_mode)));
        
        // 事件 3+: 逐 token 流式输出
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
        
        // 流式结束后，把完整消息存到 SQLite
        store.save_full_message(&sid, &user_msg, &full_reply).await.ok();
        
        // 后台压缩并持久化
        let compress_mode = CompressMode::from_str(&compress_mode_str);
        if compress_mode != CompressMode::None {
            tracing::info!("开始压缩持久化: sid={}, mode={:?}", sid, compress_mode);
            match store.compress_and_persist(&sid, compress_mode).await {
                Ok(()) => tracing::info!("压缩持久化完成: sid={}", sid),
                Err(e) => tracing::error!("压缩持久化失败: sid={}, err={:?}", sid, e),
            }
        }
        
        // 记录统计
        let duration = start_time.elapsed().as_millis() as i64;
        let tokens = crate::stores::estimate_tokens(&full_reply) as i64;
        if let Err(e) = api.record_api_call("chat_stream", tokens, duration, success).await {
            tracing::error!("记录chat_stream统计失败: {}", e);
        }
        
        // 最终事件：通知前端流结束了
        yield Ok(Event::default().event("done").data("[DONE]"));
    };
    
Ok(Sse::new(stream))
}

/// 获取对话历史
/// GET /api/chat/history/:session_id
pub async fn get_chat_history(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<ConversationMessage>>, ApiErrorResponse> {
    let history = state.api.get_conversation_history(&session_id).await?;
    Ok(Json(history))
}

/// 获取全部会话列表
/// GET /api/chat/sessions
/// 按更新时间倒序，最多 20 条
pub async fn get_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SessionInfo>>, ApiErrorResponse> {
    let sessions = state.api.get_sessions().await?;
    Ok(Json(sessions))
}

/// 获取可用的压缩模式列表
/// GET /api/chat/compress-modes
/// 返回: ["none", "sliding_window", "token_limit", "summary", "layered"]
pub async fn get_compress_modes() -> Json<Vec<CompressModeInfo>> {
    Json(crate::stores::ConversationStore::get_compress_modes())
}

/// 清空会话
/// POST /api/chat/clear/:session_id
/// 删除该会话及所有消息
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

/// 编辑消息
/// PUT /api/chat/message/:message_id
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

/// 删除消息
/// DELETE /api/chat/message/:message_id
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

/// 重新生成 AI 回复
/// POST /api/chat/message/:message_id/regenerate
/// 删除原回答 → 重新调 LLM → 保存新回答 → 返回
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

/// 导出会话
/// GET /api/chat/session/:session_id/export
/// 返回 JSON 格式的完整会话（含所有消息）
pub async fn export_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionExport>, ApiErrorResponse> {
    let export = state.api.export_session(&session_id).await?;
    Ok(Json(export))
}

/// 导入会话
/// POST /api/chat/session/import
/// 从 JSON 恢复会话
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

/// 搜索会话
/// POST /api/chat/sessions/search
/// 按消息内容或会话标题模糊匹配
pub async fn search_sessions(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SessionSearchRequest>,
) -> Result<Json<Vec<SessionInfo>>, ApiErrorResponse> {
    let sessions = state.api.search_sessions(&request.query).await?;
    Ok(Json(sessions))
}

/// 分支会话
/// POST /api/chat/session/branch
/// 从某条消息的位置分叉出一个新会话（类似 Git 分支）
pub async fn branch_session(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BranchRequest>,
) -> Result<Json<BranchResponse>, ApiErrorResponse> {
    let response = state.api.branch_session(&request.session_id, &request.from_message_id).await?;
    Ok(Json(response))
}

/// 获取重要上下文
/// GET /api/chat/context/:session_id
#[derive(Deserialize)]
pub struct SetContextRequest {
    pub context: String,
}

pub async fn get_important_context(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let context = state.api.conversation_store.get_important_context(&session_id).await
        .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "context": context })))
}

pub async fn set_important_context(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(request): Json<SetContextRequest>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    state.api.conversation_store.set_important_context(&session_id, &request.context).await
        .map_err(|e| ApiErrorResponse(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}
