//! LangChainRust Agent - Web 服务器入口

use langchainrust_agent::{
    config::Config,
    routes::create_router,
    handlers::AppState,
    services::ApiService,
};

use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    let config = Config::load().expect("配置加载失败，请检查 config.toml 文件");
    
    tracing::info!("启动服务: {}", config.server_addr());
    tracing::info!("Qdrant URL: {}", config.qdrant.url);
    tracing::info!("Collection: {}", config.qdrant.collection_name);
    
    let api = Arc::new(ApiService::new(config.clone()).await
        .expect("API 服务初始化失败"));
    
    let state = Arc::new(AppState {
        api,
        config: config.clone(),
    });
    
    let app = create_router(state);
    
    let addr: std::net::SocketAddr = config.server_addr().parse()
        .expect("地址解析失败");
    
    tracing::info!("服务运行在 http://{}", addr);
    tracing::info!("打开浏览器访问 http://{} 即可使用", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    
    axum::serve(listener, app).await.unwrap();
}