//! API 业务服务层
//!
//! 这是整个系统的核心编排器（大脑），把所有模块串在一起：
//! - 文档上传 → 处理 → 存储（Qdrant + MongoDB）
//! - 对话 → 检索 → 上下文构建 → LLM调用
//! - 统计、导出、分支等管理功能
//!
//! ApiService 的结构：
//!   vector_store      → Qdrant 向量检索
//!   bm25_store        → MongoDB BM25 检索
//!   hybrid_store      → RRF 混合检索
//!   conversation_store → SQLite 对话历史 + 压缩
//!   processor         → 文档加载 + 分块

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

/// API 服务：组合所有存储模块，对外提供统一的业务方法
pub struct ApiService {
    pub vector_store: Arc<QdrantStore>,            // Qdrant 向量存储
    pub bm25_store: Arc<BM25Store>,                // BM25 关键词存储（MongoDB）
    pub hybrid_store: Arc<HybridStore>,             // 混合检索（RRF）
    pub conversation_store: Arc<ConversationStore>, // 对话历史（SQLite）
    processor: DocumentProcessor,                   // 文档处理器（加载+分块）
    config: Config,                                 // 配置
}

impl ApiService {
    /// 初始化 API 服务
    /// 依次连接：Qdrant → MongoDB(BM25) → RRF混合器 → SQLite(对话) → 文档处理器
    pub async fn new(config: Config) -> Result<Self, ApiError> {
        // 1. 连接 Qdrant 向量数据库
        let vector_store = Arc::new(QdrantStore::new(config.clone()).await?);
        
        // 2. 连接 MongoDB，初始化 BM25 检索器
        let bm25_store = Arc::new(BM25Store::new(&config).await?);
        
        // 3. 创建 RRF 混合检索器（包装 BM25 + 向量）
        let hybrid_store = Arc::new(HybridStore::new(
            bm25_store.clone(),
            vector_store.clone(),
            config.clone(),
        ));
        
        // 4. 连接 SQLite，创建对话存储（含压缩）
        let conversation_store = Arc::new(ConversationStore::new(&config).await?);
        
        // 5. 创建文档处理器
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
    
    /// ──────────────────── 文档上传 ────────────────────
    
    /// 上传并处理文件
    /// 流程：加载 → 分块 → Qdrant(向量) + MongoDB(BM25)
    pub async fn upload_file(&self, file_path: &Path, original_name: &str) -> Result<UploadResponse, ApiError> {
        let (original_docs, chunks) = self.processor.process_file(file_path).await?;
        //   ↑ 原始文档          ↑ 分块后的chunks
        
        let doc_count = original_docs.len();
        let chunk_count = chunks.len();
        
        // chunks 加上元数据（来源文件名、上传时间）
        let chunk_documents: Vec<langchainrust::Document> = chunks.into_iter()
            .enumerate()
            .map(|(i, chunk)| {
                chunk
                    .with_id(format!("{}_{}", Uuid::new_v4(), i))
                    .with_metadata("original_filename", original_name)
                    .with_metadata("upload_time", chrono::Utc::now().to_rfc3339())
            })
            .collect();
        
        // 向量库：存分块后的文档（用于语义搜索）
        let vector_ids = self.vector_store.add_documents(chunk_documents).await?;
        
        // BM25 库：存原始文档（用于关键词搜索）
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
    
    // ──────────────────── 搜索 ────────────────────
    
    /// 向量检索：用户问题 → Embedding → Qdrant 搜相似
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
    
    /// BM25 检索：关键词匹配 → MongoDB BM25 索引
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
    
    /// 混合检索：同时跑向量 + BM25，RRF 算法融合
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
    
    /// 三种检索对比（同时跑向量+BM25+混合，返回结果对比）
    pub async fn compare_search(&self, query: String, top_k: usize) -> Result<CompareResponse, ApiError> {
        // 同时跑三种检索
        let vector_results = self.vector_store.search(&query, top_k).await?;
        let bm25_results = self.bm25_store.search(&query, top_k)?;
        let hybrid_results = self.hybrid_store.search(&query, top_k).await?;
        
        // 格式化结果
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
        
        // 统计重叠情况
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
    
    /// ──────────────────── 统计 ────────────────────
    
    /// 获取系统统计
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
    
    /// ──────────────────── 对话 ────────────────────
    
    /// 执行一次对话
    /// 流程：选择检索模式 → 检索知识库 → 构建上下文 → LLM调用 → 保存历史
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ApiError> {
        use crate::models::SearchMode;
        
        // 第一步：决定检索模式
        let search_mode = SearchMode::from_flags(request.use_vector, request.use_bm25);
        
        // 第二步：检索知识库，获取相关文档
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
        
        // 第三步：交给对话引擎（包含历史加载、压缩、上下文构建、LLM调用）
        let response = self.conversation_store.chat(request, rag_sources).await?;
        Ok(response)
    }
    
    /// ──────────────────── 对话历史管理 ────────────────────
    
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
    
    /// 清空全部数据（向量库 + BM25 + 对话历史）
    pub async fn clear_all(&self) -> Result<(), ApiError> {
        self.vector_store.clear().await?;
        self.bm25_store.clear()?;
        self.conversation_store.clear_all().await?;
        Ok(())
    }
    
    /// ──────────────────── 文档管理 ────────────────────
    
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
    
    /// ──────────────────── LangGraph ────────────────────
    
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
    
    pub fn get_langgraph_structure(mode: String) -> Result<LangGraphStructureResponse, ApiError> {
        let mermaid = LangGraphDemoService::get_graph_mermaid(&mode)
            .map_err(|e| ApiError::SearchError(e.to_string()))?;
        let structure = LangGraphDemoService::get_graph_structure(&mode)
            .map_err(|e| ApiError::SearchError(e.to_string()))?;
        Ok(LangGraphStructureResponse { mode, mermaid, structure })
    }
    
    pub async fn decompose_task(&self, task: String) -> Result<TaskDecomposeResult, ApiError> {
        LangGraphDemoService::decompose_task(&self.config, task).await
            .map_err(|e| ApiError::SearchError(e.to_string()))
    }
    
    pub async fn execute_sub_tasks(&self, task: String, sub_tasks: Vec<SubTaskDef>) -> Result<TaskExecuteResult, ApiError> {
        let results = LangGraphDemoService::execute_sub_tasks(&self.config, task, sub_tasks).await
            .map_err(|e| ApiError::SearchError(e.to_string()))?;
        Ok(TaskExecuteResult { execution_results: results })
    }
    
    /// ──────────────────── 真实 Agent 系统 ────────────────────
    
    pub async fn agent_plan(&self, task: String) -> Result<AgentPlan, ApiError> {
        crate::services::agent_executor::AgentEngine::plan(&self.config, task).await
            .map_err(|e| ApiError::SearchError(e.to_string()))
    }
    
    pub async fn agent_execute(&self, task: String, agent_tasks: Vec<AgentTask>) -> Result<AgentExecResponse, ApiError> {
        crate::services::agent_executor::AgentEngine::execute(&self.config, task, agent_tasks).await
            .map_err(|e| ApiError::SearchError(e.to_string()))
    }
    
    /// ──────────────────── API统计 ────────────────────
    
    pub async fn get_api_stats(&self) -> Result<ApiStatsSummary, ApiError> {
        let stats = self.conversation_store.get_api_stats().await?;
        Ok(stats)
    }
    
    pub async fn record_api_call(&self, api_type: &str, tokens: i64, duration_ms: i64, success: bool) -> Result<(), ApiError> {
        self.conversation_store.record_api_call(api_type, tokens, duration_ms, success).await?;
        Ok(())
    }
    
    /// ──────────────────── 重生成 ────────────────────
    
    pub async fn regenerate_message(&self, message_id: &str) -> Result<RegenerateResponse, ApiError> {
        let (session_id, new_message_id, reply) = self.conversation_store.regenerate_message(message_id).await?;
        Ok(RegenerateResponse {
            message_id: new_message_id,
            reply,
        })
    }
    
    /// ──────────────────── 导入/导出 ────────────────────
    
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
    
    /// 分支会话：从某条消息位置创建新会话
    pub async fn branch_session(&self, session_id: &str, from_message_id: &str) -> Result<BranchResponse, ApiError> {
        let (new_session_id, title, message_count) = self.conversation_store.branch_session(session_id, from_message_id).await?;
        Ok(BranchResponse {
            new_session_id,
            title,
            message_count,
        })
    }
}
