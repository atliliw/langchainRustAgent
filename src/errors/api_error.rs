use thiserror::Error;

use crate::errors::{BM25Error, ConversationError, HybridError, ProcessError, StoreError};

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("文件上传失败: {0}")]
    UploadError(String),

    #[error("搜索失败: {0}")]
    SearchError(String),

    #[error("向量存储失败: {0}")]
    VectorError(#[from] StoreError),

    #[error("BM25 存储失败: {0}")]
    BM25Error(#[from] BM25Error),

    #[error("混合检索失败: {0}")]
    HybridError(#[from] HybridError),

    #[error("处理失败: {0}")]
    ProcessError(#[from] ProcessError),

    #[error("对话失败: {0}")]
    ConversationError(#[from] ConversationError),
}
