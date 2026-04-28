//! LangChainRust Agent - RAG 对话系统
//!
//! 模块结构：
//! - config: 配置管理
//! - errors: 错误定义
//! - models: 数据结构
//! - stores: 存储层（Qdrant/MongoDB/SQLite）
//! - services: 业务逻辑
//! - handlers: HTTP 处理函数
//! - routes: 路由定义
//! - utils: 工具函数

pub mod config;
pub mod errors;
pub mod models;
pub mod stores;
pub mod services;
pub mod handlers;
pub mod routes;
pub mod utils;