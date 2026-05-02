//! LangChainRust Agent - RAG 对话系统 + AI信息聚合
//!
//! 模块结构（按执行顺序阅读）：
//! 
//! 配置文件层
//! └── config      读取 config.toml，提供所有模块的配置项
//! 
//! 数据模型层（查数据长什么样 → 看这里）
//! └── models      定义 ChatRequest、SearchResult 等数据结构
//! 
//! 错误定义层（报错文本 → 看这里）
//! └── errors      各种错误类型（ApiError、ConversationError 等）
//! 
//! 存储层（怎么存数据 → 看这里）
//! ├── stores/qdrant          向量存储（Qdrant 数据库）
//! ├── stores/bm25            BM25 关键词检索（MongoDB 持久化）
//! ├── stores/hybrid          混合检索（RRF 融合算法）
//! ├── stores/conversation    对话历史（SQLite + 压缩策略）
//! └── stores/content_store   聚合内容存储（Agent采集数据）
//! 
//! 业务逻辑层（核心逻辑 → 看这里）
//! ├── services/api_service         API 业务服务（核心编排）
//! ├── services/langgraph_service   LangGraph 状态图演示
//! └── services/aggregate_service   数据采集 Agent 调度
//! 
//! HTTP 处理层（API 请求怎么处理 → 看这里）
//! ├── routes         路由定义（哪个 URL 对应哪个处理函数）
//! └── handlers       HTTP 处理函数（收请求→调服务→返回响应）
//! 
//! Agent 层（采集外部数据 → 看这里）
//! └── agents         GitHub/HackerNews/RSS/ArXiv 数据采集工具
//! 
//! 工具层
//! └── utils          文档处理（加载、分块）、搜索测试

pub mod config;     // 配置管理
pub mod errors;     // 错误定义
pub mod models;     // 数据结构
pub mod stores;     // 存储层（Qdrant/MongoDB/SQLite）
pub mod services;   // 业务逻辑
pub mod handlers;   // HTTP 处理函数
pub mod routes;     // 路由定义
pub mod agents;     // 数据采集 Agent
pub mod utils;      // 工具函数