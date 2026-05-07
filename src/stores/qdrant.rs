//! Qdrant 向量存储模块
//! 
//! Qdrant 是一个向量数据库，用来存文档的 Embedding 向量。
//! 搜索时把用户问题转成向量，在 Qdrant 中找最相似的文档。
//!
//! 核心操作：
//!   add_documents()   文档入库：Embedding → 存入 Qdrant
//!   search()          文档检索：问题 → Embedding → Qdrant 查询
//!   clear()           清空集合

use crate::config::Config;
use crate::errors::StoreError;
use langchainrust::{
    Document, SearchResult, VectorStore, Embeddings,
    QdrantVectorStore,
    OpenAIEmbeddings,
};
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{Filter, Condition, DeletePointsBuilder};
use std::sync::Arc;

/// Qdrant 向量存储
///
/// 保存了每个文档的：
/// - 原始文本（content）
/// - 向量（1536 维的 Embedding）
/// - 元数据（来源文件名、上传时间等）
pub struct QdrantStore {
    store: Arc<QdrantVectorStore>,    // Qdrant 向量库客户端
    embeddings: Arc<OpenAIEmbeddings>,  // Embedding 模型（把文本→向量）
    qdrant_client: Arc<Qdrant>,         // Qdrant 底层客户端（用于按元数据删除）
    collection_name: String,            // 集合名
    config: Config,
}

impl QdrantStore {
    /// 初始化 QdrantStore
    /// 连接 Qdrant 服务器，初始化 Embedding 模型
    pub async fn new(config: Config) -> Result<Self, StoreError> {
        let qdrant_config = config.to_langchain_qdrant_config();
        
        // 连接 Qdrant
        let store = QdrantVectorStore::new(qdrant_config).await
            .map_err(|e| StoreError::ConnectionError(e.to_string()))?;
        
        // 初始化 Embedding 模型（调用豆包 API 生成向量）
        let embeddings_config = config.to_langchain_embeddings_config();
        let embeddings = OpenAIEmbeddings::new(embeddings_config);
        
        // 创建 Qdrant 客户端（用于高级操作）
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
    
    /// 添加文档到向量库
    /// 流程：Embedding(批量) → 存入 Qdrant
    pub async fn add_documents(&self, documents: Vec<Document>) -> Result<Vec<String>, StoreError> {
        // 提取所有文本
        let texts: Vec<&str> = documents.iter()
            .map(|d| d.content.as_str())
            .collect();
        
        // 批量生成 Embedding（调用豆包 API）
        let embeddings = self.embeddings.embed_documents(&texts).await
            .map_err(|e| StoreError::EmbeddingError(e.to_string()))?;
        
        // 文档 + 向量 一起存入 Qdrant
        self.store.add_documents(documents, embeddings).await
            .map_err(|e| StoreError::AddError(e.to_string()))
    }
    
    /// 搜索最相似的文档
    /// 流程：问题 → Embedding → Qdrant 相似度搜索 → 过滤低分 → 返回
    pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>, StoreError> {
        // 把用户问题转成向量
        let query_embedding = self.embeddings.embed_query(query).await
            .map_err(|e| StoreError::EmbeddingError(e.to_string()))?;
        
        // 在 Qdrant 中搜索最相似的 top_k 个向量
        let results = self.store.similarity_search(&query_embedding, top_k).await
            .map_err(|e| StoreError::SearchError(e.to_string()))?;
        
        // 过滤掉分数低于 min_score 的结果
        let filtered = results.into_iter()
            .filter(|r| r.score >= self.config.search.min_score)
            .collect();
        
        Ok(filtered)
    }

    /// Agent RAG 专用搜索（更松的阈值，搜更多结果）
    pub async fn search_rag(&self, query: &str, top_k: usize) -> Result<Vec<SearchResult>, StoreError> {
        let query_embedding = self.embeddings.embed_query(query).await
            .map_err(|e| StoreError::EmbeddingError(e.to_string()))?;
        let results = self.store.similarity_search(&query_embedding, top_k).await
            .map_err(|e| StoreError::SearchError(e.to_string()))?;
        // Agent RAG 用 0.1 阈值，搜到更多结果
        let filtered = results.into_iter()
            .filter(|r| r.score >= 0.1)
            .collect();
        Ok(filtered)
    }
    
    /// 获取文档总数
    pub async fn count(&self) -> usize {
        self.store.count().await
    }
    
    /// 清空所有文档
    pub async fn clear(&self) -> Result<(), StoreError> {
        self.store.clear().await
            .map_err(|e| StoreError::SearchError(e.to_string()))
    }
    
    /// 获取单个文档
    pub async fn get_document(&self, id: &str) -> Result<Option<Document>, StoreError> {
        self.store.get_document(id).await
            .map_err(|e| StoreError::SearchError(e.to_string()))
    }
    
    /// 删除单个文档
    pub async fn delete_document(&self, id: &str) -> Result<(), StoreError> {
        self.store.delete_document(id).await
            .map_err(|e| StoreError::SearchError(e.to_string()))
    }
    
    /// 获取向量维度
    pub fn vector_size(&self) -> usize {
        self.config.qdrant.vector_size
    }

    /// 按元数据删除文档（比如按文件名删除）
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
