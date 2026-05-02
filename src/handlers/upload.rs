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
use crate::models::UploadResponse;
use crate::services::ApiService;
use axum::{
    extract::{Multipart, State},
    Json,
};
use std::path::PathBuf;
use std::sync::Arc;

/// 全局状态：存放 API 服务和配置
/// 在 main.rs 创建，通过 Axum 的 State 注入到每个处理函数
pub struct AppState {
    pub api: Arc<ApiService>,  // API 业务服务（核心逻辑）
    pub config: Config,         // 配置
}

/// 处理文件上传
/// 从前端收到文件 → 处理后双写到 Qdrant + MongoDB
pub async fn upload_file(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiErrorResponse> {
    // 遍历 multipart 中的每个字段
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().unwrap_or("unknown").to_string();
        
        // 只处理 name="file" 的字段（前端表单中的文件字段名）
        if name == "file" {
            // 读取上传的文件数据
            let data = field.bytes().await.unwrap_or_default();
            
            // 确保 uploads/ 目录存在
            let upload_dir = PathBuf::from(&state.config.server.upload_dir);
            if !upload_dir.exists() {
                std::fs::create_dir_all(&upload_dir).ok();
            }
            
            // 生成唯一文件名：UUID_原文件名，避免冲突
            let unique_name = format!("{}_{}", 
                uuid::Uuid::new_v4(),
                file_name
            );
            let file_path = upload_dir.join(&unique_name);
            
            // 把文件写入临时目录
            std::fs::write(&file_path, &data).ok();
            
            // 核心：处理文件（加载→分块→向量化→双写）
            let response = state.api.upload_file(&file_path, &file_name).await?;
            
            // 处理完后删除临时文件
            std::fs::remove_file(&file_path).ok();
            
            return Ok(Json(response));
        }
    }
    
    // 如果请求体中没有找到 name="file" 的字段
    Err(ApiErrorResponse(
        axum::http::StatusCode::BAD_REQUEST,
        "未找到上传文件".to_string(),
    ))
}
