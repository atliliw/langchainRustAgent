//! 真实 Agent 任务执行引擎
//!
//! 架构：
//! 1. 规划器 (planner) — LLM 拆解任务 + 分配工具
//! 2. 执行器 (executor) — 拓扑排序 + 并发执行 + 上下文传递
//! 3. 验证器 (verifier) — LLM 检查最终结果

use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use langchainrust::{
    language_models::OpenAIChat,
    schema::Message,
    core::runnables::Runnable,
};
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tokio::time::sleep;

/// 可用工具列表（供 LLM 规划时参考）
pub const AVAILABLE_TOOLS: &[(&str, &str)] = &[
    ("llm_query", "直接用 LLM 回答，适合无需外部数据的子任务"),
    ("web_search", "搜索网络获取最新信息"),
    ("code_execute", "执行代码片段（Python/Rust）"),
    ("read_file", "读取本地文件内容"),
    ("summarize", "对已有结果进行总结提炼"),
];

pub struct AgentEngine;

impl AgentEngine {
    /// ── 1. 规划：LLM 拆解任务 + 分配工具 + 描述输入模板 ──
    pub async fn plan(config: &Config, task: String) -> Result<AgentPlan, GraphDemoError> {
        let llm = OpenAIChat::new(
            config.to_langchain_openai_config().with_max_tokens(2048)
        );

        let tools_json = AVAILABLE_TOOLS.iter()
            .map(|(n, d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}", n, d))
            .collect::<Vec<_>>().join(",\n  ");

        let prompt = format!(
            r#"你是一个任务规划专家。将任务拆解为 2-6 个子任务，为每个子任务指定工具。

可用工具：[{tools_json}]

返回 JSON，格式：
[
  {{
    "name": "子任务名（中文简短）",
    "description": "做什么",
    "tool": "工具名",
    "depends_on": ["前置子任务名"],
    "input_template": "执行时需要的前置结果说明，如'基于上一步的{{搜索总结}}来写报告'"
  }}
]

规则：
- 无依赖的子任务会并行执行
- input_template 说明需要从哪些前置任务获取数据
- 需要外部信息的用 web_search，纯推理的用 llm_query

任务：{task}

只返回 JSON。"#,
            tools_json = tools_json, task = task,
        );

        let resp = llm.invoke(vec![Message::human(&prompt)], None)
            .await.map_err(|e| GraphDemoError::ExecutionError(format!("规划失败: {}", e)))?;

        let cleaned = resp.content
            .trim_start_matches("```json").trim_start_matches("```")
            .trim_end_matches("```").trim();

        let tasks: Vec<AgentTask> = serde_json::from_str(cleaned)
            .map_err(|e| GraphDemoError::BuildError(format!(
                "规划格式错误: {} — 原始内容: {}",
                e, &cleaned.chars().take(300).collect::<String>()
            )))?;

        if tasks.is_empty() {
            return Err(GraphDemoError::BuildError("规划未返回子任务".to_string()));
        }

        // 构建图结构
        let graph_structure = Self::build_graph(&tasks);

        Ok(AgentPlan { original_task: task, tasks, graph_structure })
    }

    fn build_graph(tasks: &[AgentTask]) -> serde_json::Value {
        let nodes: Vec<String> = tasks.iter().map(|t| t.name.clone()).collect();
        let mut edges = Vec::new();

        // 链式：保证所有节点可达
        for i in 0..tasks.len() {
            if i == 0 {
                edges.push(serde_json::json!({"type":"fixed","source":"__start__","target":tasks[i].name}));
            }
            if i + 1 < tasks.len() {
                edges.push(serde_json::json!({"type":"fixed","source":tasks[i].name,"target":tasks[i+1].name}));
            }
        }

        // 依赖边
        for task in tasks {
            for dep in &task.depends_on {
                if nodes.iter().any(|n| n == dep) {
                    edges.push(serde_json::json!({"type":"fixed","source":dep,"target":task.name}));
                }
            }
            edges.push(serde_json::json!({"type":"fixed","source":task.name,"target":"__end__"}));
        }

        serde_json::json!({
            "entry_point": tasks[0].name,
            "nodes": nodes,
            "edges": edges,
            "routers": [],
        })
    }

    /// ── 2. 执行：用 StateGraph 管理状态和上下文传递 ──
    pub async fn execute(
        config: &Config,
        task: String,
        agent_tasks: Vec<AgentTask>,
    ) -> Result<AgentExecResponse, GraphDemoError> {
        let total_start = Instant::now();
        let llm: Arc<OpenAIChat> = Arc::new(
            OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024))
        );

        use langchainrust::{
            StateGraph, START, END,
            AgentState, StateUpdate,
            CompiledGraph,
        };

        // 动态构建 StateGraph
        let mut graph: StateGraph<AgentState> = StateGraph::new();
        let name_set: std::collections::HashSet<&str> =
            agent_tasks.iter().map(|t| t.name.as_str()).collect();

        // 每个子任务作为一个 async node
        for at in &agent_tasks {
            let node_name = at.name.clone();
            let desc = at.description.clone();
            let tool = at.tool.clone();
            let template = at.input_template.clone();
            let l = llm.clone();
            let t = task.clone();

            graph.add_async_node(node_name.clone(), move |state: &AgentState| {
                let s = state.clone();
                let n = node_name.clone();
                let d = desc.clone();
                let tool = tool.clone();
                let tmpl = template.clone();
                let l = l.clone();
                let t = t.clone();
                async move {
                    // 从 state.messages 中提取前置结果
                    let ctx: String = s.messages.iter()
                        .map(|m| format!("[{}]: {}", match m.role { langchainrust::MessageRole::AI => "AI", _ => "User" }, m.content))
                        .collect::<Vec<_>>().join("\n");

                    let prompt = format!(
                        "任务：{t}\n当前子任务：{d}\n工具：{tool}\n输入：{tmpl}\n\n已完成的任务结果：\n{ctx}\n\n请输出结果。",
                        t = t, d = d, tool = tool, tmpl = tmpl, ctx = ctx,
                    );

                    let resp = l.invoke(vec![Message::human(&prompt)], None).await
                        .map_err(|e| langchainrust::langgraph::GraphError::NodeError(e.to_string()))?;

                    let output = resp.content.chars().take(500).collect::<String>();
                    let mut new_state = s;
                    new_state.add_message(langchainrust::MessageEntry::ai(format!("[{}] {}", n, output)));
                    new_state.add_step(langchainrust::StepEntry::new(n, output.clone()));
                    Ok(StateUpdate::full(new_state))
                }
            });
        }

        // 建边：链式保证可达 + 依赖边
        graph.add_edge(START, &agent_tasks[0].name);
        for i in 1..agent_tasks.len() {
            graph.add_edge(&agent_tasks[i-1].name, &agent_tasks[i].name);
        }
        for at in &agent_tasks {
            for dep in &at.depends_on {
                if name_set.contains(dep.as_str()) {
                    graph.add_edge(dep, &at.name);
                }
            }
            graph.add_edge(&at.name, END);
        }

        let compiled = graph.compile()
            .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;

        // 执行图
        let initial = AgentState::new(task.clone());
        let result = compiled.invoke(initial).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;

        // 从 state.steps 中提取执行结果
        let exec_results: Vec<AgentExecResult> = result.final_state.steps.iter()
            .filter_map(|step| {
                let at = agent_tasks.iter().find(|t| t.name == step.action)?;
                Some(AgentExecResult {
                    task_name: step.action.clone(),
                    tool: at.tool.clone(),
                    input_summary: at.input_template.clone(),
                    output: step.observation.clone(),
                    duration_ms: 0,
                    tokens: 0,
                })
            }).collect();

        // ── 3. 验证 + 最终汇总 ──
        let verifier_llm = OpenAIChat::new(
            config.to_langchain_openai_config().with_max_tokens(1024)
        );

        let exec_summary: String = exec_results.iter()
            .map(|r| format!("【{}】\n{}", r.task_name, r.output))
            .collect::<Vec<_>>().join("\n\n");

        let verify_prompt = format!(
            r#"检查以下执行结果是否完成了用户任务。

任务：{task}

执行结果：
{exec_summary}

如果完成，给出最终答案。如果没完成，说明缺少什么。

返回 JSON：{{"completed": true/false, "final_answer": "最终答案", "missing": "缺少说明"}}"#,
            task = task, exec_summary = exec_summary,
        );

        let verify_resp = verifier_llm.invoke(vec![Message::human(&verify_prompt)], None).await
            .map_err(|e| GraphDemoError::ExecutionError(format!("验证失败: {}", e)))?;

        let verify_cleaned = verify_resp.content
            .trim_start_matches("```json").trim_start_matches("```")
            .trim_end_matches("```").trim();

        #[derive(serde::Deserialize)]
        struct VerifyResult {
            completed: bool,
            #[serde(default)]
            final_answer: String,
            #[serde(default)]
            missing: String,
        }

        let verify: VerifyResult = serde_json::from_str(verify_cleaned).unwrap_or(VerifyResult {
            completed: true,
            final_answer: exec_results.last().map(|r| r.output.clone()).unwrap_or_default(),
            missing: String::new(),
        });

        let total_tokens: usize = exec_results.iter().map(|r| r.tokens).sum::<usize>()
            + verify_resp.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);

        Ok(AgentExecResponse {
            results: exec_results,
            final_answer: if verify.completed { verify.final_answer } else {
                format!("执行不完整: {}", verify.missing)
            },
            total_duration_ms: total_start.elapsed().as_millis() as u64,
            total_tokens,
        })
    }
}
