//! API 业务服务层

use crate::config::Config;
use crate::errors::ApiError;
use crate::models::*;
use crate::stores::*;
use crate::utils::DocumentProcessor;
use crate::services::LangGraphDemoService;
use crate::stores::ApiStatsSummary;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

pub struct ApiService {
    pub vector_store: Arc<QdrantStore>,
    pub bm25_store: Arc<BM25Store>,
    pub hybrid_store: Arc<HybridStore>,
    pub conversation_store: Arc<ConversationStore>,
    processor: DocumentProcessor,
    config: Config,
}

impl ApiService {
    pub async fn new(config: Config) -> Result<Self, ApiError> {
        let vector_store = Arc::new(QdrantStore::new(config.clone()).await?);
        let bm25_store = Arc::new(BM25Store::new(&config).await?);
        let hybrid_store = Arc::new(HybridStore::new(
            bm25_store.clone(),
            vector_store.clone(),
            config.clone(),
        ));
        let conversation_store = Arc::new(ConversationStore::new(&config).await?);
        let processor = DocumentProcessor::new(config.clone());
        
        tracing::info!("BM25 存储使用 MongoDB 持久化");
        tracing::info!("对话记忆使用 SQLite 持久化（参考 OpenCode）: {}", config.sqlite.db_path);
        
        Ok(Self {
            vector_store,
            bm25_store,
            hybrid_store,
            conversation_store,
            processor,
            config,
        })
    }
    
    pub async fn upload_file(&self, file_path: &Path, original_name: &str) -> Result<UploadResponse, ApiError> {
        let (original_docs, chunks) = self.processor.process_file(file_path).await?;
        
        let doc_count = original_docs.len();
        let chunk_count = chunks.len();
        
        let chunk_documents: Vec<langchainrust::Document> = chunks.into_iter()
            .enumerate()
            .map(|(i, chunk)| {
                chunk
                    .with_id(format!("{}_{}", Uuid::new_v4(), i))
                    .with_metadata("original_filename", original_name)
                    .with_metadata("upload_time", chrono::Utc::now().to_rfc3339())
            })
            .collect();
        
        let vector_ids = self.vector_store.add_documents(chunk_documents).await?;
        
        let parent_documents: Vec<langchainrust::Document> = original_docs.into_iter()
            .enumerate()
            .map(|(_i, doc)| {
                doc
                    .with_id(format!("parent_{}", Uuid::new_v4()))
                    .with_metadata("original_filename", original_name)
                    .with_metadata("upload_time", chrono::Utc::now().to_rfc3339())
                    .with_metadata("chunk_count", chunk_count.to_string())
            })
            .collect();
        
        self.bm25_store.add_documents(parent_documents)?;
        
        Ok(UploadResponse {
            success: true,
            document_count: doc_count,
            chunk_count: vector_ids.len(),
            message: format!("成功上传 {} 个原始文档，分割为 {} 个chunks（向量+BM25 MongoDB 已持久化）", doc_count, vector_ids.len()),
            document_ids: vector_ids,
        })
    }
    
    pub async fn search_vector(&self, request: SearchRequest) -> Result<SearchResponse, ApiError> {
        let results = self.vector_store.search(&request.query, request.top_k).await?;
        
        let items: Vec<SearchResultItem> = results.into_iter()
            .map(|r| SearchResultItem {
                id: r.document.id.clone(),
                content: r.document.content.clone(),
                score: r.score,
                source: Some("vector".to_string()),
                metadata: serde_json::to_value(&r.document.metadata).unwrap_or(serde_json::Value::Null),
            })
            .collect();
        
        Ok(SearchResponse {
            query: request.query,
            mode: "vector".to_string(),
            results: items.clone(),
            total_count: items.len(),
        })
    }
    
    pub fn search_bm25(&self, request: SearchRequest) -> Result<SearchResponse, ApiError> {
        let results = self.bm25_store.search(&request.query, request.top_k)?;
        
        let items: Vec<SearchResultItem> = results.into_iter()
            .map(|r| SearchResultItem {
                id: Some(r.id.clone()),
                content: r.content.clone(),
                score: r.score,
                source: Some("bm25".to_string()),
                metadata: serde_json::json!({
                    "parent_id": r.parent_id,
                    "is_merged": r.is_merged,
                }),
            })
            .collect();
        
        Ok(SearchResponse {
            query: request.query.clone(),
            mode: "bm25".to_string(),
            results: items.clone(),
            total_count: items.len(),
        })
    }
    
    pub async fn search_hybrid(&self, request: SearchRequest) -> Result<SearchResponse, ApiError> {
        let results = self.hybrid_store.search(&request.query, request.top_k).await?;
        
        let items: Vec<SearchResultItem> = results.into_iter()
            .map(|r| SearchResultItem {
                id: r.id.clone(),
                content: r.content.clone(),
                score: r.rrf_score,
                source: Some(r.source.clone()),
                metadata: serde_json::json!({
                    "bm25_score": r.bm25_score,
                    "vector_score": r.vector_score,
                }),
            })
            .collect();
        
        Ok(SearchResponse {
            query: request.query,
            mode: "hybrid".to_string(),
            results: items.clone(),
            total_count: items.len(),
        })
    }
    
    pub async fn compare_search(&self, query: String, top_k: usize) -> Result<CompareResponse, ApiError> {
        let vector_results = self.vector_store.search(&query, top_k).await?;
        let bm25_results = self.bm25_store.search(&query, top_k)?;
        let hybrid_results = self.hybrid_store.search(&query, top_k).await?;
        
        let vector_items: Vec<SearchResultItem> = vector_results.into_iter()
            .map(|r| SearchResultItem {
                id: r.document.id.clone(),
                content: r.document.content.clone(),
                score: r.score,
                source: Some("vector".to_string()),
                metadata: serde_json::Value::Null,
            })
            .collect();
        
        let bm25_items: Vec<SearchResultItem> = bm25_results.into_iter()
            .map(|r| SearchResultItem {
                id: Some(r.id.clone()),
                content: r.content.clone(),
                score: r.score,
                source: Some("bm25".to_string()),
                metadata: serde_json::json!({
                    "parent_id": r.parent_id,
                    "is_merged": r.is_merged,
                }),
            })
            .collect();
        
        let hybrid_items: Vec<SearchResultItem> = hybrid_results.into_iter()
            .map(|r| SearchResultItem {
                id: r.id.clone(),
                content: r.content.clone(),
                score: r.rrf_score,
                source: Some(r.source.clone()),
                metadata: serde_json::Value::Null,
            })
            .collect();
        
        let vector_ids: Vec<String> = vector_items.iter()
            .filter_map(|i| i.id.clone())
            .collect();
        let bm25_ids: Vec<String> = bm25_items.iter()
            .filter_map(|i| i.id.clone())
            .collect();
        let _hybrid_ids: Vec<String> = hybrid_items.iter()
            .filter_map(|i| i.id.clone())
            .collect();
        
        let overlap = vector_ids.iter()
            .filter(|id| bm25_ids.contains(id))
            .count();
        
        let comparison = SearchComparison {
            vector_top1_score: vector_items.first().map(|r| r.score).unwrap_or(0.0),
            bm25_top1_score: bm25_items.first().map(|r| r.score).unwrap_or(0.0),
            hybrid_top1_score: hybrid_items.first().map(|r| r.score).unwrap_or(0.0),
            overlap_count: overlap,
            unique_vector: vector_items.len() - overlap,
            unique_bm25: bm25_items.len() - overlap,
            unique_hybrid: hybrid_items.len(),
        };
        
        Ok(CompareResponse {
            query,
            vector_results: vector_items,
            bm25_results: bm25_items,
            hybrid_results: hybrid_items,
            comparison,
        })
    }
    
    pub async fn get_stats(&self) -> Result<StatsResponse, ApiError> {
        let sessions = self.conversation_store.get_sessions().await?;
        Ok(StatsResponse {
            total_documents: self.vector_store.count().await,
            vector_size: self.vector_store.vector_size(),
            bm25_chunks: self.bm25_store.count(),
            bm25_persisted: self.bm25_store.is_mongo(),
            collection_name: self.config.qdrant.collection_name.clone(),
            conversation_sessions: sessions.len(),
        })
    }
    
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ApiError> {
        use crate::models::SearchMode;
        
        let search_mode = SearchMode::from_flags(request.use_vector, request.use_bm25);
        
        let rag_sources = match search_mode {
            SearchMode::None => Vec::new(),
            SearchMode::Vector => {
                let results = self.vector_store.search(&request.message, request.top_k).await?;
                results.into_iter().map(|r| SourceInfo {
                    content: r.document.content.clone(),
                    score: r.score,
                    source: "vector".to_string(),
                }).collect()
            },
            SearchMode::BM25 => {
                let results = self.bm25_store.search(&request.message, request.top_k)?;
                results.into_iter().map(|r| SourceInfo {
                    content: r.content.clone(),
                    score: r.score,
                    source: "bm25".to_string(),
                }).collect()
            },
            SearchMode::Hybrid => {
                let results = self.hybrid_store.search(&request.message, request.top_k).await?;
                results.into_iter().map(|r| SourceInfo {
                    content: r.content.clone(),
                    score: r.rrf_score,
                    source: r.source.clone(),
                }).collect()
            },
        };
        
        let response = self.conversation_store.chat(request, rag_sources).await?;
        Ok(response)
    }
    
    pub async fn get_conversation_history(&self, session_id: &str) -> Result<Vec<ConversationMessage>, ApiError> {
        let history = self.conversation_store.get_history(session_id).await?;
        Ok(history)
    }
    
    pub async fn get_sessions(&self) -> Result<Vec<SessionInfo>, ApiError> {
        let sessions = self.conversation_store.get_sessions().await?;
        Ok(sessions)
    }
    
    pub async fn clear_session(&self, session_id: &str) -> Result<(), ApiError> {
        self.conversation_store.clear_session(session_id).await?;
        Ok(())
    }
    
    pub async fn edit_message(&self, message_id: &str, content: &str) -> Result<(), ApiError> {
        self.conversation_store.edit_message(message_id, content).await?;
        Ok(())
    }
    
    pub async fn delete_message(&self, message_id: &str) -> Result<(), ApiError> {
        self.conversation_store.delete_message(message_id).await?;
        Ok(())
    }
    
    pub async fn clear_all(&self) -> Result<(), ApiError> {
        self.vector_store.clear().await?;
        self.bm25_store.clear()?;
        self.conversation_store.clear_all().await?;
        Ok(())
    }
    
    pub async fn list_documents(&self) -> Result<Vec<DocumentInfo>, ApiError> {
        let documents = self.bm25_store.list_documents().await?;
        Ok(documents)
    }

    pub async fn delete_document(&self, parent_id: &str, filename: &str) -> Result<DeleteDocumentResponse, ApiError> {
        self.bm25_store.delete_document(parent_id).await?;
        
        let deleted_count = self.vector_store.delete_by_metadata("original_filename", filename).await?;
        
        Ok(DeleteDocumentResponse {
            success: true,
            parent_id: parent_id.to_string(),
            bm25_chunks_deleted: true,
            vector_count_deleted: deleted_count,
            message: format!("成功删除文档 {}，BM25 chunks 和 {} 个向量已清除", filename, deleted_count),
        })
    }
    
    pub fn get_langgraph_info() -> serde_json::Value {
        LangGraphDemoService::new().get_graph_info()
    }
    
    pub async fn run_langgraph_parallel(input: String) -> Result<ParallelDemoResult, ApiError> {
        let service = LangGraphDemoService::new();
        service.run_parallel_demo(input).await.map_err(|e| ApiError::SearchError(e.to_string()))
    }
    
    pub async fn run_langgraph_conditional(input: String) -> Result<ConditionalDemoResult, ApiError> {
        let service = LangGraphDemoService::new();
        service.run_conditional_demo(input).await.map_err(|e| ApiError::SearchError(e.to_string()))
    }
    
    pub async fn run_langgraph_stream(input: String) -> Result<Vec<StreamDemoEvent>, ApiError> {
        let service = LangGraphDemoService::new();
        service.run_stream_demo(input).await.map_err(|e| ApiError::SearchError(e.to_string()))
    }
    
    pub async fn get_api_stats(&self) -> Result<ApiStatsSummary, ApiError> {
        let stats = self.conversation_store.get_api_stats().await?;
        Ok(stats)
    }
    
    pub async fn record_api_call(&self, api_type: &str, tokens: i64, duration_ms: i64, success: bool) -> Result<(), ApiError> {
        self.conversation_store.record_api_call(api_type, tokens, duration_ms, success).await?;
        Ok(())
    }
    
    pub async fn regenerate_message(&self, message_id: &str) -> Result<RegenerateResponse, ApiError> {
        let (session_id, new_message_id, reply) = self.conversation_store.regenerate_message(message_id).await?;
        Ok(RegenerateResponse {
            message_id: new_message_id,
            reply,
        })
    }
    
    pub async fn export_session(&self, session_id: &str) -> Result<SessionExport, ApiError> {
        let session = self.conversation_store.get_session_info(session_id).await?;
        let messages = self.conversation_store.get_history(session_id).await?;
        Ok(SessionExport {
            session_id: session_id.to_string(),
            title: session.title,
            created_at: session.created_at,
            messages,
        })
    }
    
    pub async fn import_session(&self, import: SessionImport) -> Result<String, ApiError> {
        let session_id = self.conversation_store.import_session(import).await?;
        Ok(session_id)
    }
    
    pub async fn search_sessions(&self, query: &str) -> Result<Vec<SessionInfo>, ApiError> {
        let sessions = self.conversation_store.search_sessions(query).await?;
        Ok(sessions)
    }
    
    pub async fn batch_delete_documents(&self, parent_ids: Vec<String>) -> Result<BatchDeleteResponse, ApiError> {
        let mut deleted_count = 0;
        let mut failed_count = 0;
        
        for parent_id in parent_ids {
            let doc_info = self.bm25_store.get_document_info(&parent_id).await?;
            if let Some(info) = doc_info {
                let filename = info.filename.clone();
                self.bm25_store.delete_document(&parent_id).await?;
                self.vector_store.delete_by_metadata("original_filename", &filename).await?;
                deleted_count += 1;
            } else {
                failed_count += 1;
            }
        }
        
        Ok(BatchDeleteResponse {
            success: true,
            deleted_count,
            failed_count,
            message: format!("成功删除 {} 个文档，失败 {} 个", deleted_count, failed_count),
        })
    }
    
    pub async fn add_document_tags(&self, parent_id: &str, tags: &[String]) -> Result<(), ApiError> {
        self.bm25_store.add_document_tags(parent_id, tags).await?;
        Ok(())
    }
    
    pub async fn get_documents_by_tag(&self, tag: &str) -> Result<Vec<DocumentInfo>, ApiError> {
        let documents = self.bm25_store.get_documents_by_tag(tag).await?;
        Ok(documents)
    }
}