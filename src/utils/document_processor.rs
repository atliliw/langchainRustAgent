//! 文档处理模块 — 文件加载和文本分割
//!
//! 支持 5 种文件格式：txt, pdf, md, json, csv
//! 分块策略：支持 Recursive / Large / Small / Paragraph
//! 分块后同时存入 Qdrant 向量库 + MongoDB BM25 索引

use crate::config::Config;
use crate::errors::ProcessError;
use crate::models::ChunkStrategy;
use crate::utils::chunkers::{TokenTextSplitter, SemanticChunker};
use langchainrust::{
    Document, TextSplitter, RecursiveCharacterSplitter, OpenAIEmbeddings,
    PDFLoader, TextLoader, JSONLoader, MarkdownLoader, CSVLoader,
    DocumentLoader,
};
use std::path::Path;
use std::sync::Arc;

/// 文档处理器：把上传的文件转成 LLM 可用的文档块
pub struct DocumentProcessor {
    config: Config,
    embeddings: Option<Arc<OpenAIEmbeddings>>,
}

impl DocumentProcessor {
    pub fn new(config: Config) -> Self {
        Self { config, embeddings: None }
    }

    pub fn with_embeddings(config: Config, embeddings: Arc<OpenAIEmbeddings>) -> Self {
        Self { config, embeddings: Some(embeddings) }
    }
    
    /// 处理文件：加载 + 分块 + 返回 (原始文档, 分块文档)
    pub async fn process_file(&self, path: &Path) -> Result<(Vec<Document>, Vec<Document>), ProcessError> {
        self.process_file_with_strategy(path, &ChunkStrategy::default()).await
    }
    
    /// 按策略处理文件
    pub async fn process_file_with_strategy(
        &self, path: &Path, strategy: &ChunkStrategy,
    ) -> Result<(Vec<Document>, Vec<Document>), ProcessError> {
        if !path.exists() {
            return Err(ProcessError::FileNotFound(path.display().to_string()));
        }
        
        let extension = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        if !self.config.document.supported_types.contains(&extension) {
            return Err(ProcessError::UnsupportedType(extension));
        }
        
        let original_docs = self.load_file(path, &extension).await?;
        let chunks = self.split_documents_with_strategy(&original_docs, strategy).await?;
        
        Ok((original_docs, chunks))
    }
    
    /// 按文件类型选择对应的加载器
    async fn load_file(&self, path: &Path, extension: &str) -> Result<Vec<Document>, ProcessError> {
        let documents = match extension {
            "txt" => TextLoader::new(path).load().await.map_err(|e| ProcessError::LoadError(e.to_string()))?,
            "pdf" => PDFLoader::new(path).load().await.map_err(|e| ProcessError::LoadError(e.to_string()))?,
            "json" => JSONLoader::new(path).load().await.map_err(|e| ProcessError::LoadError(e.to_string()))?,
            "md" => MarkdownLoader::new(path).load().await.map_err(|e| ProcessError::LoadError(e.to_string()))?,
            "csv" => CSVLoader::new(path, "content").load().await.map_err(|e| ProcessError::LoadError(e.to_string()))?,
            _ => return Err(ProcessError::UnsupportedType(extension.to_string())),
        };
        Ok(documents)
    }
    
    /// 按策略创建分割器并分块
    async fn split_documents_with_strategy(&self, documents: &[Document], strategy: &ChunkStrategy) -> Result<Vec<Document>, ProcessError> {
        match strategy {
            ChunkStrategy::Token => {
                // Token 模式：512 tokens/chunk, 50 tokens overlap
                let splitter = TokenTextSplitter::new(512, 50);
                let all_chunks: Vec<Document> = documents.iter()
                    .flat_map(|doc| {
                        let chunks = splitter.split_document(doc);
                        chunks.into_iter().map(|chunk| {
                            chunk.with_metadata("source_file", doc.metadata.get("source")
                                .unwrap_or(&"".to_string()).clone())
                        })
                    })
                    .collect();
                Ok(all_chunks)
            }
            ChunkStrategy::Semantic => {
                match &self.embeddings {
                    Some(emb) => {
                        let chunker = SemanticChunker::new(emb.clone(), 200, 2000);
                        let mut all_chunks = Vec::new();
                        for doc in documents {
                            match chunker.split_document_semantic(doc).await {
                                Ok(chunks) => all_chunks.extend(chunks),
                                Err(_) => {
                                    // 失败则降级
                                    let splitter = RecursiveCharacterSplitter::new(500, 50);
                                    let fallback = splitter.split_document(doc);
                                    all_chunks.extend(fallback);
                                }
                            }
                        }
                        Ok(all_chunks)
                    }
                    None => {
                        tracing::warn!("Semantic chunking: no Embedding API available, falling back to Recursive");
                        let splitter = RecursiveCharacterSplitter::new(500, 50);
                        let all_chunks: Vec<Document> = documents.iter()
                            .flat_map(|doc| {
                                let chunks = splitter.split_document(doc);
                                chunks.into_iter().map(|chunk| {
                                    chunk.with_metadata("source_file", doc.metadata.get("source")
                                        .unwrap_or(&"".to_string()).clone())
                                })
                            })
                            .collect();
                        Ok(all_chunks)
                    }
                }
            }
            _ => {
                let splitter = match strategy {
                    ChunkStrategy::Recursive => RecursiveCharacterSplitter::new(500, 50),
                    ChunkStrategy::Large => RecursiveCharacterSplitter::new(1000, 100),
                    ChunkStrategy::Small => RecursiveCharacterSplitter::new(200, 30),
                    ChunkStrategy::Paragraph => {
                        RecursiveCharacterSplitter::new(1500, 0)
                            .with_separators(vec!["\n\n".to_string()])
                    }
                    _ => RecursiveCharacterSplitter::new(500, 50),
                };
                
                let all_chunks: Vec<Document> = documents.iter()
                    .flat_map(|doc| {
                        let chunks = splitter.split_document(doc);
                        chunks.into_iter().map(|chunk| {
                            chunk.with_metadata("source_file", doc.metadata.get("source")
                                .unwrap_or(&"".to_string()).clone())
                        })
                    })
                    .collect();
                Ok(all_chunks)
            }
        }
    }
    
    pub fn is_supported(&self, extension: &str) -> bool {
        self.config.document.supported_types.contains(&extension.to_lowercase())
    }
    
    pub fn supported_types(&self) -> &[String] {
        &self.config.document.supported_types
    }
}
