//! 文档管理相关数据模型

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// BM25 检索结果中的一条
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BM25SearchResult {
    pub id: String,                // 文档ID
    pub content: String,           // 文档内容
    pub score: f32,                // BM25 分数
    pub parent_id: String,         // 父文档ID
    pub is_merged: bool,           // 是否来自自动合并
}

/// 删除文档响应
#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteDocumentResponse {
    pub success: bool,
    pub parent_id: String,
    pub bm25_chunks_deleted: bool,
    pub vector_count_deleted: usize,
    pub message: String,
}

/// 删除文档请求
#[derive(Deserialize)]
pub struct DeleteDocumentRequest {
    pub filename: String,  // 要删除的文件名
}

/// 文档信息（展示用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentInfo {
    pub id: String,
    pub title: String,
    pub content_preview: String,
    pub chunk_count: usize,
    pub metadata: HashMap<String, String>,
}

/// 批量删除请求
#[derive(Debug, Serialize, Deserialize)]
pub struct BatchDeleteRequest {
    pub parent_ids: Vec<String>,
}

/// 批量删除响应
#[derive(Debug, Serialize, Deserialize)]
pub struct BatchDeleteResponse {
    pub success: bool,
    pub deleted_count: usize,
    pub failed_count: usize,
    pub message: String,
}

/// 文档标签请求
#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentTagRequest {
    pub parent_id: String,
    pub tags: Vec<String>,
}

/// 文档标签信息
#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentTagInfo {
    pub parent_id: String,
    pub tags: Vec<String>,
}
