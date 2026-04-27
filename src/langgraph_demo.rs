//! LangGraph 多任务并行演示
//!
//! 展示 LangGraph 的核心能力：
//! 1. 并行执行 (FanOut → 并行节点 → FanIn)
//! 2. 条件路由 (根据状态选择路径)
//! 3. 流式执行 (返回执行事件)

use langchainrust::{
    StateGraph, GraphBuilder, START, END,
    AgentState, StateUpdate,
    FunctionRouter,
    StreamEvent,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::time::sleep;

#[derive(Error, Debug)]
pub enum GraphDemoError {
    #[error("图构建失败: {0}")]
    BuildError(String),
    
    #[error("执行失败: {0}")]
    ExecutionError(String),
}

/// 并行执行演示结果
#[derive(Debug, Serialize, Deserialize)]
pub struct ParallelDemoResult {
    pub input: String,
    pub parallel_tasks: Vec<ParallelTaskResult>,
    pub merged_result: String,
    pub total_time_ms: u64,
    pub sequential_time_estimate_ms: u64,
    pub time_saved_percent: f32,
}

/// 单个并行任务结果
#[derive(Debug, Serialize, Deserialize)]
pub struct ParallelTaskResult {
    pub task_name: String,
    pub result: String,
    pub duration_ms: u64,
}

/// 条件路由演示结果
#[derive(Debug, Serialize, Deserialize)]
pub struct ConditionalDemoResult {
    pub input: String,
    pub route_decision: String,
    pub path_taken: String,
    pub output: String,
    pub steps: Vec<String>,
}

/// 流式执行事件
#[derive(Debug, Serialize, Deserialize)]
pub struct StreamDemoEvent {
    pub node_name: String,
    pub event_type: String,
    pub timestamp_ms: u64,
    pub state_snapshot: Option<StateSnapshot>,
}

/// 状态快照
#[derive(Debug, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub input: String,
    pub output: Option<String>,
    pub messages: Vec<String>,
}

/// LangGraph 演示服务
pub struct LangGraphDemoService {
}

impl LangGraphDemoService {
    pub fn new() -> Self {
        Self {}
    }
    
    /// 并行执行演示
    /// 
    /// 场景：同时执行多个独立任务，然后合并结果
    /// - Task A: 模拟数据获取（100ms）
    /// - Task B: 模拟文档处理（150ms）
    /// - Task C: 模拟分析计算（200ms）
    /// 
    /// 展示 FanOut → 并行 → FanIn 模式
    pub async fn run_parallel_demo(&self, input: String) -> Result<ParallelDemoResult, GraphDemoError> {
        let start_time = Instant::now();
        
        let mut graph: StateGraph<AgentState> = StateGraph::new();
        
        graph.add_node_fn("dispatcher", |state| {
            let mut new_state = state.clone();
            new_state.add_message(langchainrust::MessageEntry::ai("分发并行任务".to_string()));
            Ok(StateUpdate::full(new_state))
        });
        
        graph.add_async_node("task_a", |state: &AgentState| {
            let state = state.clone();
            async move {
                sleep(Duration::from_millis(100)).await;
                let mut new_state = state;
                new_state.add_message(langchainrust::MessageEntry::ai("TaskA: 数据已获取".to_string()));
                Ok(StateUpdate::full(new_state))
            }
        });
        
        graph.add_async_node("task_b", |state: &AgentState| {
            let state = state.clone();
            async move {
                sleep(Duration::from_millis(150)).await;
                let mut new_state = state;
                new_state.add_message(langchainrust::MessageEntry::ai("TaskB: 文档已处理".to_string()));
                Ok(StateUpdate::full(new_state))
            }
        });
        
        graph.add_async_node("task_c", |state: &AgentState| {
            let state = state.clone();
            async move {
                sleep(Duration::from_millis(200)).await;
                let mut new_state = state;
                new_state.add_message(langchainrust::MessageEntry::ai("TaskC: 分析已完成".to_string()));
                Ok(StateUpdate::full(new_state))
            }
        });
        
        graph.add_edge(START, "dispatcher");
        graph.add_fan_out("dispatcher", vec![
            "task_a".to_string(),
            "task_b".to_string(),
            "task_c".to_string(),
        ]);
        graph.add_edge("task_a", END);
        graph.add_edge("task_b", END);
        graph.add_edge("task_c", END);
        
        let compiled = graph.compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;
        
        let initial_state = AgentState::new(input.clone());
        let result = compiled.invoke(initial_state).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        
        let total_time = start_time.elapsed();
        
        let sequential_estimate = 100 + 150 + 200;
        let time_saved = (sequential_estimate as f32 - total_time.as_millis() as f32) / sequential_estimate as f32 * 100.0;
        
        let messages: Vec<ParallelTaskResult> = result.final_state.messages.iter()
            .skip(1)
            .map(|m| ParallelTaskResult {
                task_name: m.content.split(':').next().unwrap_or("unknown").to_string(),
                result: m.content.clone(),
                duration_ms: match m.content.split(':').next().unwrap_or("") {
                    "TaskA" => 100,
                    "TaskB" => 150,
                    "TaskC" => 200,
                    _ => 0,
                },
            })
            .collect();
        
        Ok(ParallelDemoResult {
            input,
            parallel_tasks: messages,
            merged_result: format!("并行执行完成，耗时 {}ms", total_time.as_millis()),
            total_time_ms: total_time.as_millis() as u64,
            sequential_time_estimate_ms: sequential_estimate,
            time_saved_percent: time_saved.max(0.0),
        })
    }
    
    /// 条件路由演示
    /// 
    /// 场景：根据输入长度选择不同处理路径
    /// - 短路径 (< 10 chars): 快速处理
    /// - 长路径 (>= 10 chars): 详细处理
    /// 
    /// 展示条件边和 FunctionRouter
    pub async fn run_conditional_demo(&self, input: String) -> Result<ConditionalDemoResult, GraphDemoError> {
        let mut graph: StateGraph<AgentState> = StateGraph::new();
        
        graph.add_node_fn("analyze", |state| {
            let mut new_state = state.clone();
            new_state.add_message(langchainrust::MessageEntry::ai(
                format!("分析输入: 长度={}", state.input.len())
            ));
            Ok(StateUpdate::full(new_state))
        });
        
        graph.add_node_fn("quick_process", |state| {
            let mut new_state = state.clone();
            new_state.add_message(langchainrust::MessageEntry::ai("快速处理完成".to_string()));
            new_state.set_output(format!("快速结果: {}", state.input));
            Ok(StateUpdate::full(new_state))
        });
        
        graph.add_node_fn("detailed_process", |state| {
            let mut new_state = state.clone();
            new_state.add_message(langchainrust::MessageEntry::ai("详细处理完成".to_string()));
            new_state.set_output(format!("详细结果: {} (长度: {})", state.input, state.input.len()));
            Ok(StateUpdate::full(new_state))
        });
        
        graph.add_edge(START, "analyze");
        
        let targets = HashMap::from([
            ("short".to_string(), "quick_process".to_string()),
            ("long".to_string(), "detailed_process".to_string()),
        ]);
        graph.add_conditional_edges("analyze", "length_router", targets, None);
        
        // 路由器
        let router = FunctionRouter::new(|state: &AgentState| {
            if state.input.len() < 10 { "short" } else { "long" }.to_string()
        });
graph.set_conditional_router("length_router", router);
        
        graph.add_edge("quick_process", END);
        graph.add_edge("detailed_process", END);
        
        let compiled = graph.compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;
        
        let initial_state = AgentState::new(input.clone());
        let result = compiled.invoke(initial_state).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        
        let route_decision = if input.len() < 10 { "short" } else { "long" };
        let path_taken = if input.len() < 10 { "quick_process" } else { "detailed_process" };
        
        let steps: Vec<String> = result.final_state.messages.iter()
            .map(|m| m.content.clone())
            .collect();
        
        Ok(ConditionalDemoResult {
            input,
            route_decision: route_decision.to_string(),
            path_taken: path_taken.to_string(),
            output: result.final_state.output.unwrap_or_default(),
            steps,
        })
    }
    
    /// 流式执行演示
    /// 
    /// 场景：展示执行过程中的事件流
    /// - 每个节点执行产生事件
    /// - 状态变化可见
    /// 
    /// 展示 stream() API
    pub async fn run_stream_demo(&self, input: String) -> Result<Vec<StreamDemoEvent>, GraphDemoError> {
        let compiled = GraphBuilder::<AgentState>::new()
            .add_node_fn("step1", |state| {
                let mut new_state = state.clone();
                new_state.add_message(langchainrust::MessageEntry::ai("步骤1完成".to_string()));
                Ok(StateUpdate::full(new_state))
            })
            .add_node_fn("step2", |state| {
                let mut new_state = state.clone();
                new_state.add_message(langchainrust::MessageEntry::ai("步骤2完成".to_string()));
                Ok(StateUpdate::full(new_state))
            })
            .add_node_fn("step3", |state| {
                let mut new_state = state.clone();
                new_state.add_message(langchainrust::MessageEntry::ai("步骤3完成".to_string()));
                new_state.set_output("流程完成".to_string());
                Ok(StateUpdate::full(new_state))
            })
            .add_edge(START, "step1")
            .add_edge("step1", "step2")
            .add_edge("step2", "step3")
            .add_edge("step3", END)
            .compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;
        
        let start_time = Instant::now();
        let events = compiled.stream(AgentState::new(input.clone())).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        
        let stream_events: Vec<StreamDemoEvent> = events.iter()
            .map(|e| StreamDemoEvent {
                node_name: match e {
                    StreamEvent::Start(_) => "START".to_string(),
                    StreamEvent::EnterNode(name, _) => name.clone(),
                    StreamEvent::NodeComplete(name, _) => name.clone(),
                    StreamEvent::StateUpdate(_) => "update".to_string(),
                    StreamEvent::End(_) => "END".to_string(),
                },
                event_type: match e {
                    StreamEvent::Start(_) => "graph_start".to_string(),
                    StreamEvent::EnterNode(_, _) => "enter".to_string(),
                    StreamEvent::NodeComplete(_, _) => "complete".to_string(),
                    StreamEvent::StateUpdate(_) => "state_update".to_string(),
                    StreamEvent::End(_) => "graph_end".to_string(),
                },
                timestamp_ms: start_time.elapsed().as_millis() as u64,
                state_snapshot: match e {
                    StreamEvent::End(state) => Some(StateSnapshot {
                        input: state.input.clone(),
                        output: state.output.clone(),
                        messages: state.messages.iter().map(|m| m.content.clone()).collect(),
                    }),
                    StreamEvent::EnterNode(_, state) => Some(StateSnapshot {
                        input: state.input.clone(),
                        output: state.output.clone(),
                        messages: state.messages.iter().map(|m| m.content.clone()).collect(),
                    }),
                    _ => None,
                },
            })
            .collect();
        
        Ok(stream_events)
    }
    
    /// 获取演示图结构信息
    pub fn get_graph_info(&self) -> serde_json::Value {
        serde_json::json!({
            "parallel_demo": {
                "name": "并行执行演示",
                "description": "FanOut → 3个并行任务 → FanIn",
                "nodes": ["dispatcher", "task_a", "task_b", "task_c", "merger"],
                "edges": [
                    "START → dispatcher",
                    "dispatcher → [task_a, task_b, task_c] (FanOut)",
                    "task_a → END",
                    "task_b → END",
                    "task_c → END"
                ],
                "features": ["add_fan_out", "add_async_node"]
            },
            "conditional_demo": {
                "name": "条件路由演示",
                "description": "根据输入长度选择路径",
                "nodes": ["analyze", "quick_process", "detailed_process"],
                "edges": [
                    "START → analyze",
                    "analyze → quick_process (条件: 长度<10)",
                    "analyze → detailed_process (条件: 长度>=10)",
                    "quick_process → END",
                    "detailed_process → END"
                ],
                "features": ["add_conditional_edges", "FunctionRouter"]
            },
            "stream_demo": {
                "name": "流式执行演示",
                "description": "展示执行事件流",
                "nodes": ["step1", "step2", "step3"],
                "edges": ["START → step1 → step2 → step3 → END"],
                "features": ["stream()", "StreamEvent"]
            }
        })
    }
}

impl Default for LangGraphDemoService {
    fn default() -> Self {
        Self::new()
    }
}