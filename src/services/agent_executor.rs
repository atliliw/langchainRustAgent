use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use crate::stores::QdrantStore;
use langchainrust::langgraph::{
    AgentState, CompiledGraph, MessageEntry, ParallelInvocation,
    StateGraph, StateUpdate, START, END,
};
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};
use std::collections::{HashSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::time::Duration;
use uuid::Uuid;

pub const AVAILABLE_TOOLS: &[(&str, &str)] = &[
    ("llm_query","直接用 LLM 回答"),("web_search","搜索网络获取信息"),("weather","查询天气"),
    ("code_execute","执行代码"),("read_file","读取文件"),("summarize","总结"),
    ("rag_search","检索知识库（RAG）获取与任务相关的文档内容"),
];

// ── 天气查询（提取为自由函数，run_batch 和框架版本共用） ──
async fn query_weather(city: &str) -> Result<String, String> {
    let e: String = city.chars().map(|c| match c { 'A'..='Z'|'a'..='z'|'0'..='9'|'-'|'_'|'.'|'~' => c.to_string(), _ => format!("%{:02X}",c as u8) }).collect();
    reqwest::get(&format!("https://wttr.in/{}?format=%C+|+%t+|+%h+|+%w&lang=zh", e)).await
        .map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())
}

/// 当 API 不返回 token_usage 时，用文本长度估算 token 数
fn estimate_token_usage(prompt: &str, response: &str) -> usize {
    let input_tokens = prompt.chars().count() / 2 + 1;
    let output_tokens = response.chars().count() / 2 + 1;
    input_tokens + output_tokens
}

// ── 状态存储（保留 session 管理） ──
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
                "第一个子任务必须是「知识库检索」，使用 rag_search 工具。后续子任务基于知识库检索的结果执行。{rag}\n\
                 将任务拆解为2-5个子任务并分配工具。\n\
                 要求：\n\
                 1. 对比类任务（如比较A和B）必须将A和B拆成独立的搜索/调研子任务，depends_on 为空，以便并行执行\n\
                 2. 每个子任务职责单一，不要合并多个不同的工作\n\
                 3. 最终汇总/分析任务 depends_on 前置的所有搜索任务\n\
                 示例：用户说\"比较go和python\"，应该拆成：\n\
                   [{{\"name\":\"调研Go\", \"depends_on\":[]}}, {{\"name\":\"调研Python\", \"depends_on\":[]}}, {{\"name\":\"对比分析\", \"depends_on\":[\"调研Go\",\"调研Python\"]}}]\n\
                 可用工具：[{tools}]\n\
                 返回JSON格式：[{{ \"name\": \"子任务名（中文）\", \"description\": \"做什么\", \"tool\": \"工具名\", \"depends_on\": [\"前置\"], \"input_template\": \"需要什么\" }}]\n\
                 任务：{task}\n\
                 只返回JSON。name和description必须用中文。",
                rag = rag_context_block, tools = tj, task = task
            )
        } else {
            format!(
                "将任务拆解为2-5个子任务并分配工具。\n\
                 要求：\n\
                 1. 对比类任务（如比较A和B）必须将A和B拆成独立的搜索/调研子任务，depends_on 为空，以便并行执行\n\
                 2. 每个子任务职责单一，不要合并多个不同的工作\n\
                 3. 最终汇总/分析任务 depends_on 前置的所有搜索任务\n\
                 示例：用户说\"比较go和python\"，应该拆成：\n\
                   [{{\"name\":\"调研Go\", \"depends_on\":[]}}, {{\"name\":\"调研Python\", \"depends_on\":[]}}, {{\"name\":\"对比分析\", \"depends_on\":[\"调研Go\",\"调研Python\"]}}]\n\
                 可用工具：[{tools}]\n\
                 返回JSON格式：[{{ \"name\": \"子任务名（中文）\", \"description\": \"做什么\", \"tool\": \"工具名\", \"depends_on\": [\"前置\"], \"input_template\": \"需要什么\" }}]\n\
                 任务：{task}\n\
                 只返回JSON。name和description必须用中文。",
                tools = tj, task = task
            )
        };
        let r = llm.invoke(vec![Message::human(&p)], None).await.map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        let c = r.content.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        let tasks: Vec<AgentTask> = serde_json::from_str(c).map_err(|e| GraphDemoError::BuildError(format!("格式错: {}", e)))?;
        if tasks.is_empty() { return Err(GraphDemoError::BuildError("规划为空".into())); }
        let gs = Self::build_graph(&tasks);
        Ok(AgentPlan{original_task:task, tasks, graph_structure:gs})
    }

    fn build_graph(tasks: &[AgentTask]) -> serde_json::Value {
        let names: HashSet<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
        let nodes: Vec<String> = tasks.iter().map(|t| t.name.clone()).collect();
        let mut edges = vec![];

        // START → 没有依赖的任务（可以直接开始）
        for task in tasks {
            if task.depends_on.iter().all(|d| !names.contains(d.as_str())) {
                edges.push(serde_json::json!({"type":"fixed","source":"__start__","target":task.name}));
            }
        }

        // depends_on 边 + 每个任务到 END
        for task in tasks {
            for d in &task.depends_on {
                if names.contains(d.as_str()) {
                    edges.push(serde_json::json!({"type":"fixed","source":d,"target":task.name}));
                }
            }
            edges.push(serde_json::json!({"type":"fixed","source":task.name,"target":"__end__"}));
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

    // ──────────────────────── 框架集成 ────────────────────────

    /// 用框架构建单层图：dispatch → FanOut(batch) → FanIn → END
    /// vector_store 不为空时，rag_search 工具会用任务名独立搜索（不再共享 rag 参数）
    fn build_batch_graph(
        config: Config,
        task: String,
        batch: Vec<AgentTask>,
        ctx: String,
        rag: String,
        vector_store: Option<Arc<QdrantStore>>,
    ) -> Result<CompiledGraph<AgentState>, GraphDemoError> {
        let mut graph = StateGraph::<AgentState>::new();

        // 空节点，作为 FanOut 的入口
        graph.add_node_fn("__dispatch__", |state| {
            Ok(StateUpdate::full(state.clone()))
        });

        // 每个任务一个 AsyncNode
        for at in &batch {
            let at = at.clone();
            let config = config.clone();
            let task = task.clone();
            let ctx = ctx.clone();
            let rag = rag.clone();
            let vector_store = vector_store.clone();

            graph.add_async_node(at.name.clone(), move |state: &AgentState| {
                let at = at.clone();
                let config = config.clone();
                let task = task.clone();
                let ctx = ctx.clone();
                let rag = rag.clone();
                let state = state.clone();
                let vector_store = vector_store.clone();

                async move {
                    let task_start = Instant::now();
                    let llm = OpenAIChat::new(
                        config.to_langchain_openai_config().with_max_tokens(2048)
                    );

                    let (output, tokens) = match at.tool.as_str() {
                        "llm_query" | "" => {
                            let p = if ctx.is_empty() {
                                format!("任务：{}\n当前子任务：{}\n\n请执行当前子任务并输出结果。{}",
                                    task, at.description, rag)
                            } else {
                                format!("任务：{}\n当前子任务：{}\n\n前置完成的任务结果：\n{}\n\n请基于前置结果执行当前子任务并输出。{}",
                                    task, at.description, ctx, rag)
                            };
                            match tokio::time::timeout(Duration::from_secs(120),
                                llm.invoke(vec![Message::human(&p)], None)
                            ).await {
                                Ok(Ok(r)) => {
                                    let t = r.token_usage.as_ref()
                                        .map(|u| u.total_tokens)
                                        .unwrap_or_else(|| estimate_token_usage(&p, &r.content));
                                    (r.content.clone(), t)
                                }
                                _ => ("执行失败".to_string(), 0),
                            }
                        }
                        "web_search" => {
                            let p = format!("任务：{}\n当前子任务：{}\n\n请基于你的知识回答。{}",
                                task, at.description, rag);
                            match llm.invoke(vec![Message::human(&p)], None).await {
                                Ok(r) => {
                                    let t = r.token_usage.as_ref()
                                        .map(|u| u.total_tokens)
                                        .unwrap_or_else(|| estimate_token_usage(&p, &r.content));
                                    (r.content.clone(), t)
                                }
                                Err(e) => (format!("执行失败: {}", e), 0),
                            }
                        }
                        "weather" => {
                            let city_prompt = format!("任务：{}\n当前子任务：{}\n\n请输出要查询天气的城市名，不要多余内容。",
                                task, at.description);
                            let city = llm.invoke(vec![Message::human(&city_prompt)], None).await
                                .map(|r| r.content.trim().to_string())
                                .unwrap_or_else(|_| at.description.clone());
                            match query_weather(&city).await {
                                Ok(r) => (format!("{}的天气：{}", city, r), 0),
                                Err(e) => (format!("天气查询失败: {}", e), 0),
                            }
                        }
                        "rag_search" => {
                            // 用任务名独立搜索向量库
                            let query = if at.input_template.is_empty() {
                                at.name.clone()
                            } else {
                                at.input_template.clone()
                            };
                            match &vector_store {
                                Some(store) => {
                                    match store.search_rag(&query, 3).await {
                                        Ok(results) => {
                                            let filtered: Vec<_> = results.iter()
                                                .filter(|r| r.score >= 0.3)
                                                .collect();
                                            if filtered.is_empty() {
                                                (format!("知识库中未找到相关文档（搜索词：{}）", query), 0)
                                            } else {
                                                let content: Vec<String> = filtered.iter().map(|r| {
                                                    format!("[相关性 {:.1}%]\n{}", r.score * 100.0, r.document.content)
                                                }).collect();
                                                let embed_tokens = query.chars().count().max(1);
                                                (format!("知识库检索到以下相关信息（搜索词：{}，Embedding 消耗 ~{} tokens）：\n\n{}",
                                                    query, embed_tokens, content.join("\n\n---\n\n")), embed_tokens)
                                            }
                                        }
                                        Err(_) => (format!("知识库搜索失败（搜索词：{}）", query), 0),
                                    }
                                }
                                None => {
                                    // 没有向量库时回退到共享的 rag 参数
                                    if rag.is_empty() {
                                        ("知识库中未找到相关文档".to_string(), 0)
                                    } else {
                                        (format!("知识库检索到以下相关信息：\n\n{}", rag), 0)
                                    }
                                }
                            }
                        }
                        _ => {
                            let p = format!("任务：{}\n子任务：{}\n(工具:{} 不可用，请直接用 LLM 执行)\n\n上下文：\n{}\n\n输出结果。{}",
                                task, at.description, at.tool,
                                if ctx.is_empty() { "无" } else { &ctx }, rag);
                            match llm.invoke(vec![Message::human(&p)], None).await {
                                Ok(r) => {
                                    let t = r.token_usage.as_ref()
                                        .map(|u| u.total_tokens)
                                        .unwrap_or_else(|| estimate_token_usage(&p, &r.content));
                                    (r.content.clone(), t)
                                }
                                Err(e) => (format!("执行失败: {}", e), 0),
                            }
                        }
                    };

                    let elapsed = task_start.elapsed().as_millis() as u64;
                    let mut new_state = state.clone();
                    new_state.add_message(MessageEntry::ai(
                        serde_json::json!({
                            "task": at.name,
                            "output": output,
                            "tokens": tokens,
                            "duration_ms": elapsed,
                            "tool": at.tool,
                            "input_summary": at.input_template,
                        }).to_string()
                    ));
                    Ok(StateUpdate::full(new_state))
                }
            });
        }

        // 边结构：dispatch → FanOut → 每个任务各自到 END
        let names: Vec<String> = batch.iter().map(|t| t.name.clone()).collect();
        graph.add_edge(START, "__dispatch__");
        graph.add_fan_out("__dispatch__", names.clone());
        for name in &names {
            graph.add_edge(name.clone(), END);
        }

        graph.compile().map_err(|e| GraphDemoError::BuildError(e.to_string()))
    }

    /// 从 invoke_parallel 的结果中提取每个任务的输出
    fn extract_results(invocation: &ParallelInvocation<AgentState>) -> Vec<AgentExecResult> {
        let mut results = Vec::new();
        for branch in &invocation.parallel_branches {
            // 每条消息是 JSON：{"task":"...","output":"...","tokens":N,"duration_ms":N}
            for msg in branch.final_state.messages.iter().rev() {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&msg.content) {
                    if parsed.get("task").and_then(|v| v.as_str()).is_some() {
                        results.push(AgentExecResult {
                            task_name: parsed["task"].as_str().unwrap_or("").to_string(),
                            output: parsed["output"].as_str().unwrap_or("").to_string(),
                            tokens: parsed["tokens"].as_u64().unwrap_or(0) as usize,
                            duration_ms: parsed["duration_ms"].as_u64().unwrap_or(0),
                            tool: parsed["tool"].as_str().unwrap_or("").to_string(),
                            input_summary: parsed["input_summary"].as_str().unwrap_or("").to_string(),
                        });
                        break;
                    }
                }
            }
        }
        results
    }

    /// 用框架执行一批任务（构建图 + invoke_parallel）
    async fn run_batch_with_framework(
        config: &Config,
        task: &str,
        batch: &[AgentTask],
        context: &[AgentExecResult],
        rag_context: &str,
        vector_store: Option<Arc<QdrantStore>>,
    ) -> Result<Vec<AgentExecResult>, GraphDemoError> {
        let ctx: String = context.iter()
            .map(|r| format!("【{}】\n{}", r.task_name, r.output))
            .collect::<Vec<_>>().join("\n\n");
        let rag = if rag_context.is_empty() {
            String::new()
        } else {
            format!("\n\n知识库检索结果：\n{}", rag_context)
        };

        let compiled = Self::build_batch_graph(
            config.clone(),
            task.to_string(),
            batch.to_vec(),
            ctx,
            rag,
            vector_store,
        )?;

        let initial = AgentState::new(task.to_string());
        let invocation = compiled.invoke_parallel(initial).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;

        Ok(Self::extract_results(&invocation))
    }

    /// ── 开始执行（返回第一批结果 + session_id） ──
    /// 使用框架 invoke_parallel 并行执行第一批就绪任务
    pub async fn execute_batch_start(config: &Config, task: String, agent_tasks: Vec<AgentTask>, rag_context: String, vector_store: Option<Arc<QdrantStore>>) -> Result<(String, Vec<AgentExecResult>, bool), GraphDemoError> {
        let sid = Uuid::new_v4().to_string();
        let done: HashSet<String> = HashSet::new();
        let batch = Self::ready_batch(&agent_tasks, &done);
        if batch.is_empty() { return Err(GraphDemoError::BuildError("没有可执行的任务".into())); }

        let results = Self::run_batch_with_framework(config, &task, &batch, &[], &rag_context, vector_store).await?;
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
    /// 使用框架 invoke_parallel 并行执行下一批就绪任务
    pub async fn execute_batch_next(config: &Config, sid: &str, vector_store: Option<Arc<QdrantStore>>) -> Result<(Vec<AgentExecResult>, bool), GraphDemoError> {
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
        let results = Self::run_batch_with_framework(config, &task, &batch, &context, &rag_context, vector_store).await?;
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

    /// ── 按依赖分批并行执行：同批任务 invoke_parallel 并行，不同批串行 ──
    /// 例: A → [B, C] → D  => 第一批: A, 第二批: B+C(并行), 第三批: D
    pub async fn execute_all_batches(config: &Config, task: String, agent_tasks: Vec<AgentTask>, rag_context: String, vector_store: Option<Arc<QdrantStore>>) -> Result<AgentExecResponse, GraphDemoError> {
        let total_start = Instant::now();

        let mut done: HashSet<String> = HashSet::new();
        let mut all_results: Vec<AgentExecResult> = Vec::new();

        while done.len() < agent_tasks.len() {
            let batch = Self::ready_batch(&agent_tasks, &done);
            if batch.is_empty() {
                break;
            }

            let results = Self::run_batch_with_framework(config, &task, &batch, &all_results, &rag_context, vector_store.clone()).await?;

            for r in &results {
                done.insert(r.task_name.clone());
                all_results.push(r.clone());
            }
        }

        let total_duration = total_start.elapsed().as_millis() as u64;
        let total_tokens: usize = all_results.iter().map(|r| r.tokens).sum();
        let fa = all_results.last().map(|r| r.output.clone()).unwrap_or_default();

        Ok(AgentExecResponse {
            results: all_results,
            final_answer: fa,
            total_duration_ms: total_duration,
            total_tokens,
        })
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
        let (sid, mut all, _) = Self::execute_batch_start(config, task, plan.tasks, String::new(), None).await?;
        loop {
            let (batch, has_more) = Self::execute_batch_next(config, &sid, None).await?;
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
            task, all, done, completed_names: completed, start: Instant::now(), rag_context: String::new(),
        });

        // 验证存储和读取
        let g = store().lock().unwrap();
        let m = g.as_ref().unwrap();
        let s = m.get(&sid).unwrap();
        assert_eq!(s.completed_names.len(), 1);
        assert!(s.completed_names.contains("A"));
    }

    // ──────── 框架集成测试 ────────

    /// 测试 build_batch_graph 能正确编译不同大小的批次
    #[test]
    fn test_build_batch_graph_compiles() {
        let config = Config::load().expect("需要 config.toml 才能运行此测试");

        // 单任务
        let batch1 = vec![make_task("A", vec![])];
        let graph1 = AgentEngine::build_batch_graph(config.clone(), "test".into(), batch1, String::new(), String::new(), None);
        assert!(graph1.is_ok(), "单任务批次应该编译成功");
        let compiled = graph1.unwrap();
        let nodes = compiled.node_names();
        assert!(nodes.contains(&"A".to_string()));
        assert!(nodes.contains(&"__dispatch__".to_string()));

        // 多任务
        let batch2 = vec![make_task("A", vec![]), make_task("B", vec![]), make_task("C", vec![])];
        let graph2 = AgentEngine::build_batch_graph(config, "test".into(), batch2, String::new(), String::new(), None);
        assert!(graph2.is_ok(), "多任务批次应该编译成功");
        let compiled2 = graph2.unwrap();
        assert_eq!(compiled2.node_names().len(), 4); // dispatch + A + B + C
        assert_eq!(compiled2.entry_point(), "__dispatch__");
    }

    /// 测试 extract_results 能从 ParallelInvocation 正确提取结果
    #[test]
    fn test_extract_results_parses_correctly() {
        use langchainrust::langgraph::ParallelBranch;

        // 构造一个模拟的 ParallelInvocation
        let mut branch_a = AgentState::new("test".into());
        branch_a.add_message(MessageEntry::ai(
            serde_json::json!({
                "task": "任务A",
                "output": "结果A",
                "tokens": 100,
                "duration_ms": 500,
                "tool": "llm_query",
                "input_summary": "",
            }).to_string()
        ));
        branch_a.set_output("结果A".into());

        let mut branch_b = AgentState::new("test".into());
        branch_b.add_message(MessageEntry::ai(
            serde_json::json!({
                "task": "任务B",
                "output": "结果B",
                "tokens": 200,
                "duration_ms": 800,
                "tool": "llm_query",
                "input_summary": "",
            }).to_string()
        ));
        branch_b.set_output("结果B".into());

        let invocation = ParallelInvocation {
            final_state: AgentState::new("test".into()),
            steps: vec![],
            recursion_count: 1,
            parallel_branches: vec![
                ParallelBranch {
                    name: "任务A".into(),
                    final_state: branch_a,
                    steps: vec![],
                },
                ParallelBranch {
                    name: "任务B".into(),
                    final_state: branch_b,
                    steps: vec![],
                },
            ],
        };

        let results = AgentEngine::extract_results(&invocation);
        assert_eq!(results.len(), 2, "应该提取出两个任务的结果");

        let result_a = results.iter().find(|r| r.task_name == "任务A").unwrap();
        assert_eq!(result_a.output, "结果A");
        assert_eq!(result_a.tokens, 100);
        assert_eq!(result_a.duration_ms, 500);

        let result_b = results.iter().find(|r| r.task_name == "任务B").unwrap();
        assert_eq!(result_b.output, "结果B");
        assert_eq!(result_b.tokens, 200);
        assert_eq!(result_b.duration_ms, 800);
    }

    /// 测试 ready_batch + run_batch_with_framework 的集成
    /// （不实际调 LLM，只验证框架路径能正确编译）
    #[test]
    fn test_framework_integration_compile_only() {
        let tasks = vec![
            make_task("A", vec![]),
            make_task("B", vec![]),
        ];

        // 验证分批逻辑
        let done: HashSet<String> = HashSet::new();
        let batch = AgentEngine::ready_batch(&tasks, &done);
        assert_eq!(batch.len(), 2, "无依赖任务应该全部就绪");

        // 验证 build_batch_graph 能处理这批任务
        let config = Config::load().expect("需要 config.toml 才能运行此测试");
        let graph = AgentEngine::build_batch_graph(
            config,
            "test".into(),
            batch,
            String::new(),
            String::new(),
            None,
        );
        assert!(graph.is_ok(), "批次图应该编译成功");
    }
}
