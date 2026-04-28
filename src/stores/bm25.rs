//! BM25 文档存储模块（MongoDB 持久化）

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

pub struct BM25Store {
    retriever: Arc<Mutex<ChunkedBM25Retriever<MongoChunkedDocumentStore>>>,
    mongo_client: Client,
    database_name: String,
    parent_collection_name: String,
    chunk_collection_name: String,
}

impl BM25Store {
    pub async fn new(config: &Config) -> Result<Self, BM25Error> {
        let mongo_config = MongoStoreConfig::new(
            &config.mongodb.uri,
            &config.mongodb.database,
        )
        .with_collections(
            &config.mongodb.parent_collection,
            &config.mongodb.chunk_collection,
        );

        let mongo_store = MongoChunkedDocumentStore::new(mongo_config)
            .await
            .map_err(|e| BM25Error::MongoError(e.to_string()))?;

        mongo_store
            .create_indexes()
            .await
            .map_err(|e| BM25Error::MongoError(e.to_string()))?;

        tracing::info!("MongoDB BM25 存储已连接: {}", config.mongodb.uri);

        let document_store = Arc::new(mongo_store);

        let bm25_config = AutoMergingConfig::new()
            .with_leaf_size(config.document.chunk_size)
            .with_threshold(0.5);

        let retriever = ChunkedBM25Retriever::with_config(document_store.clone(), bm25_config);

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

    pub fn add_documents(&self, documents: Vec<Document>) -> Result<(), BM25Error> {
        let mut retriever = self.retriever.lock().unwrap();
        for doc in documents {
            retriever.add_document(doc);
        }
        Ok(())
    }

    pub fn search(&self, query: &str, k: usize) -> Result<Vec<BM25SearchResult>, BM25Error> {
        let mut retriever = self.retriever.lock().unwrap();
        let results = retriever.search(query, k);

        let search_results: Vec<BM25SearchResult> = results
            .into_iter()
            .map(|r| {
                let is_merged = r.is_merged();
                BM25SearchResult {
                    content: r.content(),
                    score: r.score,
                    parent_id: r.parent_id,
                    is_merged,
                }
            })
            .collect();

        Ok(search_results)
    }

    pub fn count(&self) -> usize {
        self.retriever.lock().unwrap().len()
    }

    pub fn clear(&self) -> Result<(), BM25Error> {
        self.retriever.lock().unwrap().clear();
        Ok(())
    }

    pub fn is_mongo(&self) -> bool {
        true
    }

    pub async fn list_documents(&self) -> Result<Vec<DocumentInfo>, BM25Error> {
        use mongodb::bson::doc;
        
        let db = self.mongo_client.database(&self.database_name);
        let parent_collection = db.collection::<mongodb::bson::Document>(&self.parent_collection_name);
        let chunk_collection = db.collection::<mongodb::bson::Document>(&self.chunk_collection_name);
        
        let mut cursor = parent_collection
            .find(doc! {}, None)
            .await
            .map_err(|e| BM25Error::OperationError(e.to_string()))?;
        
        let mut documents = Vec::new();
        
        while cursor.advance().await.map_err(|e| BM25Error::OperationError(e.to_string()))? {
            let doc = cursor.deserialize_current()
                .map_err(|e| BM25Error::OperationError(e.to_string()))?;
            
            let id = doc.get_str("_id").unwrap_or("unknown").to_string();
            let content = doc.get_str("content").unwrap_or("");
            let metadata: std::collections::HashMap<String, String> = doc.get_document("metadata")
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.to_string(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            
            let chunk_count = chunk_collection
                .count_documents(doc! { "parent_id": &id }, None)
                .await
                .map_err(|e| BM25Error::OperationError(e.to_string()))? as usize;
            
            let title = metadata.get("original_filename")
                .cloned()
                .unwrap_or_else(|| {
                    content.chars().take(50).collect()
                });
            
            let content_preview: String = content.chars().take(100).collect();
            
            documents.push(DocumentInfo {
                id,
                title,
                content_preview,
                chunk_count,
                metadata,
            });
        }
        
        Ok(documents)
    }

    pub async fn delete_document(&self, parent_id: &str) -> Result<(), BM25Error> {
        use mongodb::bson::doc;
        
        let db = self.mongo_client.database(&self.database_name);
        let parent_collection = db.collection::<mongodb::bson::Document>(&self.parent_collection_name);
        let chunk_collection = db.collection::<mongodb::bson::Document>(&self.chunk_collection_name);
        
        chunk_collection
            .delete_many(doc! { "parent_id": parent_id }, None)
            .await
            .map_err(|e| BM25Error::OperationError(e.to_string()))?;
        
        parent_collection
            .delete_one(doc! { "_id": parent_id }, None)
            .await
            .map_err(|e| BM25Error::OperationError(e.to_string()))?;
        
        Ok(())
    }
}