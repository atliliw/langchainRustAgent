use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};
use std::collections::{HashSet, HashMap};
use std::sync::Mutex;
use std::time::Instant;
use tokio::time::Duration;
use uuid::Uuid;

pub const AVAILABLE_TOOLS: &[(&str, &str)] = &[
    ("llm_query","直接用 LLM 回答"),("web_search","搜索网络获取信息"),("weather","查询天气"),
    ("code_execute","执行代码"),("read_file","读取文件"),("summarize","总结"),
    ("rag_search","检索知识库（RAG）获取与任务相关的文档内容"),
];

// ── 状态存储 ──
struct BatchState {
    task: String,
    all: Vec<AgentTask>,
    done: Vec<AgentExecResult>,
    completed_names: HashSet<String>,
    start: Instant,
    rag_context: String,
}
static STORE: Mutex<Option<HashMap<String, BatchState>>> = Mutex::new(None);
fn store() -> &'static Mutex<Option<HashMap<String, BatchState>>> { &STORE }

pub struct AgentEngine;

impl AgentEngine {
    /// ── 规划 ──
    pub async fn plan(config: &Config, task: String, rag_context: String, use_rag: bool) -> Result<AgentPlan, GraphDemoError> {
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));
        let tj = AVAILABLE_TOOLS.iter().map(|(n,d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}",n,d)).collect::<Vec<_>>().join(",");
        let p = if use_rag {
            let rag_context_block = if rag_context.is_empty() {
                String::new()
            } else {
                format!("\n\n知识库检索到以下相关信息：\n{}", rag_context)
            };
            format!(
                "第一个子任务必须是「知识库检索」，使用 rag_search 工具。后续子任务基于知识库检索的结果执行。{rag}\n将任务拆解为2-5个子任务并分配工具。可用工具：[{tools}] 返回JSON：[{{ \"name\": \"子任务名（中文）\", \"description\": \"做什么\", \"tool\": \"工具名\", \"depends_on\": [\"前置\"], \"input_template\": \"需要什么\" }}] 任务：{task} 只返回JSON。name和description必须用中文。",
                rag = rag_context_block, tools = tj, task = task
            )
        } else {
            format!("将任务拆解为2-5个子任务并分配工具。可用工具：[{tools}] 返回JSON：[{{ \"name\": \"子任务名（中文）\", \"description\": \"做什么\", \"tool\": \"工具名\", \"depends_on\": [\"前置\"], \"input_template\": \"需要什么\" }}] 任务：{task} 只返回JSON。name和description必须用中文。", tools = tj, task = task)
        };
        let r = llm.invoke(vec![Message::human(&p)], None).await.map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        let c = r.content.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        let tasks: Vec<AgentTask> = serde_json::from_str(c).map_err(|e| GraphDemoError::BuildError(format!("格式错: {}", e)))?;
        if tasks.is_empty() { return Err(GraphDemoError::BuildError("规划为空".into())); }
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

    // ── 找出当前批就绪任务（所有依赖已满足的） ──
    fn ready_batch(tasks: &[AgentTask], done: &HashSet<String>) -> Vec<AgentTask> {
        let names: HashSet<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
        tasks.iter().filter(|t| {
            if done.contains(&t.name) { return false; }
            t.depends_on.iter()
                .filter(|d| names.contains(d.as_str()))
                .all(|d| done.contains(d))
        }).cloned().collect()
    }

    /// ── 开始执行（返回第一批结果 + session_id） ──
    pub async fn execute_batch_start(config: &Config, task: String, agent_tasks: Vec<AgentTask>, rag_context: String) -> Result<(String, Vec<AgentExecResult>, bool), GraphDemoError> {
        let sid = Uuid::new_v4().to_string();
        let done: HashSet<String> = HashSet::new();
        let batch = Self::ready_batch(&agent_tasks, &done);
        if batch.is_empty() { return Err(GraphDemoError::BuildError("没有可执行的任务".into())); }

        let results = Self::run_batch(config, &task, &batch, &[], &rag_context).await?;
        let completed_names: HashSet<String> = results.iter().map(|r| r.task_name.clone()).collect();

        let has_more = {
            let remaining = Self::ready_batch(&agent_tasks, &completed_names);
            remaining.iter().any(|t| !completed_names.contains(&t.name))
        };

        let all = agent_tasks.clone();
        store().lock().unwrap().get_or_insert_with(HashMap::new).insert(sid.clone(), BatchState {
            task, all, done: results.clone(), completed_names, start: Instant::now(), rag_context,
        });

        Ok((sid, results, has_more))
    }

    /// ── 下一批 ──
    pub async fn execute_batch_next(config: &Config, sid: &str) -> Result<(Vec<AgentExecResult>, bool), GraphDemoError> {
        let (task, all, done_names, rag_context) = {
            let g = store().lock().unwrap();
            let m = g.as_ref().unwrap();
            let s = m.get(sid).ok_or_else(|| GraphDemoError::BuildError("session不存在".into()))?;
            (s.task.clone(), s.all.clone(), s.completed_names.clone(), s.rag_context.clone())
        };

        let batch = Self::ready_batch(&all, &done_names);
        if batch.is_empty() { return Err(GraphDemoError::BuildError("没有更多可执行任务".into())); }

        let context = {
            let g = store().lock().unwrap();
            g.as_ref().unwrap().get(sid).map(|s| s.done.clone()).unwrap_or_default()
        };
        let results = Self::run_batch(config, &task, &batch, &context, &rag_context).await?;
        let has_more;
        {
            let mut g = store().lock().unwrap();
            if let Some(s) = g.as_mut().unwrap().get_mut(sid) {
                for r in &results {
                    s.done.push(r.clone());
                    s.completed_names.insert(r.task_name.clone());
                }
            }
            has_more = g.as_ref().unwrap().get(sid).map(|s| s.completed_names.len() < s.all.len()).unwrap_or(false);
        }
        Ok((results, has_more))
    }

    /// ── 执行一批任务，传上下文 ──
    async fn run_batch(config: &Config, task: &str, batch: &[AgentTask], context: &[AgentExecResult], rag_context: &str) -> Result<Vec<AgentExecResult>, GraphDemoError> {
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(512));
        let ctx: String = context.iter().map(|r| format!("【{}】\n{}", r.task_name, r.output)).collect::<Vec<_>>().join("\n\n");
        let rag_section = if rag_context.is_empty() { String::new() } else { format!("\n\n知识库检索结果：\n{}", rag_context) };

        // 天气查询（用 wttr.in，免费无需 Key）
        async fn query_weather(city: &str) -> Result<String, String> {
            let e: String = city.chars().map(|c| match c { 'A'..='Z'|'a'..='z'|'0'..='9'|'-'|'_'|'.'|'~' => c.to_string(), _ => format!("%{:02X}",c as u8) }).collect();
            reqwest::get(&format!("https://wttr.in/{}?format=%C+|+%t+|+%h+|+%w&lang=zh", e)).await
                .map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())
        }

        let mut results = Vec::new();
        for at in batch {
            let start = Instant::now();

            let (output, tokens) = match at.tool.as_str() {
                "web_search" => {
                    let p = format!("任务：{}\n当前子任务：{}\n\n请基于你的知识回答。{}", task, at.description, rag_section);
                    let resp = llm.invoke(vec![Message::human(&p)], None).await;
                    match resp {
                        Ok(r) => (r.content.clone(), r.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0)),
                        Err(e) => (format!("执行失败: {}", e), 0),
                    }
                }
                "weather" => {
                    let city_prompt = format!("任务：{}\n当前子任务：{}\n\n请输出要查询天气的城市名，不要多余内容。", task, at.description);
                    let city = llm.invoke(vec![Message::human(&city_prompt)], None).await
                        .map(|r| r.content.trim().to_string())
                        .unwrap_or_else(|_| at.description.clone());
                    match query_weather(&city).await {
                        Ok(r) => (format!("{}的天气：{}", city, r), 0),
                        Err(e) => (format!("天气查询失败: {}", e), 0),
                    }
                }
                "rag_search" => {
                    if rag_context.is_empty() {
                        ("知识库中未找到相关文档，请基于自身知识回答。".to_string(), 0)
                    } else {
                        (format!("知识库检索到以下相关信息：\n\n{}", rag_context), 0)
                    }
                }
                "llm_query" | "" => {
                    let p = format!("任务：{}\n当前子任务：{}\n\n前置完成的任务结果：\n{}\n\n请基于前置结果执行当前子任务并输出。{}", task, at.description, if ctx.is_empty() { "无" } else { &ctx }, rag_section);
                    let resp = tokio::time::timeout(Duration::from_secs(120), llm.invoke(vec![Message::human(&p)], None)).await;
                    match resp {
                        Ok(Ok(r)) => {
                            let t = r.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);
                            (r.content.clone(), t)
                        }
                        _ => ("执行失败".to_string(), 0),
                    }
                }
                _ => {
                    let p = format!("任务：{}\n子任务：{}\n(工具:{} 不可用，请直接用 LLM 执行)\n\n上下文：\n{}\n\n输出结果。{}", task, at.description, at.tool, if ctx.is_empty() { "无" } else { &ctx }, rag_section);
                    let resp = llm.invoke(vec![Message::human(&p)], None).await;
                    match resp {
                        Ok(r) => {
                            let t = r.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);
                            (r.content.clone(), t)
                        }
                        Err(e) => (format!("执行失败: {}", e), 0),
                    }
                }
            };

            results.push(AgentExecResult {
                task_name: at.name.clone(), tool: at.tool.clone(), input_summary: at.input_template.clone(),
                output, duration_ms: start.elapsed().as_millis() as u64, tokens,
            });
        }
        Ok(results)
    }

    pub async fn execute_all_batches(config: &Config, task: String, agent_tasks: Vec<AgentTask>, rag_context: String) -> Result<AgentExecResponse, GraphDemoError> {
        let total_start = Instant::now();
        let rag_section = if rag_context.is_empty() { String::new() } else { format!("\n\n知识库检索结果：\n{}", rag_context) };
        // 一次性调 LLM 执行所有子任务，避免 N 次串行调用
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));
        let task_list: String = agent_tasks.iter()
            .map(|t| format!("- {}：{}", t.name, t.description))
            .collect::<Vec<_>>().join("\n");
        let prompt = format!(
            "依次执行以下子任务，为每个输出结果。\n\n原始任务：{task}\n\n子任务：\n{task_list}{rag}\n\n返回JSON数组：\n[{{\"name\":\"子任务名\",\"output\":\"结果\"}}]\n只返回JSON。",
            task = task, task_list = task_list, rag = rag_section,
        );
        let resp = llm.invoke(vec![Message::human(&prompt)], None).await
            .map_err(|e| GraphDemoError::ExecutionError(format!("执行失败: {}", e)))?;
        let cleaned = resp.content.trim_start_matches("```json").trim_start_matches("```")
            .trim_end_matches("```").trim();
        #[derive(serde::Deserialize)] struct TaskOut { name: String, output: String }
        let outputs: Vec<TaskOut> = serde_json::from_str(cleaned).unwrap_or_default();
        let total_tok = resp.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);
        let per_tok = total_tok / std::cmp::max(1, agent_tasks.len());
        let per_ms = total_start.elapsed().as_millis() as u64 / std::cmp::max(1, agent_tasks.len() as u64);

        let results: Vec<AgentExecResult> = agent_tasks.iter().enumerate().map(|(i, at)| {
            let out = outputs.get(i).map(|o| o.output.clone()).unwrap_or_default();
            AgentExecResult { task_name: at.name.clone(), tool: at.tool.clone(), input_summary: at.input_template.clone(), output: out, duration_ms: per_ms, tokens: per_tok }
        }).collect();

        let fa = results.last().map(|r| r.output.clone()).unwrap_or_default();
        Ok(AgentExecResponse{results, final_answer: fa, total_duration_ms: total_start.elapsed().as_millis() as u64, total_tokens: total_tok})
    }

    /// ── 完成验证 ──
    pub async fn batch_finalize(sid: &str) -> Result<AgentExecResponse, GraphDemoError> {
        let state = {
            let g = store().lock().unwrap();
            let m = g.as_ref().unwrap();
            m.get(sid).map(|s| (s.task.clone(), s.done.clone(), s.start))
        };
        store().lock().unwrap().as_mut().unwrap().remove(sid);
        match state {
            Some((_task, results, start)) => {
                let fa = results.last().map(|r| r.output.clone()).unwrap_or_default();
                let total_tokens: usize = results.iter().map(|r| r.tokens).sum();
                Ok(AgentExecResponse{results, final_answer: fa, total_duration_ms: start.elapsed().as_millis() as u64, total_tokens})
            }
            None => Ok(AgentExecResponse{results:vec![], final_answer:"完成".into(), total_duration_ms:0, total_tokens:0}),
        }
    }

    /// ── 兼容旧接口（一次性跑完所有批次） ──
    pub async fn plan_and_execute(config: &Config, task: String) -> Result<AgentExecResponse, GraphDemoError> {
        let plan = Self::plan(config, task.clone(), String::new(), false).await?;
        let (sid, mut all, _) = Self::execute_batch_start(config, task, plan.tasks, String::new()).await?;
        loop {
            let (batch, has_more) = Self::execute_batch_next(config, &sid).await?;
            all.extend(batch);
            if !has_more { break; }
        }
        Self::batch_finalize(&sid).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(name: &str, deps: Vec<&str>) -> AgentTask {
        AgentTask {
            name: name.to_string(), description: String::new(),
            tool: "llm_query".into(), depends_on: deps.into_iter().map(|s| s.to_string()).collect(),
            input_template: String::new(),
        }
    }

    #[test]
    fn test_ready_batch_all_independent() {
        let tasks = vec![make_task("A", vec![]), make_task("B", vec![]), make_task("C", vec![])];
        let done = HashSet::new();
        let batch = AgentEngine::ready_batch(&tasks, &done);
        assert_eq!(batch.len(), 3, "所有无依赖任务都应该就绪");
    }

    #[test]
    fn test_ready_batch_with_deps() {
        let tasks = vec![make_task("A", vec![]), make_task("B", vec!["A"]), make_task("C", vec!["B"])];
        let done: HashSet<String> = ["A"].into_iter().map(|s| s.to_string()).collect();
        let batch = AgentEngine::ready_batch(&tasks, &done);
        assert_eq!(batch.len(), 1, "只有B就绪（A已完成，C依赖B）");
        assert_eq!(batch[0].name, "B");
    }

    #[test]
    fn test_ready_batch_none_ready() {
        let tasks = vec![make_task("A", vec!["B"]), make_task("B", vec!["A"])];
        let done = HashSet::new();
        let batch = AgentEngine::ready_batch(&tasks, &done);
        assert_eq!(batch.len(), 0, "循环依赖，没有就绪任务");
    }

    #[test]
    fn test_ready_batch_some_done() {
        let tasks = vec![make_task("A", vec![]), make_task("B", vec!["A"]), make_task("C", vec![])];
        let done: HashSet<String> = ["A"].into_iter().map(|s| s.to_string()).collect();
        let batch = AgentEngine::ready_batch(&tasks, &done);
        assert_eq!(batch.len(), 2, "C（无依赖）+ B（A已就绪）= 2");
        assert!(batch.iter().any(|t| t.name == "B"));
        assert!(batch.iter().any(|t| t.name == "C"));
    }

    #[test]
    fn test_build_graph_structure() {
        let tasks = vec![make_task("A", vec![]), make_task("B", vec!["A"]), make_task("C", vec![])];
        let g = AgentEngine::build_graph(&tasks);
        let nodes = g["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0], "A");
        let edges = g["edges"].as_array().unwrap();
        assert!(edges.iter().any(|e| e["source"] == "A" && e["target"] == "B"));
    }

    #[test]
    fn test_batch_state_store_and_load() {
        let task = "test".to_string();
        let all = vec![make_task("A", vec![]), make_task("B", vec!["A"])];
        let done = vec![AgentExecResult {
            task_name: "A".into(), tool: String::new(), input_summary: String::new(),
            output: "ok".into(), duration_ms: 0, tokens: 0,
        }];
        let completed: HashSet<String> = ["A"].into_iter().map(|s| s.to_string()).collect();

        let sid = uuid::Uuid::new_v4().to_string();
        store().lock().unwrap().get_or_insert_with(HashMap::new).insert(sid.clone(), BatchState {
            task, all, done, completed_names: completed, start: Instant::now(),
        });

        // 验证存储和读取
        let g = store().lock().unwrap();
        let m = g.as_ref().unwrap();
        let s = m.get(&sid).unwrap();
        assert_eq!(s.completed_names.len(), 1);
        assert!(s.completed_names.contains("A"));
    }
}
