//! 错误类型定义
//!
//! 每个模块有自己的 Error 类型，用 thiserror 宏自动实现 Display trait

pub mod agent_error;          // Agent 采集错误
pub mod api_error;            // API 业务错误
pub mod bm25_error;           // BM25 存储错误
pub mod conversation_error;   // 对话历史错误
pub mod graph_error;          // LangGraph 错误
pub mod hybrid_error;         // 混合检索错误
pub mod process_error;        // 文档处理错误
pub mod store_error;          // 向量库错误
pub mod test_error;           // 测试错误

pub use agent_error::AgentError;
pub use api_error::ApiError;
pub use bm25_error::BM25Error;
pub use conversation_error::ConversationError;
pub use graph_error::GraphDemoError;
pub use hybrid_error::HybridError;
pub use process_error::ProcessError;
pub use store_error::StoreError;
pub use test_error::TestError;
