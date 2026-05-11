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

use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;

use langchainrust::{
    StateGraph, GraphBuilder, START, END,
    AgentState, StateUpdate, StateSchema,
    MessageEntry, ExecutionStep,
    FunctionRouter,
    StreamEvent,
    CompiledGraph,
    SubgraphNode,
    language_models::OpenAIChat,
    schema::Message,
    core::runnables::Runnable,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

pub struct LangGraphDemoService {}

impl LangGraphDemoService {
    pub fn new() -> Self {
        Self {}
    }

    /// ──────────────────── 图构建器 ────────────────────

    fn build_parallel_graph() -> Result<CompiledGraph<AgentState>, GraphDemoError> {
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

        graph.compile().map_err(|e| GraphDemoError::BuildError(e.to_string()))
    }

    fn build_conditional_graph() -> Result<CompiledGraph<AgentState>, GraphDemoError> {
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

        let router = FunctionRouter::new(|state: &AgentState| {
            if state.input.len() < 10 { "short" } else { "long" }.to_string()
        });
        graph.set_conditional_router("length_router", router);

        graph.add_edge("quick_process", END);
        graph.add_edge("detailed_process", END);

        graph.compile().map_err(|e| GraphDemoError::BuildError(e.to_string()))
    }

    fn build_stream_graph() -> Result<CompiledGraph<AgentState>, GraphDemoError> {
        GraphBuilder::<AgentState>::new()
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
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))
    }

    fn build_subgraph_graph() -> Result<CompiledGraph<AgentState>, GraphDemoError> {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct ReviewState {
            content: String,
            review_result: String,
            decision: String,
        }
        impl StateSchema for ReviewState {}

        let mut sub = StateGraph::<ReviewState>::new();
        sub.add_node_fn("审核内容", |state| Ok(StateUpdate::full(state.clone())));
        sub.add_node_fn("输出结果", |state| Ok(StateUpdate::full(state.clone())));
        sub.add_edge(START, "审核内容");
        sub.add_edge("审核内容", "输出结果");
        sub.add_edge("输出结果", END);
        let compiled_sub = sub.compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;

        let mut parent = StateGraph::<AgentState>::new();
        parent.add_node_fn("生成内容", |state| Ok(StateUpdate::full(state.clone())));
        parent.add_subgraph(
            "质量审核", compiled_sub,
            |p: &AgentState| -> ReviewState {
                ReviewState { content: p.output.clone().unwrap_or_default(), review_result: String::new(), decision: String::new() }
            },
            |s: &ReviewState, p: &mut AgentState| {
                p.add_message(MessageEntry::ai(format!("审核结果：{}", s.review_result)));
            },
        );
        parent.add_edge(START, "生成内容");
        parent.add_edge("生成内容", "质量审核");
        parent.add_edge("质量审核", END);
        parent.compile().map_err(|e| GraphDemoError::BuildError(e.to_string()))
    }

    fn build_llm_conditional_graph() -> Result<CompiledGraph<AgentState>, GraphDemoError> {
        use langchainrust::langgraph::FunctionRouter;
        let router = FunctionRouter::new(|state: &AgentState| {
            if state.input.len() > 10 { "tech".to_string() } else { "general".to_string() }
        });
        let mut graph = StateGraph::<AgentState>::new();
        graph.set_entry_point("分析问题");
        graph.add_node_fn("分析问题", |state| Ok(StateUpdate::full(state.clone())));
        graph.add_node_fn("技术路线", |state| Ok(StateUpdate::full(state.clone())));
        graph.add_node_fn("通用路线", |state| Ok(StateUpdate::full(state.clone())));
        graph.add_node_fn("生成回答", |state| Ok(StateUpdate::full(state.clone())));
        graph.add_edge(START, "分析问题");
        graph.set_conditional_router("length_router", router);
        graph.add_conditional_edges(
            "分析问题", "length_router",
            std::collections::HashMap::from([
                ("tech".to_string(), "技术路线".to_string()),
                ("general".to_string(), "通用路线".to_string()),
            ]),
            Some("通用路线".to_string()),
        );
        graph.add_edge("技术路线", "生成回答");
        graph.add_edge("通用路线", "生成回答");
        graph.add_edge("生成回答", END);
        graph.compile().map_err(|e| GraphDemoError::BuildError(e.to_string()))
    }

    /// ──────────────────── 可视化接口 ────────────────────

    pub fn get_graph_structure(mode: &str) -> Result<serde_json::Value, GraphDemoError> {
        let compiled = match mode {
            "parallel" => Self::build_parallel_graph()?,
            "conditional" => Self::build_conditional_graph()?,
            "stream" => Self::build_stream_graph()?,
            "subgraph" => Self::build_subgraph_graph()?,
            "llm_conditional" => Self::build_llm_conditional_graph()?,
            _ => return Err(GraphDemoError::BuildError(format!("未知模式: {}", mode))),
        };
        Ok(compiled.visualize_json())
    }

    pub fn get_graph_mermaid(mode: &str) -> Result<String, GraphDemoError> {
        let compiled = match mode {
            "parallel" => Self::build_parallel_graph()?,
            "conditional" => Self::build_conditional_graph()?,
            "stream" => Self::build_stream_graph()?,
            "subgraph" => Self::build_subgraph_graph()?,
            "llm_conditional" => Self::build_llm_conditional_graph()?,
            _ => return Err(GraphDemoError::BuildError(format!("未知模式: {}", mode))),
        };
        let json = compiled.visualize_json();
        let edges = json["edges"].as_array().unwrap();
        let nodes = json["nodes"].as_array().unwrap();

        let mut mermaid = String::from("graph TD\n");
        mermaid.push_str("  START(\"START\")\n");
        mermaid.push_str("  END[\"END\"]\n");

        for node in nodes {
            let name = node.as_str().unwrap();
            mermaid.push_str(&format!("  {}[\"{}\"]\n", name, name));
        }

        for edge in edges {
            let etype = edge["type"].as_str().unwrap_or("fixed");
            let source = edge["source"].as_str().unwrap_or("");
            let source = if source == "__start__" { "START" } else { source };

            match etype {
                "fanout" => {
                    let targets = edge["targets"].as_array().unwrap();
                    for target in targets {
                        let t = target.as_str().unwrap();
                        let t = if t == "__end__" { "END" } else { t };
                        mermaid.push_str(&format!("  {} --> {}\n", source, t));
                    }
                }
                "conditional" => {
                    let router = edge["router"].as_str().unwrap_or("router");
                    let _ = router;
                    let targets = edge["targets"].as_object().unwrap();
                    for (route, target) in targets {
                        let t = target.as_str().unwrap();
                        let t = if t == "__end__" { "END" } else { t };
                        mermaid.push_str(&format!("  {} -- \"{}\" --> {}\n", source, route, t));
                    }
                }
                _ => {
                    let target = edge["target"].as_str().unwrap_or("");
                    let target = if target == "__end__" { "END" } else { target };
                    mermaid.push_str(&format!("  {} --> {}\n", source, target));
                }
            }
        }

        Ok(mermaid)
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
        let compiled = Self::build_parallel_graph()?;

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
        let compiled = Self::build_conditional_graph()?;

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
        let compiled = Self::build_stream_graph()?;

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
        let parallel_mermaid = Self::get_graph_mermaid("parallel").unwrap_or_default();
        let conditional_mermaid = Self::get_graph_mermaid("conditional").unwrap_or_default();
        let stream_mermaid = Self::get_graph_mermaid("stream").unwrap_or_default();

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
                "features": ["add_fan_out", "add_async_node"],
                "mermaid": parallel_mermaid
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
                "features": ["add_conditional_edges", "FunctionRouter"],
                "mermaid": conditional_mermaid
            },
            "stream_demo": {
                "name": "流式执行演示",
                "description": "展示执行事件流",
                "nodes": ["step1", "step2", "step3"],
                "edges": ["START → step1 → step2 → step3 → END"],
                "features": ["stream()", "StreamEvent"],
                "mermaid": stream_mermaid
            }
        })
    }

    /// ──────────────────── AI 任务拆解 ────────────────────
    ///
    /// LLM 分析用户任务，拆成 N 个子任务，构建图结构
    /// 不执行，返回给前端展示，用户确认后再执行
    pub async fn decompose_task(
        config: &Config,
        task: String,
    ) -> Result<TaskDecomposeResult, GraphDemoError> {
        let llm = OpenAIChat::new(
            config.to_langchain_openai_config().with_max_tokens(512)
        );

        let prompt = format!(
            r#"你是一个任务拆解专家。将以下任务拆解为 2-6 个子任务。

返回 JSON，格式：
[
  {{"name": "子任务名（中文简短，不要英文）", "description": "子任务描述（中文）", "depends_on": ["前置任务名"]}}
]

用户任务：{task}

只返回 JSON。"#,
            task = task
        );

        let resp = llm.invoke(vec![Message::human(&prompt)], None)
            .await.map_err(|e| GraphDemoError::ExecutionError(format!("LLM 失败: {}", e)))?;

        let cleaned = resp.content
            .trim_start_matches("```json").trim_start_matches("```")
            .trim_end_matches("```").trim();

        let sub_tasks: Vec<SubTaskDef> = serde_json::from_str(cleaned)
            .map_err(|e| GraphDemoError::BuildError(format!(
                "LLM 返回格式错误: {} — 原始内容: {}",
                e, &cleaned.chars().take(300).collect::<String>()
            )))?;

        if sub_tasks.is_empty() {
            return Err(GraphDemoError::BuildError("LLM 未返回子任务".to_string()));
        }

        // 构建图结构
        let mut graph: StateGraph<AgentState> = StateGraph::new();
        let name_set: std::collections::HashSet<&str> =
            sub_tasks.iter().map(|s| s.name.as_str()).collect();

        for sub in &sub_tasks {
            graph.add_node_fn(sub.name.clone(), |state| Ok(StateUpdate::full(state.clone())));
        }

        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();

        graph.add_edge(START, &sub_tasks[0].name);
        seen.insert((START.to_string(), sub_tasks[0].name.clone()));
        for i in 1..sub_tasks.len() {
            let k = (sub_tasks[i-1].name.clone(), sub_tasks[i].name.clone());
            seen.insert(k);
            graph.add_edge(&sub_tasks[i-1].name, &sub_tasks[i].name);
        }
        // 依赖边
        for sub in &sub_tasks {
            for dep in &sub.depends_on {
                if name_set.contains(dep.as_str()) {
                    let k = (dep.clone(), sub.name.clone());
                    if seen.insert(k) {
                        graph.add_edge(dep, &sub.name);
                    }
                }
            }
        }

        // 全部 → END（含去重）
        for sub in &sub_tasks {
            let k = (sub.name.clone(), END.to_string());
            if seen.insert(k) {
                graph.add_edge(&sub.name, END);
            }
        }

        let compiled = graph.compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;

        Ok(TaskDecomposeResult {
            original_task: task,
            sub_tasks,
            graph_structure: compiled.visualize_json(),
        })
    }

    /// ──────────────────── 模式4：子图演示 ────────────────────
    ///
    /// 子图 = 把一个小图包装成节点，嵌到父图里
    /// 这里演示：
    ///   父图：生成内容 → 子图(质量审核) → 输出
    ///   子图：审核 → 决定(通过/需修改) → 结束
    ///
    /// 子图和父图可以使用不同的状态类型
    pub async fn run_subgraph_demo(input: String) -> Result<SubgraphDemoResult, GraphDemoError> {
        let total_start = Instant::now();

        // ── 1. 定义子图的状态类型（不同于父图的 AgentState） ──
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct ReviewState {
            content: String,
            review_result: String,
            decision: String,
        }
        impl StateSchema for ReviewState {}

        // ── 2. 构建子图 ──
        let mut sub = StateGraph::<ReviewState>::new();
        sub.add_node_fn("审核内容", |state| {
            let mut s = state.clone();
            s.review_result = format!("审核通过：内容长度 {} 字，符合要求", state.content.len());
            s.decision = if state.content.len() > 5 { "通过".into() } else { "需修改".into() };
            Ok(StateUpdate::full(s))
        });
        sub.add_node_fn("输出结果", |state| Ok(StateUpdate::full(state.clone())));
        sub.add_edge(START, "审核内容");
        sub.add_edge("审核内容", "输出结果");
        sub.add_edge("输出结果", END);
        let compiled_sub = sub.compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;

        // ── 3. 构建父图 ──
        let mut parent = StateGraph::<AgentState>::new();
        parent.add_node_fn("生成内容", |state| {
            let mut s = state.clone();
            let content = format!("这是一篇关于「{}」的文章...", state.input);
            s.add_message(MessageEntry::ai(content.clone()));
            s.output = Some(content);
            Ok(StateUpdate::full(s))
        });

        // 把子图作为一个节点嵌入父图
        parent.add_subgraph(
            "质量审核",
            compiled_sub,
            // 父状态 → 子状态
            |parent_state: &AgentState| -> ReviewState {
                ReviewState {
                    content: parent_state.output.clone().unwrap_or_default(),
                    review_result: String::new(),
                    decision: String::new(),
                }
            },
            // 子状态 → 父状态
            |sub_state: &ReviewState, parent_state: &mut AgentState| {
                parent_state.add_message(MessageEntry::ai(
                    format!("审核结果：{} | 决定：{}", sub_state.review_result, sub_state.decision)
                ));
            },
        );

        parent.add_edge(START, "生成内容");
        parent.add_edge("生成内容", "质量审核");
        parent.add_edge("质量审核", END);

        let compiled_parent = parent.compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;

        // ── 4. 执行父图 ──
        let initial = AgentState::new(input.clone());
        let result = compiled_parent.invoke(initial).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;

        let total_ms = total_start.elapsed().as_millis() as u64;

        // 从消息中提取审核结果
        let review_msg = result.final_state.messages.iter()
            .find(|m| m.content.contains("审核结果"))
            .map(|m| m.content.clone())
            .unwrap_or_default();

        let content_msg = result.final_state.messages.iter()
            .find(|m| m.content.contains("这是一篇关于"))
            .map(|m| m.content.clone())
            .unwrap_or_default();

        Ok(SubgraphDemoResult {
            input,
            generated_content: content_msg,
            review_result: review_msg,
            total_duration_ms: total_ms,
        })
    }

    /// ──────────────────── 模式5：LLM 条件路由演示 ────────────────────
    ///
    /// 不是用固定规则（如 input.len() < 10），而是用 LLM 判断意图来决定路由
    ///
    /// 流程：
    ///   START → 分析意图 → [LLM 判断路由] ─┬─ "tech" → 技术回答
    ///                                       ├─ "general" → 通用回答
    ///                                       └─ "other" → 兜底回答
    pub async fn run_llm_conditional_demo(
        config: &Config,
        input: String,
    ) -> Result<LLMConditionalResult, GraphDemoError> {
        let total_start = Instant::now();

        let mut graph: StateGraph<AgentState> = StateGraph::new();

        // 分析意图节点
        graph.add_node_fn("分析意图", |state| {
            let mut s = state.clone();
            s.add_message(MessageEntry::ai(format!("分析输入：{}", state.input)));
            Ok(StateUpdate::full(s))
        });

        // 三个回答节点（只保留一个，由路由决定）
        graph.add_node_fn("技术回答", |state| {
            let mut s = state.clone();
            s.set_output(format!("【技术解答】{} —— 从技术角度详细解释...", state.input));
            Ok(StateUpdate::full(s))
        });
        graph.add_node_fn("通用回答", |state| {
            let mut s = state.clone();
            s.set_output(format!("【通用回答】{} —— 用通俗易懂的方式解释...", state.input));
            Ok(StateUpdate::full(s))
        });
        graph.add_node_fn("兜底回答", |state| {
            let mut s = state.clone();
            s.set_output(format!("【其他】关于「{}」, 这是一个未分类的问题", state.input));
            Ok(StateUpdate::full(s))
        });

        graph.add_edge(START, "分析意图");

        // 条件路由 — 用 LLM 判断意图
        let targets = HashMap::from([
            ("tech".to_string(), "技术回答".to_string()),
            ("general".to_string(), "通用回答".to_string()),
            ("other".to_string(), "兜底回答".to_string()),
        ]);
        graph.add_conditional_edges("分析意图", "llm_intent_router", targets, Some("兜底回答".to_string()));

        // LLM 路由函数
        let config_for_router = config.clone();
        let router = FunctionRouter::new(move |state: &AgentState| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let llm = OpenAIChat::new(
                    config_for_router.to_langchain_openai_config().with_max_tokens(50)
                );
                let prompt = format!(
                    "判断以下问题属于哪个类别，只返回一个词：tech（技术问题）、general（通用知识）、other（其他）。\n问题：{}",
                    state.input
                );
                match llm.invoke(vec![Message::human(&prompt)], None).await {
                    Ok(r) => {
                        let answer = r.content.trim().to_lowercase();
                        if answer.contains("tech") { "tech" }
                        else if answer.contains("general") { "general" }
                        else { "other" }
                    }
                    Err(_) => "other",
                }.to_string()
            })
        });
        graph.set_conditional_router("llm_intent_router", router);

        graph.add_edge("技术回答", END);
        graph.add_edge("通用回答", END);
        graph.add_edge("兜底回答", END);

        let compiled = graph.compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;

        let initial = AgentState::new(input.clone());
        let result = compiled.invoke(initial).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;

        // 通过在 stpes 中找路由后的节点来判断走了哪条路
        let route_taken = result.steps.iter()
            .find_map(|s| match s {
                langchainrust::ExecutionStep::Node { name, .. } => {
                    if name != "分析意图" && name != START && name != END {
                        Some(name.clone())
                    } else { None }
                }
                _ => None,
            }).unwrap_or_default();

        let total_ms = total_start.elapsed().as_millis() as u64;

        Ok(LLMConditionalResult {
            input,
            route_taken,
            output: result.final_state.output.unwrap_or_default(),
            steps: result.final_state.messages.iter().map(|m| m.content.clone()).collect(),
            total_duration_ms: total_ms,
        })
    }

    /// ──────────────────── 执行子任务 ────────────────────
    ///
    /// 单次 LLM 调用执行所有子任务（比逐个并行调快得多）
    pub async fn execute_sub_tasks(
        config: &Config,
        task: String,
        sub_tasks: Vec<SubTaskDef>,
    ) -> Result<Vec<SubTaskExecResult>, GraphDemoError> {
        let llm = OpenAIChat::new(
            config.to_langchain_openai_config().with_max_tokens(1024)
        );

        let task_list: String = sub_tasks.iter()
            .map(|s| format!("- {}: {}", s.name, s.description))
            .collect::<Vec<_>>().join("\n");

        let prompt = format!(
            r#"请依次执行以下子任务，为每个子任务输出结果。

任务：{task}

子任务列表：
{task_list}

返回 JSON 数组，格式：
[
  {{"name": "子任务名", "output": "执行结果"}}
]

只返回 JSON。"#,
            task = task, task_list = task_list,
        );

        let start = Instant::now();
        let resp = llm.invoke(vec![Message::human(&prompt)], None)
            .await.map_err(|e| GraphDemoError::ExecutionError(format!("LLM 失败: {}", e)))?;

        let total_tokens = resp.token_usage.as_ref().map(|u| u.total_tokens as u64).unwrap_or(0);
        let cleaned = resp.content
            .trim_start_matches("```json").trim_start_matches("```")
            .trim_end_matches("```").trim();

        let exec_results: Vec<SubTaskExecResult> = serde_json::from_str(cleaned)
            .map_err(|e| GraphDemoError::BuildError(format!(
                "LLM 返回格式错误: {} — 原始内容: {}",
                e, &cleaned.chars().take(300).collect::<String>()
            )))?;

        let elapsed = start.elapsed().as_millis() as u64;
        let count = std::cmp::max(1, exec_results.len()) as u64;

        Ok(exec_results.into_iter().map(|mut r| {
            r.duration_ms = elapsed / count;
            r.tokens = (total_tokens / count) as usize;
            r
        }).collect())
    }
}

impl Default for LangGraphDemoService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_build_cycle_deps() {
        // 模拟所有节点循环依赖（旧代码会 fail）
        let sub_tasks = vec![
            SubTaskDef { name: "a".into(), description: "A".into(), depends_on: vec!["b".into()] },
            SubTaskDef { name: "b".into(), description: "B".into(), depends_on: vec!["c".into()] },
            SubTaskDef { name: "c".into(), description: "C".into(), depends_on: vec!["a".into()] },
        ];

        let mut graph: StateGraph<AgentState> = StateGraph::new();
        let name_set: std::collections::HashSet<&str> = sub_tasks.iter().map(|s| s.name.as_str()).collect();
        for sub in &sub_tasks {
            graph.add_node_fn(sub.name.clone(), |state| Ok(StateUpdate::full(state.clone())));
        }

        graph.add_edge(START, &sub_tasks[0].name);
        for i in 1..sub_tasks.len() {
            graph.add_edge(&sub_tasks[i-1].name, &sub_tasks[i].name);
        }
        graph.add_edge(&sub_tasks[sub_tasks.len()-1].name, END);

        for sub in &sub_tasks {
            for dep in &sub.depends_on {
                if name_set.contains(dep.as_str()) { graph.add_edge(dep, &sub.name); }
            }
        }

        let result = graph.compile();
        assert!(result.is_ok(), "循环依赖场景: {:?}", result.err());
    }

    #[test]
    fn test_graph_build_all_deps_nonexistent() {
        let sub_tasks = vec![
            SubTaskDef { name: "a".into(), description: "A".into(), depends_on: vec!["x".into()] },
            SubTaskDef { name: "b".into(), description: "B".into(), depends_on: vec!["y".into()] },
        ];

        let mut graph: StateGraph<AgentState> = StateGraph::new();
        let name_set: std::collections::HashSet<&str> = sub_tasks.iter().map(|s| s.name.as_str()).collect();
        for sub in &sub_tasks {
            graph.add_node_fn(sub.name.clone(), |state| Ok(StateUpdate::full(state.clone())));
        }

        graph.add_edge(START, &sub_tasks[0].name);
        for i in 1..sub_tasks.len() {
            graph.add_edge(&sub_tasks[i-1].name, &sub_tasks[i].name);
        }
        graph.add_edge(&sub_tasks[sub_tasks.len()-1].name, END);

        for sub in &sub_tasks {
            for dep in &sub.depends_on {
                if name_set.contains(dep.as_str()) { graph.add_edge(dep, &sub.name); }
            }
        }

        let result = graph.compile();
        assert!(result.is_ok(), "不存在依赖场景: {:?}", result.err());
    }
}
