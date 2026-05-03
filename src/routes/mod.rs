//! 路由定义
//! 
//! 每个路由 = 前端访问的 URL 地址 + 对应的处理函数
//! 比如: POST /api/chat → chat::chat() 处理函数
//!
//! 路由分类：
//!   /api/upload/*          文件上传
//!   /api/search/*          搜索（向量/BM25/混合/对比）
//!   /api/chat/*            对话（普通/流式/历史/会话管理）
//!   /api/langgraph/*       LangGraph 状态图演示
//!   /api/documents/*       文档管理（列表/删除/标签）
//!   /api/aggregate/*       数据采集 Agent
//!   /api/test/*            测试
//!   /api/stats             统计
//!   /api/clear             清空

use crate::handlers::{
    aggregate, chat, document, langgraph, search, stats, test, upload, AppState,
};
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

/// 创建路由器
/// 把所有 URL 路径注册到对应的处理函数，返回给 main.rs 启动
pub fn create_router(state: Arc<AppState>) -> Router {
    // 配置 CORS（跨域请求），允许任何来源访问
    // 因为前后端分离，前端可能在不同域名或端口
    let cors = CorsLayer::new()
        .allow_origin(Any)      // 允许任何域名
        .allow_methods(Any)     // 允许任何 HTTP 方法
        .allow_headers(Any);    // 允许任何请求头

    Router::new()
        // ────────────────────── 文件上传 ──────────────────────
        .route("/api/upload", axum::routing::post(upload::upload_file))
        // 上传文件: POST /api/upload, multipart/form-data

        // ────────────────────── 搜索 ──────────────────────
        .route("/api/search/vector", axum::routing::post(search::search_vector))
        // 向量检索: 用语义相似度搜索
        
        .route("/api/search/bm25", axum::routing::post(search::search_bm25))
        // BM25 检索: 用关键词匹配搜索
        
        .route("/api/search/hybrid", axum::routing::post(search::search_hybrid))
        // 混合检索: RRF 算法融合向量 + BM25
        
        .route("/api/search/compare", axum::routing::post(search::compare_search))
        // 对比测试: 同时跑三种搜索，看结果差异

        // ────────────────────── 统计 & 清空 ──────────────────────
        .route("/api/stats", axum::routing::get(search::get_stats))
        // 获取统计: 文档数、向量维度、对话数等
        
        .route("/api/clear", axum::routing::post(search::clear_all))
        // 清空所有数据

        // ────────────────────── 精准度测试 ──────────────────────
        .route("/api/test/precision", axum::routing::post(test::run_precision_test))
        // 运行检索精准度测试
        
        .route("/api/test/cases", axum::routing::get(test::get_test_cases))
        // 获取默认测试用例

        // ────────────────────── 对话 ──────────────────────
        .route("/api/chat", axum::routing::post(chat::chat))
        // 普通对话: 发送消息, 等全部生成完返回
        
        .route("/api/chat/stream", axum::routing::post(chat::chat_stream))
        // 流式对话: SSE 逐 token 返回 (打字机效果)
        
        .route("/api/chat/history/:session_id", axum::routing::get(chat::get_chat_history))
        // 查看对话历史
        
        .route("/api/chat/sessions", axum::routing::get(chat::get_sessions))
        // 获取全部会话列表
        
        .route("/api/chat/clear/:session_id", axum::routing::post(chat::clear_session))
        // 清空指定会话
        
        .route("/api/chat/message/:message_id", axum::routing::put(chat::edit_message))
        // 编辑消息
        
        .route("/api/chat/message/:message_id", axum::routing::delete(chat::delete_message))
        // 删除消息
        
        .route("/api/chat/message/:message_id/regenerate", axum::routing::post(chat::regenerate_message))
        // 重新生成 AI 回复
        
        .route("/api/chat/session/:session_id/export", axum::routing::get(chat::export_session))
        // 导出会话 (JSON)
        
        .route("/api/chat/session/import", axum::routing::post(chat::import_session))
        // 导入会话 (JSON)
        
        .route("/api/chat/sessions/search", axum::routing::post(chat::search_sessions))
        // 搜索会话 (按内容模糊匹配)
        
        .route("/api/chat/session/branch", axum::routing::post(chat::branch_session))
        // 分支会话: 从某条消息分叉出新会话
        
        .route("/api/chat/compress-modes", axum::routing::get(chat::get_compress_modes))
        .route("/api/chat/context/:session_id", axum::routing::get(chat::get_important_context).put(chat::set_important_context))
        // 获取可用的压缩模式列表

        // ────────────────────── LangGraph 状态图演示 ──────────────────────
        .route("/api/langgraph/info", axum::routing::get(langgraph::get_langgraph_info))
        // 获取 LangGraph 演示信息
        
        .route("/api/langgraph/parallel", axum::routing::post(langgraph::run_langgraph_parallel))
        // 并行执行演示 (FanOut)
        
        .route("/api/langgraph/conditional", axum::routing::post(langgraph::run_langgraph_conditional))
        // 条件路由演示
        
        .route("/api/langgraph/stream", axum::routing::post(langgraph::run_langgraph_stream))
        // 流式执行演示
        
        .route("/api/langgraph/structure", axum::routing::post(langgraph::get_langgraph_structure))
        // 获取图结构（含 Mermaid 可视化语法）
        
        .route("/api/langgraph/decompose", axum::routing::post(langgraph::decompose_task))
        // AI 任务拆解（LLM 拆解 + 建图，不执行）
        
        .route("/api/langgraph/execute", axum::routing::post(langgraph::execute_sub_tasks))
        // 执行子任务（LLM 逐个执行 + token 统计）

        // ────────────────────── 文档管理 ──────────────────────
        .route("/api/documents", axum::routing::get(document::list_documents))
        // 文档列表
        
        .route("/api/documents/batch-delete", axum::routing::post(document::batch_delete_documents))
        // 批量删除文档
        
        .route("/api/documents/tags", axum::routing::post(document::add_document_tags))
        // 给文档加标签
        
        .route("/api/documents/tag/:tag", axum::routing::get(document::get_documents_by_tag))
        // 按标签查文档
        
        .route("/api/documents/:parent_id", axum::routing::post(document::delete_document))
        // 删除单个文档

        // ────────────────────── Agent 数据采集 ──────────────────────
        .route("/api/aggregate/collect", axum::routing::post(aggregate::collect))
        // 采集数据: 从多个渠道(GitHub/HN/RSS/ArXiv)抓取
        
        .route("/api/aggregate/list", axum::routing::get(aggregate::list))
        // 查看已采集的数据列表
        
        .route("/api/aggregate/search", axum::routing::post(aggregate::search))
        // 在采集的数据里搜索
        
        .route("/api/aggregate/stats", axum::routing::get(aggregate::stats))
        // 采集统计

        // ────────────────────── 监控 ──────────────────────
        .route("/api/monitor/stats", axum::routing::get(stats::get_api_stats))
        // API 调用统计

        // 应用 CORS 中间件
        .layer(cors)
        // 注入全局状态，让所有处理函数都能访问
        .with_state(state)
}
