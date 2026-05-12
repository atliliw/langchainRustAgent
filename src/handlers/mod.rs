//! HTTP 处理函数
//! 
//! 每个文件对应一类 API 的处理逻辑：
//!   upload.rs      文件上传处理
//!   search.rs      搜索（向量/BM25/混合/对比）
//!   chat.rs        对话（普通/流式/历史/会话管理）
//!   document.rs    文档管理（列表/删除/标签）
//!   langgraph.rs   LangGraph 状态图演示
//!   test.rs        检索精准度测试
//!   aggregate.rs   数据采集 Agent
//!   stats.rs       API 调用统计监控
//!   error.rs       统一错误响应格式

pub mod upload;     // 文件上传
pub mod search;     // 搜索
pub mod chat;       // 对话
pub mod document;   // 文档管理
pub mod langgraph;  // LangGraph 演示
pub mod test;       // 测试
pub mod error;      // 错误定义
pub mod aggregate;  // Agent 数据采集
pub mod stats;      // API 统计
pub mod playground; // v2 Playground API

// 对外暴露：统一错误响应、全局状态类型
pub use error::ApiErrorResponse;
pub use upload::AppState;
