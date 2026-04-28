//! 聚合内容数据模型

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedContent {
    pub id: String,
    pub source: String,
    pub title: String,
    pub content: String,
    pub url: String,
    pub author: Option<String>,
    pub published_at: Option<i64>,
    pub collected_at: i64,
    pub summary: Option<String>,
    pub keywords: Vec<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CollectRequest {
    pub sources: Option<Vec<String>>,
    pub force: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CollectResponse {
    pub success: bool,
    pub collected_count: usize,
    pub records: Vec<CollectRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CollectRecord {
    pub source: String,
    pub count: usize,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct AggregateListQuery {
    pub source: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct AggregateListResponse {
    pub total: usize,
    pub items: Vec<AggregatedContent>,
}

#[derive(Debug, Deserialize)]
pub struct AggregateSearchRequest {
    pub query: String,
    pub top_k: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct AggregateSearchResponse {
    pub results: Vec<AggregateSearchResult>,
}

#[derive(Debug, Serialize)]
pub struct AggregateSearchResult {
    pub id: String,
    pub source: String,
    pub title: String,
    pub content: String,
    pub url: String,
    pub score: f32,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AggregateStatsResponse {
    pub total_items: usize,
    pub by_source: HashMap<String, usize>,
    pub last_collected_at: Option<i64>,
    pub keywords_count: usize,
}

#[derive(Debug, Serialize)]
pub struct KeywordsResponse {
    pub keywords: Vec<KeywordInfo>,
}

#[derive(Debug, Serialize)]
pub struct KeywordInfo {
    pub keyword: String,
    pub count: usize,
}
