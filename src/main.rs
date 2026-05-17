//! LangChainRust Agent - Web 服务器入口
//! 这是整个程序的起点。启动时做的事情：
//! 1. 加载 config.toml 配置
//! 2. 连接 Qdrant/MongoDB/SQLite
//! 3. 启动 Axum Web 服务，监听 8090 端口

// 引入项目中自己写的模块
use langchainrust_agent::{
    config::Config,          // 配置管理（读取 config.toml）
    routes::create_router,   // 创建所有 API 路由
    handlers::{AppState, playground},  // 全局状态 + v2 handler
    services::ApiService,    // API 业务服务（核心逻辑）
};

use std::sync::Arc;

#[tokio::main]
async fn main() {
    // 初始化日志系统（之后用 tracing::info! 打印日志）
    tracing_subscriber::fmt::init();
    
    // 从 config.toml 读取配置，没有就报错
    let config = Config::load().expect("配置加载失败，请检查 config.toml 文件");
    
    // 打印启动信息到日志
    tracing::info!("启动服务: {}", config.server_addr());
    tracing::info!("Qdrant URL: {}", config.qdrant.url);
    tracing::info!("Collection: {}", config.qdrant.collection_name);
    
    // 创建 API 服务：连接数据库 + 实例化各个模块
    let api = ApiService::new(config.clone()).await
        .expect("API 服务初始化失败");

    // 恢复未完成的 Agent session
    let pool = api.conversation_store.pool();
    let recovered = langchainrust_agent::services::agent_executor::recover_sessions(&pool).await;
    if !recovered.is_empty() {
        tracing::info!("恢复 {} 个未完成的 Agent session", recovered.len());
        for r in &recovered {
            tracing::info!("  session={}, task={}, done={}",
                r["session_id"].as_str().unwrap_or(""),
                r["task"].as_str().unwrap_or(""),
                r["completed_tasks"].as_i64().unwrap_or(0));
        }
    }

    let api = Arc::new(api);
    let mcp_bridge = Arc::new(langchainrust_agent::services::mcp::mcp_bridge::McpBridge::new());
    let mcp_server = langchainrust_agent::services::mcp::mcp_server::McpServerService::new(
        config.clone(),
        Some(api.vector_store.clone()),
    );
    
    let state = Arc::new(AppState {
        api,
        config: config.clone(),
        mcp_bridge,
        mcp_server,
    });
    
    // v2 API 路由（所有新功能），先绑定状态再合并
    use axum::Router;
    let v2_router = Router::new()
        .route("/api/v2/chat", axum::routing::post(playground::v2_chat))
        .route("/api/v2/tools", axum::routing::get(playground::v2_tools))
        .route("/api/v2/stats", axum::routing::get(playground::v2_stats))
        .route("/api/v2/cost/record", axum::routing::post(playground::v2_record_cost))
        .route("/api/v2/mcp/connect", axum::routing::post(playground::mcp_connect))
        .route("/api/v2/mcp/tools", axum::routing::post(playground::mcp_list_tools))
        .route("/api/v2/mcp/call", axum::routing::post(playground::mcp_call_tool))
        .route("/api/v2/mcp/server", axum::routing::post(playground::mcp_server_handler))
        .route("/api/v2/evaluate/run", axum::routing::post(playground::evaluate_run))
        .route("/api/v2/evaluate/compare", axum::routing::post(playground::evaluate_compare))
        .route("/api/v2/vision", axum::routing::post(playground::vision_analyze))
        .with_state(state.clone());
    // 合并 v2 路由到主路由器
    let app = create_router(state.clone()).merge(v2_router);
    
    // ===== 主服务 (Web + API) =====
    let addr: std::net::SocketAddr = config.server_addr().parse()
        .expect("地址解析失败");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("Web 服务运行在 http://{}", addr);
    
    // ===== MCP 服务 (标准 MCP 协议，供外部 AI Agent 连接) =====
    let mcp_addr: std::net::SocketAddr = format!("{}:{}", config.server.host, config.server.mcp_port)
        .parse().expect("MCP 地址解析失败");
    let mcp_listener = tokio::net::TcpListener::bind(mcp_addr).await.unwrap();
    
    // MCP 端口只暴露 /api/v2/mcp/server 端点，按标准 JSON-RPC 2.0 响应
    let mcp_router = axum::Router::new()
        .route("/", axum::routing::post(playground::mcp_server_handler))
        .with_state(state.clone());
    tracing::info!("MCP 服务运行在 http://{}（标准 MCP 协议）", mcp_addr);
    
    // 启动两个服务，任一退出则整体退出
    tokio::select! {
        _ = axum::serve(listener, app) => {},
        _ = axum::serve(mcp_listener, mcp_router) => {},
    }
}