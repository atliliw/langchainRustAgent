use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BM25SearchResult {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub parent_id: String,
    pub is_merged: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteDocumentResponse {
    pub success: bool,
    pub parent_id: String,
    pub bm25_chunks_deleted: bool,
    pub vector_count_deleted: usize,
    pub message: String,
}

#[derive(Deserialize)]
pub struct DeleteDocumentRequest {
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentInfo {
    pub id: String,
    pub title: String,
    pub content_preview: String,
    pub chunk_count: usize,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchDeleteRequest {
    pub parent_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchDeleteResponse {
    pub success: bool,
    pub deleted_count: usize,
    pub failed_count: usize,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentTagRequest {
    pub parent_id: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentTagInfo {
    pub parent_id: String,
    pub tags: Vec<String>,
}
