//! 配置管理模块
//!
//! 从 config.toml 文件加载配置，支持环境变量覆盖

use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("配置文件不存在: {0}")]
    FileNotFound(String),

    #[error("配置文件解析失败: {0}")]
    ParseError(String),

    #[error("配置项缺失: {0}")]
    MissingField(String),
}

/// 主配置结构
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub openai: OpenAIConfig,
    pub embedding: EmbeddingConfig,
    pub mongodb: MongoConfig,
    pub sqlite: SQLiteConfig,
    pub qdrant: QdrantConfig,
    pub document: DocumentConfig,
    pub search: SearchConfig,
    pub logging: LoggingConfig,
    pub conversation: Option<ConversationConfig>,
}

/// SQLite 对话历史存储配置（参考 OpenCode）
#[derive(Debug, Clone, Deserialize)]
pub struct SQLiteConfig {
    pub db_path: String,
}

impl Default for SQLiteConfig {
    fn default() -> Self {
        Self {
            db_path: "conversations.db".to_string(),
        }
    }
}

/// 对话压缩配置
#[derive(Debug, Clone, Deserialize)]
pub struct ConversationConfig {
    pub max_history_messages: usize,
    pub max_tokens: usize,
    pub keep_first_n_messages: usize,
    pub compress_threshold: usize,
    pub keep_recent_messages: usize,
    pub important_keywords: Vec<String>,
    pub summary_model: String,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            max_history_messages: 50,
            max_tokens: 4000,
            keep_first_n_messages: 2,
            compress_threshold: 15,
            keep_recent_messages: 5,
            important_keywords: vec![
                "我的名字".to_string(),
                "我是".to_string(),
                "记住".to_string(),
                "设定".to_string(),
                "角色".to_string(),
            ],
            summary_model: "gpt-3.5-turbo".to_string(),
        }
    }
}

/// 服务器配置
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub upload_dir: String,
}

/// OpenAI / LLM API 配置
#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub base_url: String,
    pub chat_model: String,
    pub embedding_model: String,
}

/// Embedding API 配置（独立配置，可使用不同 API）
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

/// Qdrant 向量数据库配置
#[derive(Debug, Clone, Deserialize)]
pub struct QdrantConfig {
    pub url: String,
    pub collection_name: String,
    pub vector_size: usize,
    pub distance: String,
}

/// MongoDB BM25 文档存储配置
#[derive(Debug, Clone, Deserialize)]
pub struct MongoConfig {
    pub uri: String,
    pub database: String,
    pub parent_collection: String,
    pub chunk_collection: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DocumentConfig {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub supported_types: Vec<String>,
}

/// 搜索配置
#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub default_top_k: usize,
    pub min_score: f32,
    pub test_sample_count: usize,
}

/// 日志配置
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file_output: bool,
    pub log_file: String,
}

impl Config {
    /// 从文件加载配置
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::FileNotFound(path.display().to_string()));
        }

        let content =
            std::fs::read_to_string(path).map_err(|e| ConfigError::ParseError(e.to_string()))?;

        let config: Config =
            toml::from_str(&content).map_err(|e| ConfigError::ParseError(e.to_string()))?;

        // 验证必要配置
        config.validate()?;

        Ok(config)
    }

    /// 从默认路径加载
    pub fn load() -> Result<Self, ConfigError> {
        // 尝试多个可能的配置文件路径
        let paths = [Path::new("config.toml"), Path::new("demo/config.toml")];

        for path in &paths {
            if path.exists() {
                return Self::from_file(path);
            }
        }

        // 使用环境变量作为后备
        Self::from_env()
    }

    /// 从环境变量加载（后备方案）
    pub fn from_env() -> Result<Self, ConfigError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| ConfigError::MissingField("OPENAI_API_KEY".to_string()))?;

        let qdrant_url =
            std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());

        Ok(Config {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
                upload_dir: "uploads".to_string(),
            },
            openai: OpenAIConfig {
                api_key,
                base_url: std::env::var("OPENAI_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
                chat_model: "gpt-3.5-turbo".to_string(),
                embedding_model: "text-embedding-ada-002".to_string(),
            },
            embedding: EmbeddingConfig {
                api_key: std::env::var("EMBEDDING_API_KEY")
                    .unwrap_or_else(|_| std::env::var("OPENAI_API_KEY").unwrap_or_default()),
                base_url: std::env::var("EMBEDDING_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
                model: std::env::var("EMBEDDING_MODEL")
                    .unwrap_or_else(|_| "text-embedding-3-small".to_string()),
            },
            mongodb: MongoConfig {
                uri: std::env::var("MONGODB_URI")
                    .unwrap_or_else(|_| "mongodb://localhost:27017".to_string()),
                database: "langchainrust_demo".to_string(),
                parent_collection: "bm25_parents".to_string(),
                chunk_collection: "bm25_chunks".to_string(),
            },
            sqlite: SQLiteConfig {
                db_path: std::env::var("SQLITE_DB_PATH")
                    .unwrap_or_else(|_| "conversations.db".to_string()),
            },
            qdrant: QdrantConfig {
                url: qdrant_url,
                collection_name: "demo_documents".to_string(),
                vector_size: 1536,
                distance: "Cosine".to_string(),
            },
            document: DocumentConfig {
                chunk_size: 500,
                chunk_overlap: 50,
                supported_types: vec![
                    "txt".to_string(),
                    "pdf".to_string(),
                    "md".to_string(),
                    "json".to_string(),
                    "csv".to_string(),
                ],
            },
            search: SearchConfig {
                default_top_k: 5,
                min_score: 0.5,
                test_sample_count: 10,
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                file_output: false,
                log_file: "logs/demo.log".to_string(),
            },
            conversation: None,
        })
    }

    /// 验证配置有效性
    fn validate(&self) -> Result<(), ConfigError> {
        if self.openai.api_key.is_empty() || self.openai.api_key.contains("your-api-key") {
            return Err(ConfigError::MissingField(
                "请在 config.toml 中配置有效的 OpenAI API Key".to_string(),
            ));
        }

        if self.qdrant.url.is_empty() {
            return Err(ConfigError::MissingField("Qdrant URL".to_string()));
        }

        // 验证向量维度
        let valid_dimensions = [128, 256, 512, 768, 1024, 1536, 3072];
        if !valid_dimensions.contains(&self.qdrant.vector_size) {
            return Err(ConfigError::ParseError(format!(
                "向量维度 {} 不是常用值，常用值: {:?}",
                self.qdrant.vector_size, valid_dimensions
            )));
        }

        Ok(())
    }

    /// 获取服务器地址
    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }

    /// 转换为 langchainrust OpenAI 配置
    pub fn to_langchain_openai_config(&self) -> langchainrust::OpenAIConfig {
        langchainrust::OpenAIConfig {
            api_key: self.openai.api_key.clone(),
            base_url: self.openai.base_url.clone(),
            model: self.openai.chat_model.clone(),
            streaming: false,
            temperature: Some(0.7),
            max_tokens: Some(500),
            ..Default::default()
        }
    }

    /// 转换为 langchainrust Embeddings 配置（使用独立 embedding API）
    pub fn to_langchain_embeddings_config(&self) -> langchainrust::OpenAIEmbeddingsConfig {
        langchainrust::OpenAIEmbeddingsConfig {
            api_key: self.embedding.api_key.clone(),
            base_url: self.embedding.base_url.clone(),
            model: self.embedding.model.clone(),
            batch_size: 100,
        }
    }

    /// 转换为 langchainrust Qdrant 配置
    pub fn to_langchain_qdrant_config(&self) -> langchainrust::QdrantConfig {
        use langchainrust::vector_stores::QdrantDistance;

        let distance = match self.qdrant.distance.as_str() {
            "Cosine" => QdrantDistance::Cosine,
            "Euclid" => QdrantDistance::Euclid,
            "Dot" => QdrantDistance::Dot,
            _ => QdrantDistance::Cosine,
        };

        langchainrust::QdrantConfig::new(&self.qdrant.url, &self.qdrant.collection_name)
            .with_vector_size(self.qdrant.vector_size)
            .with_distance(distance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
                upload_dir: "uploads".to_string(),
            },
            openai: OpenAIConfig {
                api_key: "test-key".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                chat_model: "gpt-3.5-turbo".to_string(),
                embedding_model: "text-embedding-ada-002".to_string(),
            },
            embedding: EmbeddingConfig {
                api_key: "test-embedding-key".to_string(),
                base_url: "https://api.openai-proxy.org/v1".to_string(),
                model: "text-embedding-3-small".to_string(),
            },
            mongodb: MongoConfig {
                uri: "mongodb://localhost:27017".to_string(),
                database: "test".to_string(),
                parent_collection: "bm25_parents".to_string(),
                chunk_collection: "bm25_chunks".to_string(),
            },
            sqlite: SQLiteConfig {
                db_path: "test.db".to_string(),
            },
            qdrant: QdrantConfig {
                url: "http://localhost:6334".to_string(),
                collection_name: "test".to_string(),
                vector_size: 1536,
                distance: "Cosine".to_string(),
            },
            document: DocumentConfig {
                chunk_size: 500,
                chunk_overlap: 50,
                supported_types: vec!["txt".to_string()],
            },
            search: SearchConfig {
                default_top_k: 5,
                min_score: 0.5,
                test_sample_count: 10,
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                file_output: false,
                log_file: "test.log".to_string(),
            },
            conversation: None,
        };

        assert_eq!(config.server_addr(), "0.0.0.0:8080");
        assert!(config.validate().is_ok());
    }
}
