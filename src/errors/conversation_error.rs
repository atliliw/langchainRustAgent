use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConversationError {
    #[error("SQLite 错误: {0}")]
    SqliteError(String),

    #[error("LLM 调用失败: {0}")]
    LLMError(String),
}
