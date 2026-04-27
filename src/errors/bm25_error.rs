use thiserror::Error;

#[derive(Error, Debug)]
pub enum BM25Error {
    #[error("BM25 初始化失败: {0}")]
    InitError(String),

    #[error("MongoDB 连接失败: {0}")]
    MongoError(String),

    #[error("MongoDB 操作失败: {0}")]
    OperationError(String),
}
