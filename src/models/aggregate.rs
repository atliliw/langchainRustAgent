//! 数据采集 Agent 相关数据模型
//!
//! 从多个数据源采集到统一格式后存储

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 一条采集到的内容
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedContent {
    pub id: String,
    pub source: String,               // github/hackernews/rss/arxiv
    pub title: String,
    pub content: String,
    pub url: String,
    pub author: Option<String>,
    pub published_at: Option<i64>,
    pub collected_at: i64,
    pub summary: Option<String>,       // LLM 生成的摘要
    pub keywords: Vec<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// 采集请求
#[derive(Debug, Serialize, Deserialize)]
pub struct CollectRequest {
    pub sources: Option<Vec<String>>,  // 要采集的渠道
    pub force: Option<bool>,           // 是否强制重新采集
}

/// 采集响应
#[derive(Debug, Serialize, Deserialize)]
pub struct CollectResponse {
    pub success: bool,
    pub collected_count: usize,
    pub records: Vec<CollectRecord>,
}

/// 单个渠道的采集记录
#[derive(Debug, Serialize, Deserialize)]
pub struct CollectRecord {
    pub source: String,
    pub count: usize,
    pub status: String,
}

/// 列表查询参数
#[derive(Debug, Deserialize)]
pub struct AggregateListQuery {
    pub source: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// 列表响应
#[derive(Debug, Serialize)]
pub struct AggregateListResponse {
    pub total: usize,
    pub items: Vec<AggregatedContent>,
}

/// 采集内容的搜索请求
#[derive(Debug, Deserialize)]
pub struct AggregateSearchRequest {
    pub query: String,
    pub top_k: Option<usize>,
}

/// 搜索响应
#[derive(Debug, Serialize)]
pub struct AggregateSearchResponse {
    pub results: Vec<AggregateSearchResult>,
}

/// 一条搜索结果
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

/// 采集统计响应
#[derive(Debug, Serialize)]
pub struct AggregateStatsResponse {
    pub total_items: usize,
    pub by_source: HashMap<String, usize>,
    pub last_collected_at: Option<i64>,
    pub keywords_count: usize,
}

/// 关键词统计
#[derive(Debug, Serialize)]
pub struct KeywordsResponse {
    pub keywords: Vec<KeywordInfo>,
}

/// 关键词信息
#[derive(Debug, Serialize)]
pub struct KeywordInfo {
    pub keyword: String,
    pub count: usize,
}
