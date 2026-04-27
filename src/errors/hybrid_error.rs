use thiserror::Error;

use crate::errors::{BM25Error, StoreError};

#[derive(Error, Debug)]
pub enum HybridError {
    #[error("BM25 搜索失败: {0}")]
    BM25Error(#[from] BM25Error),

    #[error("向量搜索失败: {0}")]
    VectorError(#[from] StoreError),
}
