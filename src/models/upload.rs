use serde::{Deserialize, Serialize};

/// 上传响应
#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResponse {
    pub success: bool,
    pub document_count: usize,    // 原始文档数
    pub chunk_count: usize,       // 分块后的总chunk数
    pub message: String,          // 提示消息
    pub document_ids: Vec<String>, // 向量库中的文档ID
}
