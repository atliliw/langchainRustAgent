//! 业务服务层
//!
//! api_service        API 业务服务（核心编排器，把各模块串起来）
//! langgraph_service  LangGraph 状态图演示
//! aggregate_service  数据采集 Agent 调度

pub mod api_service;          // 核心 API 服务
pub mod langgraph_service;    // LangGraph 状态图演示
pub mod aggregate_service;    // Agent 数据采集

pub use api_service::ApiService;
pub use langgraph_service::LangGraphDemoService;
pub use aggregate_service::AggregateService;
