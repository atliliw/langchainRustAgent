use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};
use std::collections::{HashSet, HashMap};
use std::sync::Mutex;
use std::time::Instant;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

pub const AVAILABLE_TOOLS: &[(&str, &str)] = &[
    ("llm_query","直接用 LLM 回答"),("web_search","搜索网络"),
    ("code_execute","执行代码"),("read_file","读取文件"),("summarize","总结"),
];

// ── 状态存储 ──
struct BatchState {
    task: String,
    all: Vec<AgentTask>,
    done: Vec<AgentExecResult>,
    completed_names: HashSet<String>,
    start: Instant,
}
static STORE: Mutex<Option<HashMap<String, BatchState>>> = Mutex::new(None);
fn store() -> &'static Mutex<Option<HashMap<String, BatchState>>> { &STORE }

pub struct AgentEngine;

impl AgentEngine {
    /// ── 规划 ──
    pub async fn plan(config: &Config, task: String) -> Result<AgentPlan, GraphDemoError> {
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));
        let tj = AVAILABLE_TOOLS.iter().map(|(n,d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}",n,d)).collect::<Vec<_>>().join(",");
        let p = format!("将任务拆解为2-5个子任务并分配工具。可用工具：[{}] 返回JSON：[{{\"name\":\"子任务名（中文）\",\"description\":\"做什么\",\"tool\":\"工具名\",\"depends_on\":[\"前置\"],\"input_template\":\"需要什么\"}}] 任务：{} 只返回JSON。", tj, task);
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
    pub async fn execute_batch_start(config: &Config, task: String, agent_tasks: Vec<AgentTask>) -> Result<(String, Vec<AgentExecResult>, bool), GraphDemoError> {
        let sid = Uuid::new_v4().to_string();
        let done: HashSet<String> = HashSet::new();
        let batch = Self::ready_batch(&agent_tasks, &done);
        if batch.is_empty() { return Err(GraphDemoError::BuildError("没有可执行的任务".into())); }

        let results = Self::run_batch(config, &task, &batch).await?;
        let completed_names: HashSet<String> = results.iter().map(|r| r.task_name.clone()).collect();

        let has_more = {
            let remaining = Self::ready_batch(&agent_tasks, &completed_names);
            remaining.iter().any(|t| !completed_names.contains(&t.name))
        };

        let all = agent_tasks.clone();
        store().lock().unwrap().get_or_insert_with(HashMap::new).insert(sid.clone(), BatchState {
            task, all, done: results.clone(), completed_names, start: Instant::now(),
        });

        Ok((sid, results, has_more))
    }

    /// ── 下一批 ──
    pub async fn execute_batch_next(config: &Config, sid: &str) -> Result<(Vec<AgentExecResult>, bool), GraphDemoError> {
        let (task, all, done_names) = {
            let g = store().lock().unwrap();
            let m = g.as_ref().unwrap();
            let s = m.get(sid).ok_or_else(|| GraphDemoError::BuildError("session不存在".into()))?;
            (s.task.clone(), s.all.clone(), s.completed_names.clone())
        };

        let batch = Self::ready_batch(&all, &done_names);
        if batch.is_empty() { return Err(GraphDemoError::BuildError("没有更多可执行任务".into())); }

        let results = Self::run_batch(config, &task, &batch).await?;
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

    /// ── 执行一批任务（逐个执行，不用 spawn） ──
    async fn run_batch(config: &Config, task: &str, batch: &[AgentTask]) -> Result<Vec<AgentExecResult>, GraphDemoError> {
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(512));

        let mut results = Vec::new();
        for at in batch {
            let start = Instant::now();
            let p = format!("任务：{}\n子任务：{}\n\n输出结果。", task, at.description);
            let resp = tokio::time::timeout(Duration::from_secs(30), llm.invoke(vec![Message::human(&p)], None)).await;
            match resp {
                Ok(Ok(r)) => results.push(AgentExecResult {
                    task_name: at.name.clone(), tool: String::new(), input_summary: String::new(),
                    output: r.content.chars().take(200).collect(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    tokens: r.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
                }),
                _ => results.push(AgentExecResult {
                    task_name: at.name.clone(), tool: String::new(), input_summary: String::new(),
                    output: "执行失败".into(), duration_ms: 0, tokens: 0,
                }),
            }
        }
        Ok(results)
    }

    pub async fn execute_all_batches(config: &Config, task: String, agent_tasks: Vec<AgentTask>) -> Result<AgentExecResponse, GraphDemoError> {
        let (sid, mut all, _) = Self::execute_batch_start(config, task, agent_tasks).await?;
        loop {
            let (batch, has_more) = Self::execute_batch_next(config, &sid).await?;
            all.extend(batch);
            if !has_more { break; }
        }
        Self::batch_finalize(&sid).await
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
        let plan = Self::plan(config, task.clone()).await?;
        let (sid, mut all, _) = Self::execute_batch_start(config, task, plan.tasks).await?;
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
