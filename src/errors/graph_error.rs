use thiserror::Error;

#[derive(Error, Debug)]
pub enum GraphDemoError {
    #[error("图构建失败: {0}")]
    BuildError(String),

    #[error("执行失败: {0}")]
    ExecutionError(String),
}
