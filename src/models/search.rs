//! 搜索相关数据模型

use serde::{Deserialize, Serialize};

/// 搜索请求
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,                     // 搜索词
    #[serde(default = "default_top_k")]
    pub top_k: usize,                      // 返回多少条
}

fn default_top_k() -> usize { 5 }

/// 搜索响应
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,                      // 搜索词
    pub mode: String,                       // 搜索模式: vector/bm25/hybrid
    pub results: Vec<SearchResultItem>,     // 搜索结果列表
    pub total_count: usize,                 // 总条数
}

/// 一条搜索结果
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResultItem {
    pub id: Option<String>,                 // 文档ID
    pub content: String,                    // 文档内容
    pub score: f32,                         // 相似度分数
    pub source: Option<String>,             // 来源
    pub metadata: serde_json::Value,        // 元数据
}

/// 对比测试请求
#[derive(Debug, Serialize, Deserialize)]
pub struct CompareRequest {
    pub query: String,                      // 搜索词
    #[serde(default = "default_top_k")]
    pub top_k: usize,                       // 返回多少条
}

/// 对比测试响应（三种检索同时执行）
#[derive(Debug, Serialize, Deserialize)]
pub struct CompareResponse {
    pub query: String,                                // 搜索词
    pub vector_results: Vec<SearchResultItem>,         // 向量检索结果
    pub bm25_results: Vec<SearchResultItem>,           // BM25 结果
    pub hybrid_results: Vec<SearchResultItem>,         // 混合检索结果
    pub comparison: SearchComparison,                   // 对比数据
}

/// 搜索对比指标
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchComparison {
    pub vector_top1_score: f32,    // 向量检索 Top1 分数
    pub bm25_top1_score: f32,      // BM25 检索 Top1 分数
    pub hybrid_top1_score: f32,    // 混合检索 Top1 分数
    pub overlap_count: usize,      // 向量和 BM25 共同返回的文档数
    pub unique_vector: usize,      // 向量独有的文档数
    pub unique_bm25: usize,        // BM25 独有的文档数
    pub unique_hybrid: usize,      // 混合检索总文档数
}
