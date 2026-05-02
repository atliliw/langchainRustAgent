//! 混合检索模块 — BM25 + 向量融合（RRF 算法）
//!
//! 同时跑 BM25 和向量检索，用 RRF（Reciprocal Rank Fusion）算法融合两个排名。
//!
//! RRF 公式：score(d) = Σ 1/(k + rank_i(d))
//!   其中 k=60（平滑常数），rank_i(d)=文档 d 在检索器 i 中的排名
//!
//! 优势：不需要归一化两个检索器的分数，只看排名，公平融合

use crate::config::Config;
use crate::errors::HybridError;
use crate::stores::{QdrantStore, BM25Store};
use langchainrust::{
    Document,
    retrieval::HybridRetriever,
};
use std::sync::Arc;

/// 混合检索结果中的一条
#[derive(Debug, Clone)]
pub struct HybridSearchResult {
    pub content: String,           // 文档内容
    pub rrf_score: f32,            // RRF 融合分数
    pub bm25_score: Option<f32>,   // BM25 原始分数
    pub vector_score: Option<f32>, // 向量原始分数
    pub source: String,            // 来自哪个检索器
    pub id: Option<String>,
}

/// 混合检索器：组合 BM25 + 向量，RRF 融合
pub struct HybridStore {
    bm25_store: Arc<BM25Store>,
    vector_store: Arc<QdrantStore>,
    config: Config,
}

impl HybridStore {
    pub fn new(bm25_store: Arc<BM25Store>, vector_store: Arc<QdrantStore>, config: Config) -> Self {
        Self {
            bm25_store,
            vector_store,
            config,
        }
    }
    
    /// 混合搜索
    /// 流程：BM25检索 → 向量检索 → RRF融合 → 返回TopK
    pub async fn search(&self, query: &str, k: usize) -> Result<Vec<HybridSearchResult>, HybridError> {
        let bm25_k = self.config.search.default_top_k;
        let vector_k = self.config.search.default_top_k;
        
        // 同时跑两种检索
        let bm25_results = self.bm25_store.search(query, bm25_k)?;
        let vector_results = self.vector_store.search(query, vector_k).await?;
        
        // 把结果转成统一格式
        let bm25_docs: Vec<Document> = bm25_results.iter()
            .map(|r| Document::new(r.content.clone()).with_id(r.parent_id.clone()))
            .collect();
        
        let vector_docs: Vec<Document> = vector_results.iter()
            .map(|r| r.document.clone())
            .collect();
        
        // RRF 融合排名
        let hybrid = HybridRetriever::new();
        let fused_results = hybrid.retrieve(bm25_docs, vector_docs);
        
        // 取前 k 个
        let results: Vec<HybridSearchResult> = fused_results
            .into_iter()
            .take(k)
            .map(|r| {
                let source = match r.source {
                    langchainrust::retrieval::RetrievalSource::BM25 => "bm25",
                    langchainrust::retrieval::RetrievalSource::Vector => "vector",
                    langchainrust::retrieval::RetrievalSource::Hybrid => "hybrid",
                };
                HybridSearchResult {
                    content: r.document.content.clone(),
                    rrf_score: r.score as f32,
                    bm25_score: None,
                    vector_score: None,
                    source: source.to_string(),
                    id: r.document.id.clone(),
                }
            })
            .collect();
        
        Ok(results)
    }
    
    /// 混合添加文档（同时写入 BM25 和 向量库）
    pub async fn add_documents(&self, documents: Vec<Document>) -> Result<Vec<String>, HybridError> {
        self.bm25_store.add_documents(documents.clone())?;
        let ids = self.vector_store.add_documents(documents).await?;
        Ok(ids)
    }
    
    /// 清空所有数据
    pub async fn clear(&self) -> Result<(), HybridError> {
        self.bm25_store.clear()?;
        self.vector_store.clear().await?;
        Ok(())
    }
}
