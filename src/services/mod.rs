//! 业务服务层
//!
//! api_service        API 业务服务（核心编排器，把各模块串起来）
//! langgraph_service  LangGraph 状态图演示
//! aggregate_service  数据采集 Agent 调度

pub mod api_service;          // 核心 API 服务
pub mod langgraph_service;    // LangGraph 状态图演示
pub mod aggregate_service;    // AI 聚合采集
pub mod agent_executor;       // 真实 Agent 执行引擎
pub mod tools;                // Agent 工具系统（可扩展工具注册）
pub mod verify;               // Agent 输出验证钩子
pub mod pageindex;            // PageIndex 无向量 RAG

pub use api_service::ApiService;
pub use langgraph_service::LangGraphDemoService;
pub use aggregate_service::AggregateService;
