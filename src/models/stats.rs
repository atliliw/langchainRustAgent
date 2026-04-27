use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct StatsResponse {
    pub total_documents: usize,
    pub vector_size: usize,
    pub bm25_chunks: usize,
    pub bm25_persisted: bool,
    pub collection_name: String,
    pub conversation_sessions: usize,
}
