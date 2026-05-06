use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};
use std::time::Instant;
use tokio::time::{timeout, Duration};

pub const AVAILABLE_TOOLS: &[(&str, &str)] = &[
    ("llm_query","直接用 LLM 回答"),("web_search","搜索网络"),
    ("code_execute","执行代码"),("read_file","读取文件"),("summarize","总结"),
];

pub struct AgentEngine;

impl AgentEngine {
    /// 规划 + 执行（一键完成）
    pub async fn plan_and_execute(config: &Config, task: String) -> Result<AgentExecResponse, GraphDemoError> {
        let total_start = Instant::now();

        // 1. 规划
        let plan = Self::plan(config, task.clone()).await?;

        // 2. 一次 LLM 调用执行所有子任务
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));

        let task_list: String = plan.tasks.iter()
            .map(|t| format!("- {}: {}", t.name, t.description))
            .collect::<Vec<_>>().join("\n");

        let prompt = format!(
            "依次执行以下子任务，为每个输出结果。\n\n原始任务：{task}\n\n子任务：\n{task_list}\n\n返回JSON数组：\n[{{\"name\":\"子任务名\",\"output\":\"执行结果\"}}]\n只返回JSON。",
            task = task, task_list = task_list,
        );

        let exec_future = llm.invoke(vec![Message::human(&prompt)], None);
        let resp = timeout(Duration::from_secs(60), exec_future).await
            .map_err(|_| GraphDemoError::ExecutionError("LLM 超时（>60s）".to_string()))?
            .map_err(|e| GraphDemoError::ExecutionError(format!("执行失败: {}", e)))?;

        let cleaned = resp.content.trim_start_matches("```json").trim_start_matches("```")
            .trim_end_matches("```").trim();

        #[derive(serde::Deserialize)]
        struct TaskOutput { name: String, output: String }
        let all_outputs: Vec<TaskOutput> = serde_json::from_str(cleaned)
            .unwrap_or_else(|_| vec![]);

        let total_tokens = resp.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);

        // 3. 组装结果
        let mut results: Vec<AgentExecResult> = Vec::new();
        for to in &all_outputs {
            if let Some(at) = plan.tasks.iter().find(|t| t.name == to.name) {
                results.push(AgentExecResult {
                    task_name: to.name.clone(), tool: at.tool.clone(),
                    input_summary: at.input_template.clone(), output: to.output.clone(),
                    duration_ms: 0, tokens: 0,
                });
            }
        }

        // 如果任务名不匹配，把 LLM 原始输出作为结果
        if results.is_empty() && !all_outputs.is_empty() {
            for to in &all_outputs {
                results.push(AgentExecResult {
                    task_name: to.name.clone(), tool: String::new(),
                    input_summary: String::new(), output: to.output.clone(),
                    duration_ms: 0, tokens: 0,
                });
            }
        }
        if results.is_empty() {
            results.push(AgentExecResult {
                task_name: "回答".into(), tool: String::new(),
                input_summary: String::new(), output: resp.content.chars().take(500).collect(),
                duration_ms: 0, tokens: total_tokens,
            });
        }

        let final_answer = results.last().map(|r| r.output.clone()).unwrap_or_default();
        Ok(AgentExecResponse {
            results,
            final_answer,
            total_duration_ms: total_start.elapsed().as_millis() as u64,
            total_tokens,
        })
    }

    pub async fn plan(config: &Config, task: String) -> Result<AgentPlan, GraphDemoError> {
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));
        let tj = AVAILABLE_TOOLS.iter().map(|(n,d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}",n,d)).collect::<Vec<_>>().join(",");
        let p = format!("将任务拆解为2-5个子任务并分配工具。可用工具：[{}] 返回JSON：[{{\"name\":\"子任务名（中文）\",\"description\":\"做什么\",\"tool\":\"工具名\",\"depends_on\":[\"前置\"],\"input_template\":\"需要什么\"}}] 任务：{} 只返回JSON。", tj, task);
        let r = llm.invoke(vec![Message::human(&p)], None).await.map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        let c = r.content.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        let tasks: Vec<AgentTask> = serde_json::from_str(c).map_err(|e| GraphDemoError::BuildError(format!("格式错误: {}", e)))?;
        if tasks.is_empty() { return Err(GraphDemoError::BuildError("规划为空".to_string())); }
        let gs = Self::build_graph(&tasks);
        Ok(AgentPlan{original_task:task, tasks, graph_structure:gs})
    }

    fn build_graph(tasks: &[AgentTask]) -> serde_json::Value {
        let nodes: Vec<String> = tasks.iter().map(|t| t.name.clone()).collect();
        let mut edges = vec![];
        for i in 0..tasks.len() {
            if i==0 { edges.push(serde_json::json!({"type":"fixed","source":"__start__","target":tasks[i].name})); }
            if i+1<tasks.len() { edges.push(serde_json::json!({"type":"fixed","source":tasks[i].name,"target":tasks[i+1].name})); }
        }
        for i in 0..tasks.len() {
            for d in &tasks[i].depends_on { if nodes.contains(d) { edges.push(serde_json::json!({"type":"fixed","source":d,"target":tasks[i].name})); } }
            edges.push(serde_json::json!({"type":"fixed","source":tasks[i].name,"target":"__end__"}));
        }
        serde_json::json!({"entry_point":tasks[0].name,"nodes":nodes,"edges":edges,"routers":[]})
    }
}
