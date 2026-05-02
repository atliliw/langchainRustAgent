//! 对话历史错误定义

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConversationError {
    #[error("SQLite 错误: {0}")]
    SqliteError(String),

    #[error("LLM 调用失败: {0}")]
    LLMError(String),

    #[error("无效操作: {0}")]
    InvalidOperation(String),
}

impl From<sqlx::Error> for ConversationError {
    fn from(e: sqlx::Error) -> Self {
        ConversationError::SqliteError(e.to_string())
    }
}
