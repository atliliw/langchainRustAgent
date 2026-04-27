use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResponse {
    pub success: bool,
    pub document_count: usize,
    pub chunk_count: usize,
    pub message: String,
    pub document_ids: Vec<String>,
}
