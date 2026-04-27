//! 文档处理模块 - 文件加载和文本分割

use crate::config::Config;
use crate::errors::ProcessError;
use langchainrust::{
    Document, TextSplitter, RecursiveCharacterSplitter,
    PDFLoader, TextLoader, JSONLoader, MarkdownLoader, CSVLoader,
    DocumentLoader,
};
use std::path::Path;

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