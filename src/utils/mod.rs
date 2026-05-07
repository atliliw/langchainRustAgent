//! 工具函数

pub mod document_processor;  // 文档加载和分块
pub mod search_test;         // 检索精准度测试
pub mod chunkers;            // 自定义文本分割器（Token/Semantic）

pub use document_processor::DocumentProcessor;
pub use search_test::SearchTester;
