//! Agent 采集错误定义

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("网络请求失败: {0}")]
    NetworkError(String),

    #[error("API错误: {0}")]
    ApiError(String),

    #[error("数据解析失败: {0}")]
    ParseError(String),

    #[error("LLM处理失败: {0}")]
    LLMError(String),
}
