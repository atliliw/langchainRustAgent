use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Qdrant 连接失败: {0}")]
    ConnectionError(String),

    #[error("文档添加失败: {0}")]
    AddError(String),

    #[error("搜索失败: {0}")]
    SearchError(String),

    #[error("向量生成失败: {0}")]
    EmbeddingError(String),

    #[error("文档不存在: {0}")]
    NotFound(String),

    #[error("建表失败: {0}")]
    CreateTableError(String),
}
