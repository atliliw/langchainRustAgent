//! 真实 Agent 任务执行引擎
//!
//! 支持逐步执行（中断/恢复）：
//! 1. execute_next — 每次执行一个就绪任务，返回结果 + 是否有下一任务
//! 2. resume — 继续执行下一任务
//! 3. 全部执行完成后自动验证

use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use langchainrust::{
    language_models::OpenAIChat,
    schema::Message,
    core::runnables::Runnable,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub const AVAILABLE_TOOLS: &[(&str, &str)] = &[
    ("llm_query", "直接用 LLM 回答"),
    ("web_search", "搜索网络获取最新信息"),
    ("code_execute", "执行代码片段"),
    ("read_file", "读取本地文件"),
    ("summarize", "对结果进行总结"),
];

/// 全局执行状态存储（每个 session 一个执行上下文）
static EXEC_STORE: Mutex<Option<HashMap<String, StoredExec>>> = Mutex::new(None);

fn with_store<F, R>(f: F) -> R where F: FnOnce(&mut HashMap<String, StoredExec>) -> R {
    let mut guard = EXEC_STORE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

struct StoredExec {
    task: String,
    remaining: Vec<AgentTask>,
    completed: Vec<AgentExecResult>,
    start_time: Instant,
}

pub struct AgentEngine;

impl AgentEngine {
    /// ── 1. 规划：LLM 拆解任务 + 分配工具 ──
    pub async fn plan(config: &Config, task: String) -> Result<AgentPlan, GraphDemoError> {
        let llm = OpenAIChat::new(
            config.to_langchain_openai_config().with_max_tokens(2048)
        );
        let tools_json = AVAILABLE_TOOLS.iter()
            .map(|(n, d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}", n, d))
            .collect::<Vec<_>>().join(",\n  ");
        let prompt = format!(
            r#"你是一个任务规划专家。将任务拆解为 2-5 个子任务并分配工具。

可用工具：[{tools_json}]

返回 JSON：
[
  {{"name":"子任务名（中文）","description":"做什么","tool":"工具名","depends_on":["前置任务"],"input_template":"需要从上游拿什么"}}
]

任务：{task}

只返回 JSON。"#,
            tools_json = tools_json, task = task,
        );
        let resp = llm.invoke(vec![Message::human(&prompt)], None).await
            .map_err(|e| GraphDemoError::ExecutionError(format!("规划失败: {}", e)))?;
        let cleaned = resp.content.trim_start_matches("```json").trim_start_matches("```")
            .trim_end_matches("```").trim();
        let tasks: Vec<AgentTask> = serde_json::from_str(cleaned)
            .map_err(|e| GraphDemoError::BuildError(format!("规划格式错误: {} — 原始内容: {}",
                e, &cleaned.chars().take(300).collect::<String>())))?;
        if tasks.is_empty() { return Err(GraphDemoError::BuildError("规划为空".to_string())); }
        let gs = Self::build_graph(&tasks);
        Ok(AgentPlan { original_task: task, tasks, graph_structure: gs })
    }

    fn build_graph(tasks: &[AgentTask]) -> serde_json::Value {
        let nodes: Vec<String> = tasks.iter().map(|t| t.name.clone()).collect();
        let mut edges = Vec::new();
        for i in 0..tasks.len() {
            if i == 0 { edges.push(serde_json::json!({"type":"fixed","source":"__start__","target":tasks[i].name})); }
            if i + 1 < tasks.len() { edges.push(serde_json::json!({"type":"fixed","source":tasks[i].name,"target":tasks[i+1].name})); }
        }
        for t in tasks {
            for d in &t.depends_on { if nodes.iter().any(|n| n == d) { edges.push(serde_json::json!({"type":"fixed","source":d,"target":t.name})); } }
            edges.push(serde_json::json!({"type":"fixed","source":t.name,"target":"__end__"}));
        }
        serde_json::json!({"entry_point":tasks[0].name,"nodes":nodes,"edges":edges,"routers":[]})
    }

    /// ── 2. 开始执行（存入 store，执行第一个就绪任务）──
    /// 返回: 第一个任务的结果 + session_id（用于继续）
    pub async fn execute_start(
        config: &Config, task: String, agent_tasks: Vec<AgentTask>,
    ) -> Result<(String, AgentExecResult, bool), GraphDemoError> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let llm = Self::build_llm(config);

        // 找第一个就绪任务
        let (result, has_next) = Self::run_next_ready(&llm, &task, &agent_tasks, &[]).await?;

        // 存储剩余任务
        let remaining: Vec<AgentTask> = agent_tasks.iter()
            .filter(|t| t.name != result.task_name).cloned().collect();
        let completed = vec![result.clone()];

        with_store(|m| {
            m.insert(session_id.clone(), StoredExec {
                task: task.clone(), remaining, completed: completed.clone(),
                start_time: Instant::now(),
            });
        });

        Ok((session_id, result, has_next))
    }

    /// ── 3. 继续执行（从 store 恢复状态）──
    /// 返回: 下一任务结果 + 是否还有后续任务
    pub async fn execute_next(
        config: &Config, session_id: &str,
    ) -> Result<(AgentExecResult, bool), GraphDemoError> {
        let (task, remaining, completed) = {
            let guard = EXEC_STORE.lock().unwrap();
            let map = guard.as_ref().unwrap();
            let s = map.get(session_id).ok_or_else(||
                GraphDemoError::BuildError("session 不存在或已过期".to_string())
            )?;
            (s.task.clone(), s.remaining.clone(), s.completed.clone())
        };

        let llm = Self::build_llm(config);
        let (result, has_next) = Self::run_next_ready(&llm, &task, &remaining, &completed).await?;

        with_store(|m| {
            if let Some(s) = m.get_mut(session_id) {
                s.remaining.retain(|t| t.name != result.task_name);
                s.completed.push(result.clone());
            }
        });

        Ok((result, has_next))
    }

    /// ── 获取最终验证结果 ──
    pub async fn finalize(
        config: &Config, session_id: &str,
    ) -> Result<AgentExecResponse, GraphDemoError> {
        let (task, completed, total_start) = {
            let guard = EXEC_STORE.lock().unwrap();
            let map = guard.as_ref().unwrap();
            let s = map.get(session_id).ok_or_else(||
                GraphDemoError::BuildError("session 不存在".to_string())
            )?;
            (s.task.clone(), s.completed.clone(), s.start_time)
        };

        let verifier = Self::build_llm(config);
        let summary: String = completed.iter()
            .map(|r| format!("【{}】\n{}", r.task_name, r.output))
            .collect::<Vec<_>>().join("\n\n");

        let vp = format!(
            r#"检查以下结果是否完成了任务。

任务：{task}

结果：
{summary}

返回 JSON：{{"completed":true/false,"final_answer":"最终答案","missing":"缺少什么"}}"#,
            task = task, summary = summary,
        );
        let vr = verifier.invoke(vec![Message::human(&vp)], None).await
            .map_err(|e| GraphDemoError::ExecutionError(format!("验证失败: {}", e)))?;
        let vc = vr.content.trim_start_matches("```json").trim_start_matches("```")
            .trim_end_matches("```").trim();
        #[derive(serde::Deserialize)]
        struct V { completed: bool, #[serde(default)] final_answer: String, #[serde(default)] missing: String }
        let v: V = serde_json::from_str(vc).unwrap_or(V {
            completed: true, final_answer: completed.last().map(|r| r.output.clone()).unwrap_or_default(), missing: String::new(),
        });

        with_store(|m| { m.remove(session_id); });

        Ok(AgentExecResponse {
            results: completed,
            final_answer: if v.completed { v.final_answer } else { format!("不完整: {}", v.missing) },
            total_duration_ms: total_start.elapsed().as_millis() as u64,
            total_tokens: 0,
        })
    }

    /// ── 核心：找出就绪任务并执行 ──
    async fn run_next_ready(
        llm: &OpenAIChat, task: &str, remaining: &[AgentTask], completed: &[AgentExecResult],
    ) -> Result<(AgentExecResult, bool), GraphDemoError> {
        let name_set: std::collections::HashSet<&str> =
            remaining.iter().map(|t| t.name.as_str()).collect();

        // 找第一个依赖已满足的任务
        let ready = remaining.iter().find(|t| {
            t.depends_on.iter()
                .filter(|d| name_set.contains(d.as_str()))
                .all(|d| completed.iter().any(|c| &c.task_name == d))
        }).ok_or_else(|| GraphDemoError::BuildError("没有可执行的任务（可能存在循环依赖）".to_string()))?;

        let ctx: String = completed.iter()
            .map(|r| format!("[{}] {}", r.task_name, r.output))
            .collect::<Vec<_>>().join("\n");

        let prompt = format!(
            "任务：{task}\n子任务：{desc}\n工具：{tool}\n输入：{tmpl}\n\n前置结果：\n{ctx}\n\n输出结果。",
            task = task, desc = ready.description, tool = ready.tool, tmpl = ready.input_template, ctx = ctx,
        );

        let start = Instant::now();
        let resp = llm.invoke(vec![Message::human(&prompt)], None).await
            .map_err(|e| GraphDemoError::ExecutionError(format!("执行失败: {}", e)))?;

        let output = resp.content.chars().take(500).collect::<String>();
        let has_next = remaining.iter().any(|t| t.name != ready.name);

        Ok((AgentExecResult {
            task_name: ready.name.clone(), tool: ready.tool.clone(),
            input_summary: ready.input_template.clone(), output,
            duration_ms: start.elapsed().as_millis() as u64,
            tokens: resp.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
        }, has_next))
    }

    fn build_llm(config: &Config) -> OpenAIChat {
        OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024))
    }
}
