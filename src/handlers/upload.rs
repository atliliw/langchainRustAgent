//! 文件上传处理函数

use crate::config::Config;
use crate::errors::ApiError;
use crate::handlers::ApiErrorResponse;
use crate::models::UploadResponse;
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
}

pub async fn upload_file(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiErrorResponse> {
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().unwrap_or("unknown").to_string();
        
        if name == "file" {
            let data = field.bytes().await.unwrap_or_default();
            
            let upload_dir = PathBuf::from(&state.config.server.upload_dir);
            if !upload_dir.exists() {
                std::fs::create_dir_all(&upload_dir).ok();
            }
            
            let unique_name = format!("{}_{}", 
                uuid::Uuid::new_v4(),
                file_name
            );
            let file_path = upload_dir.join(&unique_name);
            
            std::fs::write(&file_path, &data).ok();
            
            let response = state.api.upload_file(&file_path, &file_name).await?;
            
            std::fs::remove_file(&file_path).ok();
            
            return Ok(Json(response));
        }
    }
    
    Err(ApiErrorResponse(
        axum::http::StatusCode::BAD_REQUEST,
        "未找到上传文件".to_string(),
    ))
}