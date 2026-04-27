use thiserror::Error;

#[derive(Error, Debug)]
pub enum TestError {
    #[error("测试数据初始化失败: {0}")]
    InitError(String),

    #[error("搜索测试失败: {0}")]
    SearchError(String),
}
