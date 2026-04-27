use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    5
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub mode: String,
    pub results: Vec<SearchResultItem>,
    pub total_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResultItem {
    pub id: Option<String>,
    pub content: String,
    pub score: f32,
    pub source: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompareRequest {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompareResponse {
    pub query: String,
    pub vector_results: Vec<SearchResultItem>,
    pub bm25_results: Vec<SearchResultItem>,
    pub hybrid_results: Vec<SearchResultItem>,
    pub comparison: SearchComparison,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchComparison {
    pub vector_top1_score: f32,
    pub bm25_top1_score: f32,
    pub hybrid_top1_score: f32,
    pub overlap_count: usize,
    pub unique_vector: usize,
    pub unique_bm25: usize,
    pub unique_hybrid: usize,
}
