//! ============================================================================
//! 主入口文件 - Web 服务器和路由配置
//! ============================================================================
//!
//! 功能说明：
//! 1. 启动 HTTP 服务器（使用 Axum 框架）
//! 2. 配置 API 路由（上传、搜索、统计等）
//! 3. 提供静态文件服务（前端页面）
//! 4. 连接 Qdrant 向量数据库
//!
//! 技术栈：
//! - Axum: Rust Web 框架（类似 Express.js）
//! - Tower: 中间件库（处理 CORS、静态文件等）
//! - Tokio: 异步运行时（类似 Node.js 的事件循环）

// ============================================================================
// 导入依赖
// ============================================================================

// 导入我们自己写的模块
use langchainrust_demo::api::{
    ApiService, SearchRequest, SearchResponse, UploadResponse, StatsResponse, CompareResponse, DeleteDocumentResponse
};
use langchainrust_demo::config::Config;
use langchainrust_demo::search_test::{SearchTester, TestCase, PrecisionReport};
use langchainrust_demo::conversation_store::{ChatRequest, ChatResponse, SessionInfo};

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router, Json,
    extract::{Multipart, State, Query, Path},
    http::StatusCode,
    response::{IntoResponse, Response, sse::{Event, Sse}},
};
use futures_util::stream::StreamExt as FuturesStreamExt;

use tower_http::{
    services::ServeDir,
    cors::{CorsLayer, Any},
};

use serde::Deserialize;

// ============================================================================
// 应用状态结构体
// ============================================================================

/// 应用状态 - 在所有请求处理函数之间共享的数据
///
/// 使用 Arc 包装，可以在多个线程间安全共享
struct AppState {
    api: Arc<ApiService>,   // API 服务实例（处理上传、搜索等业务逻辑）
    config: Config,         // 配置信息（从 config.toml 加载）
}

// ============================================================================
// 主函数 - 程序入口
// ============================================================================

#[tokio::main]  // 使用 Tokio 异步运行时（必须标注）
async fn main() {
    // 初始化日志系统
    // 日志级别由环境变量 RUST_LOG 控制，默认 info
    tracing_subscriber::fmt::init();
    
    // 加载配置文件 config.toml
    // 配置包括：服务器地址、Qdrant URL、OpenAI API Key 等
    let config = Config::load().expect("配置加载失败，请检查 config.toml 文件");
    
    // 打印启动信息
    tracing::info!("启动服务: {}", config.server_addr());
    tracing::info!("Qdrant URL: {}", config.qdrant.url);
    tracing::info!("Collection: {}", config.qdrant.collection_name);
    
    // 创建 API 服务实例
    // ApiService 负责：文件处理、向量生成、Qdrant 存储等
    let api = Arc::new(ApiService::new(config.clone()).await
        .expect("API 服务初始化失败"));
    
    // 创建应用状态
    // 这个状态会被所有请求处理函数共享访问
    let state = Arc::new(AppState {
        api,
        config: config.clone(),
    });
    
    // 配置 CORS（跨域资源共享）
    // 允许任何来源访问，方便前端开发和部署
    let cors = CorsLayer::new()
        .allow_origin(Any)    // 允许任何域名
        .allow_methods(Any)   // 允许任何 HTTP 方法
        .allow_headers(Any);  // 允许任何请求头
    
    // 创建路由器
    // 这是核心配置：定义 URL 路径和对应的处理函数
    let app = Router::new()
        .route("/api/upload", post(upload_file))
        .route("/api/search/vector", post(search_vector))
        .route("/api/search/bm25", post(search_bm25))
        .route("/api/search/hybrid", post(search_hybrid))
        .route("/api/search/compare", post(compare_search))
        .route("/api/stats", get(get_stats))
        .route("/api/clear", post(clear_all))
        .route("/api/test/precision", post(run_precision_test))
        .route("/api/test/cases", get(get_test_cases))
        .route("/api/chat", post(chat))
        .route("/api/chat/stream", post(chat_stream))
        .route("/api/chat/history/:session_id", get(get_chat_history))
        .route("/api/chat/sessions", get(get_sessions))
        .route("/api/chat/clear/:session_id", post(clear_session))
        .route("/api/chat/compress-modes", get(get_compress_modes))
        .route("/api/langgraph/info", get(get_langgraph_info))
        .route("/api/langgraph/parallel", post(run_langgraph_parallel))
        .route("/api/langgraph/conditional", post(run_langgraph_conditional))
        .route("/api/langgraph/stream", post(run_langgraph_stream))
        .route("/api/documents", get(list_documents))
        .route("/api/documents/:parent_id", post(delete_document))
        .fallback_service(ServeDir::new("frontend"))
        .layer(cors)
        .with_state(state);
    
    // 解析服务器地址
    // 例如: "0.0.0.0:8080" 表示监听所有网络接口的 8080 端口
    let addr: SocketAddr = config.server_addr().parse()
        .expect("地址解析失败");
    
    // 打印访问地址
    tracing::info!("服务运行在 http://{}", addr);
    tracing::info!("打开浏览器访问 http://{} 即可使用", addr);
    
    // 绑定 TCP 监听端口
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    
    // 启动 HTTP 服务器
    // axum::serve 会持续运行，处理所有 incoming 请求
    axum::serve(listener, app).await.unwrap();
}

// ============================================================================
// API 处理函数 - 文件上传
// ============================================================================

/// 处理文件上传请求
///
/// 流程：
/// 1. 接收 multipart/form-data 格式的文件
/// 2. 保存到临时目录
/// 3. 调用 API 服务处理文件（分割、向量化、存储）
/// 4. 返回处理结果
///
/// @param state - 应用状态（包含 ApiService）
/// @param multipart - multipart 表单数据（包含上传的文件）
/// @return 上传结果（成功/失败、文档数量等）
async fn upload_file(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiErrorResponse> {
    // 遍历 multipart 表单中的每个字段
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        // 获取字段名称和文件名
        let name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().unwrap_or("unknown").to_string();
        
        // 只处理名为 "file" 的字段（前端上传表单的字段名）
        if name == "file" {
            // 读取文件内容到内存
            let data = field.bytes().await.unwrap_or_default();
            
            // 创建上传目录（如果不存在）
            let upload_dir = PathBuf::from(&state.config.server.upload_dir);
            if !upload_dir.exists() {
                std::fs::create_dir_all(&upload_dir).ok();
            }
            
            // 生成唯一的文件名（UUID + 原文件名）
            // 防止多个用户上传同名文件时冲突
            let unique_name = format!("{}_{}", 
                uuid::Uuid::new_v4(),
                file_name
            );
            let file_path = upload_dir.join(&unique_name);
            
            // 写入临时文件
            std::fs::write(&file_path, &data).ok();
            
            // 调用 API 服务处理文件
            // 处理包括：文本分割、向量生成、存入 Qdrant
            let response = state.api.upload_file(&file_path, &file_name).await?;
            
            // 处理完成后删除临时文件（节省磁盘空间）
            std::fs::remove_file(&file_path).ok();
            
            // 返回处理结果
            return Ok(Json(response));
        }
    }
    
    // 如果没有找到 "file" 字段，返回错误
    Err(ApiErrorResponse(
        StatusCode::BAD_REQUEST,
        "未找到上传文件".to_string(),
    ))
}

// ============================================================================
// API 处理函数 - 向量搜索
// ============================================================================

/// 处理向量搜索请求
///
/// 流程：
/// 1. 接收搜索请求（查询文本 + 返回数量）
/// 2. 调用 API 服务进行向量搜索
/// 3. 返回搜索结果（匹配的文档 + 相似度分数）
///
/// @param state - 应用状态
/// @param request - 搜索请求 JSON（query, top_k）
/// @return 搜索结果（匹配的文档列表）
#[derive(Deserialize)]
struct CompareRequest {
    query: String,
    #[serde(default = "default_top_k")]
    top_k: usize,
}

fn default_top_k() -> usize { 5 }

async fn search_vector(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiErrorResponse> {
    let response = state.api.search_vector(request).await?;
    Ok(Json(response))
}

async fn search_bm25(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiErrorResponse> {
    let response = state.api.search_bm25(request)?;
    Ok(Json(response))
}

async fn search_hybrid(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiErrorResponse> {
    let response = state.api.search_hybrid(request).await?;
    Ok(Json(response))
}

async fn compare_search(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CompareRequest>,
) -> Result<Json<CompareResponse>, ApiErrorResponse> {
    let response = state.api.compare_search(request.query, request.top_k).await?;
    Ok(Json(response))
}

// ============================================================================
// API 处理函数 - 统计信息
// ============================================================================

/// 获取系统统计信息
///
/// 返回：
/// - total_documents: 文档总数
/// - vector_size: 向量维度
/// - collection_name: Qdrant 集合名称
async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatsResponse>, ApiErrorResponse> {
    let response = state.api.get_stats().await?;
    Ok(Json(response))
}

// ============================================================================
// API 处理函数 - 清空数据
// ============================================================================

/// 清空所有文档数据
///
/// 用途：重新测试时清空已有数据
async fn clear_all(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    state.api.clear_all().await?;
    
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "所有文档已清空"
    })))
}

// ============================================================================
// API 处理函数 - 精准度测试
// ============================================================================

/// 精准度测试请求参数
#[derive(Deserialize)]
struct PrecisionTestQuery {
    #[serde(default)]
    custom_cases: bool,  // 是否使用自定义测试用例
}

/// 运行搜索精准度测试
///
/// 功能：
/// 1. 插入预设的测试文档
/// 2. 执行搜索查询
/// 3. 计算搜索排名和精准度分数
///
/// @param state - 应用状态
/// @param query - URL 查询参数（custom_cases=true 时使用自定义用例）
/// @param custom_cases_opt - 自定义测试用例（可选）
/// @return 测试报告（通过数、精准度分数、详细结果）
async fn run_precision_test(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PrecisionTestQuery>,
    custom_cases_opt: Option<Json<Vec<TestCase>>>,
) -> Result<Json<PrecisionReport>, ApiErrorResponse> {
    // 选择测试用例
    let test_cases = if query.custom_cases {
        // 使用自定义用例（如果有）
        custom_cases_opt.map(|j| j.0).unwrap_or_default()
    } else {
        // 使用默认预设用例
        SearchTester::get_default_test_cases()
    };
    
    // 创建测试器并运行测试
    let tester = SearchTester::new(state.api.vector_store.clone(), state.config.clone());
    let report = tester.run_precision_test(test_cases).await?;
    
    // 返回测试报告
    Ok(Json(report))
}

/// 获取默认测试用例
///
/// 用途：前端展示测试用例内容
async fn get_test_cases() -> Json<Vec<TestCase>> {
    Json(SearchTester::get_default_test_cases())
}

// ============================================================================
// API 处理函数 - 对话功能
// ============================================================================

async fn chat(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ApiErrorResponse> {
    let response = state.api.chat(request).await?;
    Ok(Json(response))
}

async fn chat_stream(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiErrorResponse> {
    use langchainrust_demo::conversation_store::SearchMode;
    
    let search_mode = SearchMode::from_flags(request.use_vector, request.use_bm25);
    
    let rag_sources = match search_mode {
        SearchMode::None => Vec::new(),
        SearchMode::Vector => {
            let results = state.api.vector_store.search(&request.message, request.top_k).await
                .map_err(|e| ApiErrorResponse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            results.into_iter().map(|r| langchainrust_demo::conversation_store::SourceInfo {
                content: r.document.content.clone(),
                score: r.score,
                source: "vector".to_string(),
            }).collect()
        },
        SearchMode::BM25 => {
            let results = state.api.bm25_store.search(&request.message, request.top_k).map_err(|e| ApiErrorResponse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            results.into_iter().map(|r| langchainrust_demo::conversation_store::SourceInfo {
                content: r.content.clone(),
                score: r.score,
                source: "bm25".to_string(),
            }).collect()
        },
        SearchMode::Hybrid => {
            let results = state.api.hybrid_store.search(&request.message, request.top_k).await
                .map_err(|e| ApiErrorResponse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            results.into_iter().map(|r| langchainrust_demo::conversation_store::SourceInfo {
                content: r.content.clone(),
                score: r.rrf_score,
                source: r.source.clone(),
            }).collect()
        },
    };
    
    let (session_id, mut token_stream) = state.api.conversation_store.chat_stream(request.clone(), rag_sources).await
        .map_err(|e| ApiErrorResponse(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    eprintln!("=== main.rs: session_id from chat_stream='{}' ===", session_id);
    let session_id_clean = session_id.trim().to_string();
    eprintln!("=== main.rs: session_id_clean='{}' ===", session_id_clean);
    
    let user_msg = request.message.clone();
    let store = state.api.conversation_store.clone();
    
    let stream = async_stream::stream! {
        let mut full_reply = String::new();
        let sid = session_id_clean.clone();
        
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
                    yield Ok(Event::default().event("error").data(e.to_string()));
                    break;
                }
            }
        }
        
        store.save_full_message(&sid, &user_msg, &full_reply).await.ok();
        
        yield Ok(Event::default().event("done").data("[DONE]"));
    };
    
    Ok(Sse::new(stream))
}

async fn get_chat_history(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<langchainrust_demo::conversation_store::ConversationMessage>>, ApiErrorResponse> {
    let history = state.api.get_conversation_history(&session_id).await?;
    Ok(Json(history))
}

async fn get_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SessionInfo>>, ApiErrorResponse> {
    let sessions = state.api.get_sessions().await?;
    Ok(Json(sessions))
}

async fn get_compress_modes() -> Json<Vec<langchainrust_demo::conversation_store::CompressModeInfo>> {
    Json(langchainrust_demo::conversation_store::ConversationStore::get_compress_modes())
}

async fn clear_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    state.api.clear_session(&session_id).await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("会话 {} 已清空", session_id)
    })))
}

async fn get_langgraph_info() -> Json<serde_json::Value> {
    Json(langchainrust_demo::api::ApiService::get_langgraph_info())
}

#[derive(Deserialize)]
struct LangGraphRequest {
    input: String,
}

async fn run_langgraph_parallel(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<langchainrust_demo::langgraph_demo::ParallelDemoResult>, ApiErrorResponse> {
    let result = langchainrust_demo::api::ApiService::run_langgraph_parallel(request.input).await?;
    Ok(Json(result))
}

async fn run_langgraph_conditional(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<langchainrust_demo::langgraph_demo::ConditionalDemoResult>, ApiErrorResponse> {
    let result = langchainrust_demo::api::ApiService::run_langgraph_conditional(request.input).await?;
    Ok(Json(result))
}

async fn run_langgraph_stream(
    Json(request): Json<LangGraphRequest>,
) -> Result<Json<Vec<langchainrust_demo::langgraph_demo::StreamDemoEvent>>, ApiErrorResponse> {
    let result = langchainrust_demo::api::ApiService::run_langgraph_stream(request.input).await?;
    Ok(Json(result))
}

async fn list_documents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<langchainrust_demo::bm25_store::DocumentInfo>>, ApiErrorResponse> {
    let documents = state.api.list_documents().await?;
    Ok(Json(documents))
}

#[derive(Deserialize)]
struct DeleteDocumentRequest {
    filename: String,
}

async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path(parent_id): Path<String>,
    Json(request): Json<DeleteDocumentRequest>,
) -> Result<Json<DeleteDocumentResponse>, ApiErrorResponse> {
    let result = state.api.delete_document(&parent_id, &request.filename).await?;
    Ok(Json(result))
}

// ============================================================================
// 错误处理
// ============================================================================

/// 自定义 API 错误响应
///
/// 包含：
/// - HTTP 状态码（如 400, 500）
/// - 错误消息
struct ApiErrorResponse(StatusCode, String);

/// 实现响应转换
///
/// 将 ApiErrorResponse 转换为 HTTP 响应
/// 格式：{"success": false, "error": "错误消息"}
impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({
            "success": false,
            "error": self.1
        }))).into_response()
    }
}

/// 自动转换错误类型
///
/// 任何实现了 Display trait 的错误都可以自动转换为 ApiErrorResponse
/// 这样可以在处理函数中使用 ? 运算符自动处理错误
impl<E: std::fmt::Display> From<E> for ApiErrorResponse {
    fn from(err: E) -> Self {
        ApiErrorResponse(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    }
}