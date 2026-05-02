//! BM25 文档存储模块（MongoDB 持久化）
//!
//! BM25 是一种关键词检索算法（TF-IDF 的升级版）。
//! 本模块基于 langchainrust 的 ChunkedBM25Retriever，
//! 文档分块后存入 MongoDB，检索时使用 BM25 算法计算相关性。
//!
//! 特色：AutoMerging（自动合并）
//! 如果一个父文档的多个子 chunk 都匹配，自动合并返回父文档内容

use crate::config::Config;
use crate::errors::BM25Error;
use crate::models::{BM25SearchResult, DocumentInfo};
use langchainrust::{
    retrieval::bm25::{AutoMergingConfig, ChunkedBM25Retriever},
    vector_stores::{MongoChunkedDocumentStore, MongoStoreConfig},
    Document,
};
use mongodb::{Client, options::ClientOptions};
use std::sync::{Arc, Mutex};

/// BM25 存储
///
/// 文档层级：
///   父文档（Parent）→ 原始上传的文件
///   子文档（Chunk）  → 分块后的片段
///
/// 检索时支持 AutoMerging：多个子 chunk 匹配时自动合并成父文档返回
pub struct BM25Store {
    retriever: Arc<Mutex<ChunkedBM25Retriever<MongoChunkedDocumentStore>>>,
    mongo_client: Client,                    // MongoDB 客户端（用于高级操作）
    database_name: String,
    parent_collection_name: String,          // 父文档集合名
    chunk_collection_name: String,           // 子文档集合名
}

impl BM25Store {
    /// 初始化：连接 MongoDB → 创建集合索引 → 初始化 BM25 检索器
    pub async fn new(config: &Config) -> Result<Self, BM25Error> {
        let mongo_config = MongoStoreConfig::new(
            &config.mongodb.uri,
            &config.mongodb.database,
        )
        .with_collections(
            &config.mongodb.parent_collection,
            &config.mongodb.chunk_collection,
        );

        // 创建 MongoDB 文档存储
        let mongo_store = MongoChunkedDocumentStore::new(mongo_config)
            .await
            .map_err(|e| BM25Error::MongoError(e.to_string()))?;

        // 创建索引（加速查询）
        mongo_store
            .create_indexes()
            .await
            .map_err(|e| BM25Error::MongoError(e.to_string()))?;

        tracing::info!("MongoDB BM25 存储已连接: {}", config.mongodb.uri);

        let document_store = Arc::new(mongo_store);

        // BM25 配置：自动合并+分块大小
        let bm25_config = AutoMergingConfig::new()
            .with_leaf_size(config.document.chunk_size)     // 子 chunk 大小 500
            .with_threshold(0.5);                             // 合并阈值 0.5

        let retriever = ChunkedBM25Retriever::with_config(document_store.clone(), bm25_config);

        // 创建 MongoDB 客户端（用于文档管理）
        let client_options = ClientOptions::parse(&config.mongodb.uri)
            .await
            .map_err(|e| BM25Error::MongoError(e.to_string()))?;
        
        let mongo_client = Client::with_options(client_options)
            .map_err(|e| BM25Error::MongoError(e.to_string()))?;

        Ok(Self {
            retriever: Arc::new(Mutex::new(retriever)),
            mongo_client,
            database_name: config.mongodb.database.clone(),
            parent_collection_name: config.mongodb.parent_collection.clone(),
            chunk_collection_name: config.mongodb.chunk_collection.clone(),
        })
    }

    /// 添加文档到 BM25 索引
    pub fn add_documents(&self, documents: Vec<Document>) -> Result<(), BM25Error> {
        let mut retriever = self.retriever.lock().unwrap();
        for doc in documents {
            retriever.add_document(doc);  // 自动分块 + 建索引 + 存入 MongoDB
        }
        Ok(())
    }

    /// 使用 BM25 算法搜索
    /// 返回按相关性排序的文档列表，支持 AutoMerging
    pub fn search(&self, query: &str, k: usize) -> Result<Vec<BM25SearchResult>, BM25Error> {
        let mut retriever = self.retriever.lock().unwrap();
        let results = retriever.search(query, k);  // BM25 算法计算

        let search_results: Vec<BM25SearchResult> = results
            .into_iter()
            .enumerate()
            .map(|(idx, r)| {
                let is_merged = r.is_merged();  // 是否经过了自动合并
                let chunk_id = if is_merged {
                    r.parent_id.clone()
                } else if r.leaf_chunks.len() > 0 {
                    r.leaf_chunks[0].chunk_id.clone()
                } else {
                    format!("bm25_{}", idx)
                };
                BM25SearchResult {
                    id: chunk_id,
                    content: r.content(),
                    score: r.score,
                    parent_id: r.parent_id,
                    is_merged,
                }
            })
            .collect();

        Ok(search_results)
    }

    /// 获取 BM25 索引中的文档总数
    pub fn count(&self) -> usize {
        self.retriever.lock().unwrap().len()
    }

    /// 清空所有 BM25 索引
    pub fn clear(&self) -> Result<(), BM25Error> {
        self.retriever.lock().unwrap().clear();
        Ok(())
    }

    /// 是否使用了 MongoDB 持久化（是的）
    pub fn is_mongo(&self) -> bool {
        true
    }

    /// ──────────────────── 文档管理 ────────────────────

    /// 列出所有文档
    pub async fn list_documents(&self) -> Result<Vec<DocumentInfo>, BM25Error> {
        use mongodb::bson::doc;
        let db = self.mongo_client.database(&self.database_name);
        let parent_collection = db.collection::<mongodb::bson::Document>(&self.parent_collection_name);
        let chunk_collection = db.collection::<mongodb::bson::Document>(&self.chunk_collection_name);
        
        let mut cursor = parent_collection.find(doc! {}, None).await
            .map_err(|e| BM25Error::OperationError(e.to_string()))?;
        
        let mut documents = Vec::new();
        while cursor.advance().await.map_err(|e| BM25Error::OperationError(e.to_string()))? {
            let doc = cursor.deserialize_current()
                .map_err(|e| BM25Error::OperationError(e.to_string()))?;
            
            let id = doc.get_str("_id").unwrap_or("unknown").to_string();
            let content = doc.get_str("content").unwrap_or("");
            let metadata: std::collections::HashMap<String, String> = doc.get_document("metadata")
                .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.to_string(), s.to_string()))).collect())
                .unwrap_or_default();
            
            let chunk_count = chunk_collection
                .count_documents(doc! { "parent_id": &id }, None).await
                .map_err(|e| BM25Error::OperationError(e.to_string()))? as usize;
            
            let title = metadata.get("original_filename").cloned()
                .unwrap_or_else(|| content.chars().take(50).collect());
            
            documents.push(DocumentInfo {
                id, title,
                content_preview: content.chars().take(100).collect(),
                chunk_count, metadata,
            });
        }
        Ok(documents)
    }

    /// 删除文档（同时删除父文档和所有子 chunk）
    pub async fn delete_document(&self, parent_id: &str) -> Result<(), BM25Error> {
        use mongodb::bson::doc;
        let db = self.mongo_client.database(&self.database_name);
        let parent_collection = db.collection::<mongodb::bson::Document>(&self.parent_collection_name);
        let chunk_collection = db.collection::<mongodb::bson::Document>(&self.chunk_collection_name);
        
        chunk_collection.delete_many(doc! { "parent_id": parent_id }, None).await
            .map_err(|e| BM25Error::OperationError(e.to_string()))?;
        parent_collection.delete_one(doc! { "_id": parent_id }, None).await
            .map_err(|e| BM25Error::OperationError(e.to_string()))?;
        Ok(())
    }
    
    /// 获取文档信息（文件名、元数据）
    pub async fn get_document_info(&self, parent_id: &str) -> Result<Option<DocumentFileInfo>, BM25Error> {
        use mongodb::bson::doc;
        let db = self.mongo_client.database(&self.database_name);
        let parent_collection = db.collection::<mongodb::bson::Document>(&self.parent_collection_name);
        
        let doc = parent_collection.find_one(doc! { "_id": parent_id }, None).await
            .map_err(|e| BM25Error::OperationError(e.to_string()))?;
        
        if let Some(d) = doc {
            let metadata: std::collections::HashMap<String, String> = d.get_document("metadata")
                .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.to_string(), s.to_string()))).collect())
                .unwrap_or_default();
            let filename = metadata.get("original_filename").cloned().unwrap_or_default();
            Ok(Some(DocumentFileInfo { parent_id: parent_id.to_string(), filename, metadata }))
        } else {
            Ok(None)
        }
    }
    
    /// 给文档加标签
    pub async fn add_document_tags(&self, parent_id: &str, tags: &[String]) -> Result<(), BM25Error> {
        use mongodb::bson::doc;
        let db = self.mongo_client.database(&self.database_name);
        let parent_collection = db.collection::<mongodb::bson::Document>(&self.parent_collection_name);
        let tags_doc: Vec<mongodb::bson::Bson> = tags.iter().map(|t| mongodb::bson::Bson::String(t.clone())).collect();
        parent_collection.update_one(
            doc! { "_id": parent_id },
            doc! { "$set": { "metadata.tags": tags_doc } },
            None
        ).await.map_err(|e| BM25Error::OperationError(e.to_string()))?;
        Ok(())
    }
    
    /// 按标签查找文档
    pub async fn get_documents_by_tag(&self, tag: &str) -> Result<Vec<DocumentInfo>, BM25Error> {
        use mongodb::bson::doc;
        let db = self.mongo_client.database(&self.database_name);
        let parent_collection = db.collection::<mongodb::bson::Document>(&self.parent_collection_name);
        let chunk_collection = db.collection::<mongodb::bson::Document>(&self.chunk_collection_name);
        
        let mut cursor = parent_collection.find(doc! { "metadata.tags": tag }, None).await
            .map_err(|e| BM25Error::OperationError(e.to_string()))?;
        
        let mut documents = Vec::new();
        while cursor.advance().await.map_err(|e| BM25Error::OperationError(e.to_string()))? {
            let doc = cursor.deserialize_current()
                .map_err(|e| BM25Error::OperationError(e.to_string()))?;
            
            let id = doc.get_str("_id").unwrap_or("unknown").to_string();
            let content = doc.get_str("content").unwrap_or("");
            let metadata: std::collections::HashMap<String, String> = doc.get_document("metadata")
                .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.to_string(), s.to_string()))).collect())
                .unwrap_or_default();
            
            let chunk_count = chunk_collection
                .count_documents(doc! { "parent_id": &id }, None).await
                .map_err(|e| BM25Error::OperationError(e.to_string()))? as usize;
            
            let title = metadata.get("original_filename").cloned()
                .unwrap_or_else(|| content.chars().take(50).collect());
            
            documents.push(DocumentInfo {
                id, title,
                content_preview: content.chars().take(100).collect(),
                chunk_count, metadata,
            });
        }
        Ok(documents)
    }
}

/// 文档文件信息
#[derive(Debug, Clone)]
pub struct DocumentFileInfo {
    pub parent_id: String,
    pub filename: String,
    pub metadata: std::collections::HashMap<String, String>,
}
