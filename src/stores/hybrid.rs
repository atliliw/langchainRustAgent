//! 混合检索模块 - BM25 + 向量融合（RRF 算法）

use crate::config::Config;
use crate::errors::HybridError;
use crate::stores::{QdrantStore, BM25Store};
use langchainrust::{
    Document,
    retrieval::{HybridRetriever, RetrievedDocument},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct HybridSearchResult {
    pub content: String,
    pub rrf_score: f32,
    pub bm25_score: Option<f32>,
    pub vector_score: Option<f32>,
    pub source: String,
    pub id: Option<String>,
}

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
    
    pub async fn search(&self, query: &str, k: usize) -> Result<Vec<HybridSearchResult>, HybridError> {
        let bm25_k = self.config.search.default_top_k;
        let vector_k = self.config.search.default_top_k;
        
        let bm25_results = self.bm25_store.search(query, bm25_k)?;
        
        let bm25_docs: Vec<Document> = bm25_results.iter()
            .map(|r| Document::new(r.content.clone()).with_id(r.parent_id.clone()))
            .collect();
        
        let vector_results = self.vector_store.search(query, vector_k).await?;
        
        let vector_docs: Vec<Document> = vector_results.iter()
            .map(|r| r.document.clone())
            .collect();
        
        let hybrid = HybridRetriever::new();
        let fused_results = hybrid.retrieve(bm25_docs, vector_docs);
        
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
    
    pub async fn add_documents(&self, documents: Vec<Document>) -> Result<Vec<String>, HybridError> {
        self.bm25_store.add_documents(documents.clone())?;
        
        let ids = self.vector_store.add_documents(documents).await?;
        
        Ok(ids)
    }
    
    pub async fn clear(&self) -> Result<(), HybridError> {
        self.bm25_store.clear()?;
        self.vector_store.clear().await?;
        Ok(())
    }
}