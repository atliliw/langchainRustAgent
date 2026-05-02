use serde::{Deserialize, Serialize};

/// 统计信息响应
#[derive(Debug, Serialize, Deserialize)]
pub struct StatsResponse {
    pub total_documents: usize,       // 向量库中的文档总数
    pub vector_size: usize,           // 向量维度 (1536)
    pub bm25_chunks: usize,           // BM25 索引中的chunk数
    pub bm25_persisted: bool,         // BM25 是否用 MongoDB 持久化
    pub collection_name: String,      // Qdrant 集合名
    pub conversation_sessions: usize, // 对话会话数
}
