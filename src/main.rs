//! LangChainRust Agent - Web 服务器入口
//! 这是整个程序的起点。启动时做的事情：
//! 1. 加载 config.toml 配置
//! 2. 连接 Qdrant/MongoDB/SQLite
//! 3. 启动 Axum Web 服务，监听 8090 端口

// 引入项目中自己写的模块
use langchainrust_agent::{
    config::Config,          // 配置管理（读取 config.toml）
    routes::create_router,   // 创建所有 API 路由
    handlers::AppState,      // 全局状态，存放 API 服务和配置
    services::ApiService,    // API 业务服务（核心逻辑）
};

// Arc = 原子引用计数，让多个地方安全共享同一个数据
use std::sync::Arc;

// #[tokio::main] 把 main 函数变成异步入口
// tokio 是 Rust 的异步运行时，类似 Python 的 asyncio
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
    // Arc::new 包装成线程安全共享引用
    let api = Arc::new(ApiService::new(config.clone()).await
        .expect("API 服务初始化失败"));
    
    // 全局状态 = API 服务 + 配置，传给所有路由处理函数
    let state = Arc::new(AppState {
        api,
        config: config.clone(),
    });
    
    // 创建路由器（注册所有 API 路由）
    let app = create_router(state);
    
    // 解析地址字符串为实际地址
    let addr: std::net::SocketAddr = config.server_addr().parse()
        .expect("地址解析失败");
    
    tracing::info!("服务运行在 http://{}", addr);
    tracing::info!("打开浏览器访问 http://{} 即可使用", addr);
    
    // 创建 TCP 监听器，绑定到指定端口
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    
    // 使用 Axum 框架启动 HTTP 服务器
    // axum::serve 会把收到的每个 HTTP 请求交给 create_router 注册的路由处理
    axum::serve(listener, app).await.unwrap();
}