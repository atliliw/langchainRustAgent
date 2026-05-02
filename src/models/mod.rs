//! 数据模型定义
//!
//! 定义了所有 API 请求/响应的数据结构。
//! 每个 struct 对应一个 JSON 格式，用 serde 做序列化/反序列化。
//!
//! chat.rs      对话相关（发消息、历史、会话列表、压缩模式等）
//! search.rs    搜索相关（请求、响应、对比测试）
//! document.rs  文档管理（BM25结果、删除、标签等）
//! langgraph.rs LangGraph 状态图演示（并行/条件/流式）
//! aggregate.rs Agent 数据采集（采集内容、请求、统计）
//! upload.rs    上传响应
//! stats.rs     统计信息
//! test_case.rs 精准度测试用例

pub mod chat;         // 对话
pub mod search;       // 搜索
pub mod document;     // 文档管理
pub mod langgraph;    // LangGraph 演示
pub mod aggregate;    // Agent 数据采集
pub mod upload;       // 上传
pub mod stats;        // 统计
pub mod test_case;    // 测试用例

// 常用类型对外暴露，外部只需要 use models::* 就能访问
pub use chat::*;
pub use search::*;
pub use document::*;
pub use langgraph::*;
pub use aggregate::*;
pub use upload::*;
pub use stats::*;
pub use test_case::*;
