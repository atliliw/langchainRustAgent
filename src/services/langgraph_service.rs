//! LangGraph 多任务并行演示服务
//!
//! LangGraph = 有向图执行引擎，用来编排多个 Agent 或任务的执行流程。
//!
//! 核心概念：
//!   Node（节点）：一个执行单元（比如"获取数据"、"分析文档"）
//!   Edge（边）：节点间的连接（哪个在前、哪个在后）
//!   State（状态）：在节点间传递的数据
//!   Router（路由）：根据状态决定走哪条边
//!
//! 三种演示模式：
//!   1. 并行执行（FanOut）：同时跑多个任务，总耗时 = 最慢那个
//!   2. 条件路由：根据输入状态动态选择执行路径
//!   3. 流式执行：实时推送每个节点的执行进度

use crate::errors::GraphDemoError;
use crate::models::*;
use langchainrust::{
    StateGraph, GraphBuilder, START, END,
    AgentState, StateUpdate,
    FunctionRouter,
    StreamEvent,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::time::sleep;

pub struct LangGraphDemoService {}

impl LangGraphDemoService {
    pub fn new() -> Self {
        Self {}
    }
    
    /// ──────────────────── 模式1：并行执行演示 ────────────────────
    ///
    /// 演示：1个分发器 → 3个并行任务 → 完成
    ///
    /// 图结构：
    ///              ┌─→ TaskA (100ms) ─┐
    /// START → 分发器 ┼─→ TaskB (150ms) ─┼→ END
    ///              └─→ TaskC (200ms) ─┘
    ///
    /// 关键点：3个任务同时执行，总耗时 ≈ 200ms（最长的）
    /// 如果是串行，耗时 = 100+150+200 = 450ms
    /// 所以并行节省了约 55% 的时间
    pub async fn run_parallel_demo(&self, input: String) -> Result<ParallelDemoResult, GraphDemoError> {
        let start_time = Instant::now();
        
        // 创建一个 StateGraph
        let mut graph: StateGraph<AgentState> = StateGraph::new();
        
        // 添加节点：分发器（调度任务）
        graph.add_node_fn("dispatcher", |state| {
            let mut new_state = state.clone();
            new_state.add_message(langchainrust::MessageEntry::ai("分发并行任务".to_string()));
            Ok(StateUpdate::full(new_state))
        });
        
        // 添加节点：任务A（模拟耗时100ms）
        graph.add_async_node("task_a", |state: &AgentState| {
            let state = state.clone();
            async move {
                sleep(Duration::from_millis(100)).await;  // 模拟耗时操作
                let mut new_state = state;
                new_state.add_message(langchainrust::MessageEntry::ai("TaskA: 数据已获取".to_string()));
                Ok(StateUpdate::full(new_state))
            }
        });
        
        // 添加节点：任务B（模拟耗时150ms）
        graph.add_async_node("task_b", |state: &AgentState| {
            let state = state.clone();
            async move {
                sleep(Duration::from_millis(150)).await;
                let mut new_state = state;
                new_state.add_message(langchainrust::MessageEntry::ai("TaskB: 文档已处理".to_string()));
                Ok(StateUpdate::full(new_state))
            }
        });
        
        // 添加节点：任务C（模拟耗时200ms）
        graph.add_async_node("task_c", |state: &AgentState| {
            let state = state.clone();
            async move {
                sleep(Duration::from_millis(200)).await;
                let mut new_state = state;
                new_state.add_message(langchainrust::MessageEntry::ai("TaskC: 分析已完成".to_string()));
                Ok(StateUpdate::full(new_state))
            }
        });
        
        // 定义边：START → dispatcher
        graph.add_edge(START, "dispatcher");
        // 定义 FanOut：dispatcher → 同时发散（并行）到 task_a/b/c
        graph.add_fan_out("dispatcher", vec![
            "task_a".to_string(),
            "task_b".to_string(),
            "task_c".to_string(),
        ]);
        // 三个任务都指向 END
        graph.add_edge("task_a", END);
        graph.add_edge("task_b", END);
        graph.add_edge("task_c", END);
        
        // 编译图
        let compiled = graph.compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;
        
        // 用初始状态执行
        let initial_state = AgentState::new(input.clone());
        let result = compiled.invoke(initial_state).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        
        let total_time = start_time.elapsed();
        
        let sequential_estimate = 100 + 150 + 200;  // 串行估计
        let time_saved = (sequential_estimate as f32 - total_time.as_millis() as f32) / sequential_estimate as f32 * 100.0;
        
        // 提取每个任务的结果
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
    
    /// ──────────────────── 模式2：条件路由演示 ────────────────────
    ///
    /// 演示：根据输入长度(<10)自动选择"快速处理"或"详细分析"
    ///
    /// 图结构：
    ///                    ┌─ 长度<10 ─→ QuickProcess ─┐
    /// START → Analyze ──┤                            ├─→ END
    ///                    └─ 长度≥10 ─→ DetailedProcess ┘
    ///
    /// 关键点：执行路径由当前状态动态决定
    pub async fn run_conditional_demo(&self, input: String) -> Result<ConditionalDemoResult, GraphDemoError> {
        let mut graph: StateGraph<AgentState> = StateGraph::new();
        
        // 分析节点：分析输入内容
        graph.add_node_fn("analyze", |state| {
            let mut new_state = state.clone();
            new_state.add_message(langchainrust::MessageEntry::ai(
                format!("分析输入: 长度={}", state.input.len())
            ));
            Ok(StateUpdate::full(new_state))
        });
        
        // 快速处理（短输入）
        graph.add_node_fn("quick_process", |state| {
            let mut new_state = state.clone();
            new_state.add_message(langchainrust::MessageEntry::ai("快速处理完成".to_string()));
            new_state.set_output(format!("快速结果: {}", state.input));
            Ok(StateUpdate::full(new_state))
        });
        
        // 详细分析（长输入）
        graph.add_node_fn("detailed_process", |state| {
            let mut new_state = state.clone();
            new_state.add_message(langchainrust::MessageEntry::ai("详细处理完成".to_string()));
            new_state.set_output(format!("详细结果: {} (长度: {})", state.input, state.input.len()));
            Ok(StateUpdate::full(new_state))
        });
        
        graph.add_edge(START, "analyze");
        
        // 条件路由：analyze 节点之后，根据路由器的选择走不同路径
        let targets = HashMap::from([
            ("short".to_string(), "quick_process".to_string()),
            ("long".to_string(), "detailed_process".to_string()),
        ]);
        graph.add_conditional_edges("analyze", "length_router", targets, None);
        
        // 路由器：根据输入长度决定走哪条路
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
    
    /// ──────────────────── 模式3：流式执行演示 ────────────────────
    ///
    /// 演示：step1 → step2 → step3，逐步推送事件
    ///
    /// 图结构：START → step1 → step2 → step3 → END
    ///
    /// 事件类型：
    ///   graph_start:    图开始
    ///   enter:          进入某个节点（附带当前状态）
    ///   complete:       节点完成
    ///   state_update:   状态更新
    ///   graph_end:      图执行完成（附带最终状态）
    pub async fn run_stream_demo(&self, input: String) -> Result<Vec<StreamDemoEvent>, GraphDemoError> {
        // 使用简洁的 GraphBuilder 快速构建
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
        
        // 流式执行：一边执行一边返回事件列表
        let start_time = Instant::now();
        let events = compiled.stream(AgentState::new(input.clone())).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        
        // 把事件转成前端可读的格式
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
                    StreamEvent::End(state) | StreamEvent::EnterNode(_, state) => Some(StateSnapshot {
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
    
    /// 获取三种演示模式的说明信息（前端展示用）
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
