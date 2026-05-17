//! 文件上传处理函数
//! 
//! POST /api/upload (multipart/form-data)
//!
//! 流程：前端选择文件 → 上传到临时目录 → 文档处理(加载+分块+Embedding) 
//!       → 存入Qdrant(向量) + MongoDB(BM25) → 删除临时文件 → 返回结果
//!
//! 支持的文件类型：txt, pdf, md, json, csv （在 config.toml 中配置）

use crate::config::Config;
use crate::handlers::ApiErrorResponse;
use crate::models::{ChunkStrategy, UploadResponse};
use crate::services::mcp::mcp_bridge::McpBridge;
use crate::services::mcp::mcp_server::McpServerService;
use crate::services::ApiService;
use axum::{
    extract::{Multipart, State},
    Json,
};
use std::path::PathBuf;
use std::sync::Arc;

pub struct AppState {
    pub api: Arc<ApiService>,
    pub config: Config,
    pub mcp_bridge: Arc<McpBridge>,
    pub mcp_server: McpServerService,
}

/// 处理文件上传
/// 从前端收到文件 → 处理后双写到 Qdrant + MongoDB
pub async fn upload_file(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiErrorResponse> {
    let mut file_data: Option<(Vec<u8>, String)> = None;
    let mut strategy = ChunkStrategy::default();

    // 先遍历 multipart 读所有字段
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap_or("").to_string();
        
        match name.as_str() {
            "file" => {
                let file_name = field.file_name().unwrap_or("unknown").to_string();
                let data = field.bytes().await.unwrap_or_default();
                file_data = Some((data.to_vec(), file_name));
            }
            "chunk_strategy" => {
                if let Ok(val) = field.text().await {
                    strategy = ChunkStrategy::from_str(&val);
                }
            }
            _ => {}
        }
    }

    if let Some((data, file_name)) = file_data {
        let upload_dir = PathBuf::from(&state.config.server.upload_dir);
        if !upload_dir.exists() {
            std::fs::create_dir_all(&upload_dir).ok();
        }
        
        let unique_name = format!("{}_{}", uuid::Uuid::new_v4(), file_name);
        let file_path = upload_dir.join(&unique_name);
        
        std::fs::write(&file_path, &data).ok();
        
        let response = state.api.upload_file_with_strategy(&file_path, &file_name, strategy).await?;
        
        std::fs::remove_file(&file_path).ok();
        
        return Ok(Json(response));
    }
    
    Err(ApiErrorResponse(
        axum::http::StatusCode::BAD_REQUEST,
        "未找到上传文件".to_string(),
    ))
}
