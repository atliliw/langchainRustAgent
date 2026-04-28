//! 路由模块（前后端分离架构）

use crate::handlers::{
    aggregate, chat, document, langgraph, search, stats, test, upload, AppState,
};
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

pub fn create_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/upload", axum::routing::post(upload::upload_file))
        .route(
            "/api/search/vector",
            axum::routing::post(search::search_vector),
        )
        .route("/api/search/bm25", axum::routing::post(search::search_bm25))
        .route(
            "/api/search/hybrid",
            axum::routing::post(search::search_hybrid),
        )
        .route(
            "/api/search/compare",
            axum::routing::post(search::compare_search),
        )
        .route("/api/stats", axum::routing::get(search::get_stats))
        .route("/api/clear", axum::routing::post(search::clear_all))
        .route(
            "/api/test/precision",
            axum::routing::post(test::run_precision_test),
        )
        .route("/api/test/cases", axum::routing::get(test::get_test_cases))
        .route("/api/chat", axum::routing::post(chat::chat))
        .route("/api/chat/stream", axum::routing::post(chat::chat_stream))
        .route(
            "/api/chat/history/:session_id",
            axum::routing::get(chat::get_chat_history),
        )
        .route("/api/chat/sessions", axum::routing::get(chat::get_sessions))
        .route(
            "/api/chat/clear/:session_id",
            axum::routing::post(chat::clear_session),
        )
        .route(
            "/api/chat/message/:message_id",
            axum::routing::put(chat::edit_message),
        )
        .route(
            "/api/chat/message/:message_id",
            axum::routing::delete(chat::delete_message),
        )
        .route(
            "/api/chat/compress-modes",
            axum::routing::get(chat::get_compress_modes),
        )
        .route(
            "/api/langgraph/info",
            axum::routing::get(langgraph::get_langgraph_info),
        )
        .route(
            "/api/langgraph/parallel",
            axum::routing::post(langgraph::run_langgraph_parallel),
        )
        .route(
            "/api/langgraph/conditional",
            axum::routing::post(langgraph::run_langgraph_conditional),
        )
        .route(
            "/api/langgraph/stream",
            axum::routing::post(langgraph::run_langgraph_stream),
        )
        .route(
            "/api/documents",
            axum::routing::get(document::list_documents),
        )
        .route(
            "/api/documents/:parent_id",
            axum::routing::post(document::delete_document),
        )
        .route(
            "/api/aggregate/collect",
            axum::routing::post(aggregate::collect),
        )
        .route("/api/aggregate/list", axum::routing::get(aggregate::list))
        .route(
            "/api/aggregate/search",
            axum::routing::post(aggregate::search),
        )
        .route("/api/aggregate/stats", axum::routing::get(aggregate::stats))
        .route(
            "/api/monitor/stats",
            axum::routing::get(stats::get_api_stats),
        )
        .layer(cors)
        .with_state(state)
}
