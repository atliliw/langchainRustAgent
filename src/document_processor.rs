//! ============================================================================
//! 文档处理模块 - 文件加载和文本分割
//! ============================================================================
//!
//! 功能说明：
//! 1. 加载不同格式的文件（TXT, PDF, MD, JSON, CSV）
//! 2. 将长文本分割成小块（便于向量化）
//! 3. 保留文档元数据（来源文件名等）
//!
//! 文本分割策略：
//! - chunk_size: 每块最大字符数（配置文件中设置，默认 500）
//! - chunk_overlap: 块之间的重叠字符数（默认 50）
//!
//! 为什么需要分割？
//! - OpenAI Embeddings API 有 token 限制
//! - 小块便于精确搜索匹配
//! - 提高搜索结果的相关性

use crate::config::Config;
use langchainrust::{
    Document, TextSplitter, RecursiveCharacterSplitter,
    PDFLoader, TextLoader, JSONLoader, MarkdownLoader, CSVLoader,
    DocumentLoader,
};
use std::path::Path;
use thiserror::Error;

// ============================================================================
// 错误类型定义
// ============================================================================

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("文件不存在: {0}")]
    FileNotFound(String),
    
    #[error("不支持的文件类型: {0}")]
    UnsupportedType(String),
    
    #[error("文档加载失败: {0}")]
    LoadError(String),
    
    #[error("文本分割失败: {0}")]
    SplitError(String),
}

// ============================================================================
// 文档处理器结构体
// ============================================================================

pub struct DocumentProcessor {
    config: Config,
}

impl DocumentProcessor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    pub async fn process_file(&self, path: &Path) -> Result<(Vec<Document>, Vec<Document>), ProcessError> {
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
        let chunks = self.split_documents(&original_docs)?;
        
        Ok((original_docs, chunks))
    }
    
    async fn load_file(&self, path: &Path, extension: &str) -> Result<Vec<Document>, ProcessError> {
        let documents = match extension {
            "txt" => {
                TextLoader::new(path).load().await
                    .map_err(|e| ProcessError::LoadError(e.to_string()))?
            }
            "pdf" => {
                PDFLoader::new(path).load().await
                    .map_err(|e| ProcessError::LoadError(e.to_string()))?
            }
            "json" => {
                JSONLoader::new(path).load().await
                    .map_err(|e| ProcessError::LoadError(e.to_string()))?
            }
            "md" => {
                MarkdownLoader::new(path).load().await
                    .map_err(|e| ProcessError::LoadError(e.to_string()))?
            }
            "csv" => {
                CSVLoader::new(path, "content").load().await
                    .map_err(|e| ProcessError::LoadError(e.to_string()))?
            }
            _ => {
                return Err(ProcessError::UnsupportedType(extension.to_string()));
            }
        };
        
        Ok(documents)
    }
    
    fn split_documents(&self, documents: &[Document]) -> Result<Vec<Document>, ProcessError> {
        let splitter = RecursiveCharacterSplitter::new(
            self.config.document.chunk_size,
            self.config.document.chunk_overlap,
        );
        
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
    
    pub fn is_supported(&self, extension: &str) -> bool {
        self.config.document.supported_types.contains(&extension.to_lowercase())
    }
    
    pub fn supported_types(&self) -> &[String] {
        &self.config.document.supported_types
    }
}