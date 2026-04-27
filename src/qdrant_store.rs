//! ============================================================================
//! Qdrant 向量存储模块 - 向量数据库交互层
//! ============================================================================
//!
//! 功能说明：
//! 1. 连接 Qdrant 向量数据库
//! 2. 生成文档向量嵌入（使用 OpenAI Embeddings API）
//! 3. 存储文档和向量
//! 4. 执行向量相似度搜索
//!
//! 技术要点：
//! - OpenAI Embeddings: 将文本转换为向量（1536 维）
//! - Qdrant: 存储向量并执行相似度搜索
//! - 相似度计算: 使用余弦相似度 (Cosine Similarity)

use crate::config::Config;
use langchainrust::{
    Document, SearchResult, VectorStore, Embeddings,
    QdrantVectorStore,
    OpenAIEmbeddings,
};
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{Filter, Condition, DeletePointsBuilder};
use std::sync::Arc;
use thiserror::Error;

// ============================================================================
// 错误类型定义
// ============================================================================

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Qdrant 连接失败: {0}")]
    ConnectionError(String),
    
    #[error("文档添加失败: {0}")]
    AddError(String),
    
    #[error("搜索失败: {0}")]
    SearchError(String),
    
    #[error("向量生成失败: {0}")]
    EmbeddingError(String),
    
    #[error("文档不存在: {0}")]
    NotFound(String),
}

// ============================================================================
// Qdrant 存储结构体
// ============================================================================

pub struct QdrantStore {
    store: Arc<QdrantVectorStore>,
    embeddings: Arc<OpenAIEmbeddings>,
    qdrant_client: Arc<Qdrant>,
    collection_name: String,
    config: Config,
}

impl QdrantStore {
    pub async fn new(config: Config) -> Result<Self, StoreError> {
        let qdrant_config = config.to_langchain_qdrant_config();
        
        let store = QdrantVectorStore::new(qdrant_config).await
            .map_err(|e| StoreError::ConnectionError(e.to_string()))?;
        
        let embeddings_config = config.to_langchain_embeddings_config();
        let embeddings = OpenAIEmbeddings::new(embeddings_config);
        
        let qdrant_client = Qdrant::from_url(&config.qdrant.url).build()
            .map_err(|e| StoreError::ConnectionError(format!("Qdrant client 创建失败: {}", e)))?;
        
        Ok(Self {
            store: Arc::new(store),
            embeddings: Arc::new(embeddings),
            qdrant_client: Arc::new(qdrant_client),
            collection_name: config.qdrant.collection_name.clone(),
            config,
        })
    }
    
    pub async fn add_documents(&self, documents: Vec<Document>) -> Result<Vec<String>, StoreError> {
        let texts: Vec<&str> = documents.iter()
            .map(|d| d.content.as_str())
            .collect();
        
        let embeddings = self.embeddings.embed_documents(&texts).await
            .map_err(|e| StoreError::EmbeddingError(e.to_string()))?;
        
        self.store.add_documents(documents, embeddings).await
            .map_err(|e| StoreError::AddError(e.to_string()))
    }
    
    pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>, StoreError> {
        let query_embedding = self.embeddings.embed_query(query).await
            .map_err(|e| StoreError::EmbeddingError(e.to_string()))?;
        
        let results = self.store.similarity_search(&query_embedding, top_k).await
            .map_err(|e| StoreError::SearchError(e.to_string()))?;
        
        let filtered = results.into_iter()
            .filter(|r| r.score >= self.config.search.min_score)
            .collect();
        
        Ok(filtered)
    }
    
    pub async fn count(&self) -> usize {
        self.store.count().await
    }
    
    pub async fn clear(&self) -> Result<(), StoreError> {
        self.store.clear().await
            .map_err(|e| StoreError::SearchError(e.to_string()))
    }
    
    pub async fn get_document(&self, id: &str) -> Result<Option<Document>, StoreError> {
        self.store.get_document(id).await
            .map_err(|e| StoreError::SearchError(e.to_string()))
    }
    
    pub async fn delete_document(&self, id: &str) -> Result<(), StoreError> {
        self.store.delete_document(id).await
            .map_err(|e| StoreError::SearchError(e.to_string()))
    }
    
    pub fn vector_size(&self) -> usize {
        self.config.qdrant.vector_size
    }

    pub async fn delete_by_metadata(&self, key: &str, value: &str) -> Result<usize, StoreError> {
        let filter = Filter::must([Condition::matches(key, value.to_string())]);

        self.qdrant_client
            .delete_points(
                DeletePointsBuilder::new(&self.collection_name)
                    .points(filter)
            )
            .await
            .map_err(|e| StoreError::SearchError(format!("按metadata删除失败: {}", e)))?;

        Ok(0)
    }
}