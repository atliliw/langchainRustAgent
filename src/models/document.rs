use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
