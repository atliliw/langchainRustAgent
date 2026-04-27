use thiserror::Error;

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
