use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use crate::stores::QdrantStore;
use langchainrust::langgraph::{
    AgentState, CompiledGraph, MessageEntry, ParallelInvocation,
    StateGraph, StateUpdate, START, END,
};
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};
use sqlx::SqlitePool;
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

// ── SQLite 持久化辅助 ──
async fn ensure_agent_tables(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS agent_sessions (
            session_id TEXT PRIMARY KEY,
            task TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            status TEXT NOT NULL DEFAULT 'running'
        )"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS agent_results (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            task_name TEXT NOT NULL,
            tool TEXT NOT NULL DEFAULT '',
            output TEXT NOT NULL DEFAULT '',
            duration_ms INTEGER NOT NULL DEFAULT 0,
            tokens INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (session_id) REFERENCES agent_sessions(session_id)
        )"
    ).execute(pool).await?;

    Ok(())
}

async fn save_agent_session(pool: &SqlitePool, session_id: &str, task: &str) {
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO agent_sessions (session_id, task, status) VALUES (?, ?, 'running')"
    )
    .bind(session_id)
    .bind(task)
    .execute(pool)
    .await;
}

async fn save_agent_result(pool: &SqlitePool, session_id: &str, r: &AgentExecResult) {
    let _ = sqlx::query(
        "INSERT INTO agent_results (session_id, task_name, tool, output, duration_ms, tokens) VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(session_id)
    .bind(&r.task_name)
    .bind(&r.tool)
    .bind(&r.output)
    .bind(r.duration_ms as i64)
    .bind(r.tokens as i64)
    .execute(pool)
    .await;
}

async fn update_agent_session_status(pool: &SqlitePool, session_id: &str, status: &str) {
    let _ = sqlx::query("UPDATE agent_sessions SET status = ? WHERE session_id = ?")
        .bind(status)
        .bind(session_id)
        .execute(pool)
        .await;
}

// ── 状态存储（保留 session 管理） ──
struct BatchState {
    task: String,
    all: Vec<AgentTask>,
    done: Vec<AgentExecResult>,
    completed_names: HashSet<String>,
    start: Instant,
    rag_context: String,
    review_pending: Vec<AgentTask>,
}
static STORE: Mutex<Option<HashMap<String, BatchState>>> = Mutex::new(None);
fn store() -> &'static Mutex<Option<HashMap<String, BatchState>>> { &STORE }

/// 尝试修复常见 JSON 格式错误
fn fix_json(input: &str) -> String {
    let s = input.trim().to_string();

    // 1. 移除 BOM
    let s = if s.starts_with('\u{feff}') { s[3..].to_string() } else { s };

    // 2. 提取 JSON 边界（跳过前导/后续文本）
    let s = {
        let start = s.find(|c| c == '[' || c == '{');
        let end = s.rfind(|c| c == ']' || c == '}');
        match (start, end) {
            (Some(si), Some(ei)) if si <= ei => s[si..=ei].to_string(),
            _ => return s,
        }
    };

    // 3. 修复 JSON 常见问题
    let mut fixed = s.clone();

    // 3a. 移除尾随逗号：",]" -> "]" 和 ",}" -> "}"
    loop {
        let before = fixed.clone();
        fixed = fixed.replace(",]", "]").replace(",}", "}");
        if fixed == before { break; }
    }

    // 3b. 替换单引号为双引号（但不在字符串值内部简单替换）
    // 简单方法：替换键周围的单引号
    let mut chars: Vec<char> = fixed.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\'' {
            // 检查是否可能是键：前后有 {, , : 等
            let prev_char = if i > 0 { chars[i-1] } else { '{' };
            let next_char = if i + 1 < chars.len() { chars[i+1] } else { '}' };
            if matches!(prev_char, '{' | ',' | ':') || matches!(next_char, ':' | ',' | '}' | ']') {
                chars[i] = '"';
            }
        }
        i += 1;
    }
    fixed = chars.into_iter().collect();

    // 3c. 尝试给未加引号的属性名加引号
    // 匹配模式 {word: 或 ,word: 其中 word 是字母数字
    let mut result = String::new();
    let c2: Vec<char> = fixed.chars().collect();
    let mut j = 0;
    while j < c2.len() {
        // 检查当前位置是否可能是一个未加引号的键的开始
        if (j == 0 || c2[j-1] == '{' || c2[j-1] == ',') && c2[j].is_ascii_alphabetic() {
            // 找到键名结束位置
            let mut k = j;
            while k < c2.len() && (c2[k].is_alphanumeric() || c2[k] == '_') {
                k += 1;
            }
            // 如果后面跟着 :，说明是未加引号的键
            if k < c2.len() && c2[k] == ':' {
                result.push('"');
                result.extend(&c2[j..k]);
                result.push('"');
                j = k;
                continue;
            }
        }
        result.push(c2[j]);
        j += 1;
    }
    fixed = result;

    fixed
}

pub struct AgentEngine;

impl AgentEngine {
    fn parse_tasks(s: &str) -> Result<Vec<AgentTask>, String> {
        serde_json::from_str::<Vec<AgentTask>>(s)
            .map_err(|e| format!("JSON 格式错误 ({}): \n{}", e, s.chars().take(200).collect::<String>()))
    }

    /// ── 规划 ──
    pub async fn plan(config: &Config, task: String, rag_context: String, use_rag: bool, use_routing: bool) -> Result<AgentPlan, GraphDemoError> {
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));
        let tj = AVAILABLE_TOOLS.iter().map(|(n,d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}",n,d)).collect::<Vec<_>>().join(",");

        let routing_section = if use_routing {
            "注意：如果任务需要根据中间结果决定下一步，必须创建 type=decision 的决策节点。\n\
             决策节点不需要 tool，通过 routes 定义走向（key=结果, value=下一个任务）。\n\
             routes 中的所有 value 必须在同一个任务列表中创建对应的子任务，每个子任务用 depends_on 指向决策节点。\n\
             如果任务需要人工审批才能继续，创建 type=human_review 的审核节点。\n\
             示例：调研Go和Python后，需要判断信息是否充分：\n\
                [\n\
                  {\"name\":\"调研Go\",\"tool\":\"rag_search\",\"depends_on\":[],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                  {\"name\":\"调研Python\",\"tool\":\"rag_search\",\"depends_on\":[],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                  {\"name\":\"判断信息是否充分\",\"task_type\":\"decision\",\"depends_on\":[\"调研Go\",\"调研Python\"],\"routes\":{\"充分\":\"写对比\",\"不充分\":\"补充搜索\"},\"input_template\":\"\"},\n\
                  {\"name\":\"补充搜索\",\"tool\":\"web_search\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                  {\"name\":\"写对比\",\"tool\":\"llm_query\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\",\"input_template\":\"\"}\n\
                ]\n"
        } else {
            ""
        };

        let p = if use_rag {
            let rag_context_block = if rag_context.is_empty() {
                String::new()
            } else {
                format!("\n\n知识库检索到以下相关信息：\n{}", rag_context)
            };
            format!(
                "第一个子任务必须是「知识库检索」，使用 rag_search 工具。后续子任务基于知识库检索的结果执行。{rag}\n\
                 将任务拆解为2-5个子任务并分配工具。\n\
                 {routing}\
                 要求：对比类任务必须将A和B拆成独立的搜索任务（depends_on为空），最终汇总任务depends_on所有搜索任务。\n\
                 可用工具：[{tools}]\n\
                 返回JSON：[{{ \"name\": \"子任务名（中文）\", \"description\": \"做什么\", \"tool\": \"工具名\", \"task_type\": \"normal\", \"depends_on\": [\"前置\"], \"input_template\": \"需要什么\" }}]\n\
                 任务：{task}\n\
                 只返回JSON。",
                rag = rag_context_block, routing = routing_section, tools = tj, task = task
            )
        } else {
            format!(
                "将任务拆解为2-5个子任务并分配工具。\n\
                 {routing}\
                 要求：对比类任务必须将A和B拆成独立的搜索任务（depends_on为空），最终汇总任务depends_on所有搜索任务。\n\
                 可用工具：[{tools}]\n\
                 返回JSON：[{{ \"name\": \"子任务名（中文）\", \"description\": \"做什么\", \"tool\": \"工具名\", \"task_type\": \"normal\", \"depends_on\": [\"前置\"], \"input_template\": \"需要什么\" }}]\n\
                 任务：{task}\n\
                 只返回JSON。",
                routing = routing_section, tools = tj, task = task
            )
        };
        let r = llm.invoke(vec![Message::human(&p)], None).await.map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        let c = r.content.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();

        let tasks = match Self::parse_tasks(c) {
            Ok(t) => t,
            Err(first_err) => {
                let fixed = fix_json(c);
                match Self::parse_tasks(&fixed) {
                    Ok(t) => t,
                    Err(_) => {
                        tracing::warn!("JSON 格式错误，尝试让 LLM 修正: {}", first_err);
                        let fix_prompt = format!(
                            "JSON 格式错误：{}\n\n原始内容：\n```\n{}\n```\n\n请修正为合法 JSON，只返回修正后的 JSON。",
                            first_err, c
                        );
                        match llm.invoke(vec![Message::human(&fix_prompt)], None).await {
                            Ok(fix_r) => {
                                let fixed_c = fix_r.content.trim_start_matches("```json")
                                    .trim_start_matches("```").trim_end_matches("```").trim();
                                let fixed2 = fix_json(fixed_c);
                                Self::parse_tasks(&fixed2).map_err(|e| {
                                    GraphDemoError::BuildError(format!("LLM 修正后 JSON 仍无效: {}", e))
                                })?
                            }
                            Err(e) => return Err(GraphDemoError::BuildError(format!("LLM 修正失败: {}", e))),
                        }
                    }
                }
            }
        };

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
                    let task_name = at.name.clone();
                    let tool_name = at.tool.clone();
                    let input_template = at.input_template.clone();
                    tracing::info!(task_name = %task_name, tool = %tool_name, "任务开始执行");

                    let task_start = Instant::now();

                    let handle = tokio::spawn(async move {
                        let llm = OpenAIChat::new(
                            config.to_langchain_openai_config().with_max_tokens(2048)
                        );

                        let (output, tokens) = match at.task_type.as_str() {
                            "human_review" => {
                                (format!("⏸️ 待人工审批：{}", at.description), 0)
                            }
                            // 决策节点：调 LLM 判断，不执行具体任务
                            "decision" => {
                                let ctx_str = if ctx.is_empty() { "无前置结果".to_string() } else {
                                    format!("前置完成的任务结果：\n{}", ctx)
                                };
                                let route_options: Vec<String> = at.routes.keys().map(|k| format!("「{}」", k)).collect();
                                let p = format!(
                                    "基于以下信息做出判断，只返回决策结果（{}），不要多余内容。\n\n{}\n\n当前决策：{}",
                                    route_options.join(" 或 "), ctx_str, at.description
                                );
                                match llm.invoke(vec![Message::human(&p)], None).await {
                                    Ok(r) => {
                                        let t = r.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);
                                        (r.content.clone(), t)
                                    }
                                    Err(e) => ("判断失败".to_string(), 0),
                                }
                            }
                            _ => match at.tool.as_str() {
                            "llm_query" | "" => {
                                let p = if ctx.is_empty() {
                                    format!("任务：{}\n当前子任务：{}\n\n请执行当前子任务并输出结果。{}",
                                        task, at.description, rag)
                                } else {
                                    format!("任务：{}\n当前子任务：{}\n\n前置完成的任务结果：\n{}\n\n请基于前置结果执行当前子任务并输出。{}",
                                        task, at.description, ctx, rag)
                                };
                                let mut last_error = String::new();
                                let mut result = None;
                                for attempt in 1..=3 {
                                    match tokio::time::timeout(Duration::from_secs(120),
                                        llm.invoke(vec![Message::human(&p)], None)
                                    ).await {
                                        Ok(Ok(r)) => {
                                            let t = r.token_usage.as_ref()
                                                .map(|u| u.total_tokens)
                                                .unwrap_or_else(|| estimate_token_usage(&p, &r.content));
                                            result = Some((r.content.clone(), t));
                                            break;
                                        }
                                        Ok(Err(e)) => {
                                            last_error = e.to_string();
                                            tracing::warn!("llm_query attempt {}/3 failed: {}", attempt, last_error);
                                            if attempt < 3 {
                                                tokio::time::sleep(std::time::Duration::from_secs([1, 3, 5][attempt - 1])).await;
                                            }
                                        }
                                        Err(_) => {
                                            last_error = "timeout".to_string();
                                            tracing::warn!("llm_query attempt {}/3 timeout", attempt);
                                            if attempt < 3 {
                                                tokio::time::sleep(std::time::Duration::from_secs([1, 3, 5][attempt - 1])).await;
                                            }
                                        }
                                    }
                                }
                                match result {
                                    Some(r) => r,
                                    None => (format!("执行失败(重试3次后): {}", last_error), 0),
                                }
                            }
                            "web_search" => {
                                let p = format!("任务：{}\n当前子任务：{}\n\n请基于你的知识回答。{}",
                                    task, at.description, rag);
                                let mut last_error = String::new();
                                let mut result = None;
                                for attempt in 1..=3 {
                                    match tokio::time::timeout(Duration::from_secs(60),
                                        llm.invoke(vec![Message::human(&p)], None)
                                    ).await {
                                        Ok(Ok(r)) => {
                                            let t = r.token_usage.as_ref()
                                                .map(|u| u.total_tokens)
                                                .unwrap_or_else(|| estimate_token_usage(&p, &r.content));
                                            result = Some((r.content.clone(), t));
                                            break;
                                        }
                                        Ok(Err(e)) => {
                                            last_error = e.to_string();
                                            tracing::warn!("web_search attempt {}/3 failed: {}", attempt, last_error);
                                            if attempt < 3 {
                                                tokio::time::sleep(std::time::Duration::from_secs([1, 3, 5][attempt - 1])).await;
                                            }
                                        }
                                        Err(_) => {
                                            last_error = "timeout".to_string();
                                            tracing::warn!("web_search attempt {}/3 timeout", attempt);
                                            if attempt < 3 {
                                                tokio::time::sleep(std::time::Duration::from_secs([1, 3, 5][attempt - 1])).await;
                                            }
                                        }
                                    }
                                }
                                match result {
                                    Some(r) => r,
                                    None => (format!("执行失败(重试3次后): {}", last_error), 0),
                                }
                            }
                            "weather" => {
                                let city_prompt = format!("任务：{}\n当前子任务：{}\n\n请输出要查询天气的城市名，不要多余内容。",
                                    task, at.description);
                                let mut last_error = String::new();
                                let mut city_result = None;
                                for attempt in 1..=2 {
                                    match tokio::time::timeout(Duration::from_secs(15),
                                        llm.invoke(vec![Message::human(&city_prompt)], None)
                                    ).await {
                                        Ok(Ok(r)) => {
                                            city_result = Some(r.content.trim().to_string());
                                            break;
                                        }
                                        Ok(Err(e)) => {
                                            last_error = e.to_string();
                                            tracing::warn!("weather city extraction attempt {}/2 failed: {}", attempt, last_error);
                                            if attempt < 2 {
                                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                            }
                                        }
                                        Err(_) => {
                                            last_error = "timeout".to_string();
                                            tracing::warn!("weather city extraction attempt {}/2 timeout", attempt);
                                            if attempt < 2 {
                                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                            }
                                        }
                                    }
                                }
                                let city = match city_result {
                                    Some(c) => c,
                                    None => at.description.clone(),
                                };
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
                                        match tokio::time::timeout(Duration::from_secs(30), store.search_rag(&query, 3)).await {
                                            Ok(Ok(results)) => {
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
                                            Ok(Err(_)) => (format!("知识库搜索失败（搜索词：{}）", query), 0),
                                            Err(_) => (format!("知识库搜索超时（搜索词：{}）", query), 0),
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
                                let mut last_error = String::new();
                                let mut result = None;
                                for attempt in 1..=3 {
                                    match tokio::time::timeout(Duration::from_secs(60),
                                        llm.invoke(vec![Message::human(&p)], None)
                                    ).await {
                                        Ok(Ok(r)) => {
                                            let t = r.token_usage.as_ref()
                                                .map(|u| u.total_tokens)
                                                .unwrap_or_else(|| estimate_token_usage(&p, &r.content));
                                            result = Some((r.content.clone(), t));
                                            break;
                                        }
                                        Ok(Err(e)) => {
                                            last_error = e.to_string();
                                            tracing::warn!("unknown_tool({}) attempt {}/3 failed: {}", at.tool, attempt, last_error);
                                            if attempt < 3 {
                                                tokio::time::sleep(std::time::Duration::from_secs([1, 3, 5][attempt - 1])).await;
                                            }
                                        }
                                        Err(_) => {
                                            last_error = "timeout".to_string();
                                            tracing::warn!("unknown_tool({}) attempt {}/3 timeout", at.tool, attempt);
                                            if attempt < 3 {
                                                tokio::time::sleep(std::time::Duration::from_secs([1, 3, 5][attempt - 1])).await;
                                            }
                                        }
                                    }
                                }
                                match result {
                                    Some(r) => r,
                                    None => (format!("执行失败(重试3次后): {}", last_error), 0),
                                }
                            }
                        }
                    };

                        (output, tokens)
                    });

                    match handle.await {
                        Ok((output, tokens)) => {
                            let elapsed = task_start.elapsed().as_millis() as u64;
                            tracing::info!(task_name = %task_name, duration_ms = elapsed, tokens = tokens, "任务完成");
                            let mut new_state = state;
                            new_state.add_message(MessageEntry::ai(
                                serde_json::json!({
                                    "task": task_name,
                                    "output": output,
                                    "tokens": tokens,
                                    "duration_ms": elapsed,
                                    "tool": tool_name,
                                    "input_summary": input_template,
                                }).to_string()
                            ));
                            Ok(StateUpdate::full(new_state))
                        }
                        Err(e) => {
                            let elapsed = task_start.elapsed().as_millis() as u64;
                            tracing::warn!(task_name = %task_name, error = %e, "任务执行失败");
                            let mut new_state = state;
                            new_state.add_message(MessageEntry::ai(
                                serde_json::json!({
                                    "task": task_name,
                                    "output": format!("任务 panic: {}", e),
                                    "tokens": 0,
                                    "duration_ms": elapsed,
                                    "tool": tool_name,
                                    "input_summary": input_template,
                                }).to_string()
                            ));
                            Ok(StateUpdate::full(new_state))
                        }
                    }
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
    pub async fn execute_batch_start(config: &Config, task: String, agent_tasks: Vec<AgentTask>, rag_context: String, vector_store: Option<Arc<QdrantStore>>, pool: Option<SqlitePool>) -> Result<(String, Vec<AgentExecResult>, bool), GraphDemoError> {
        let sid = Uuid::new_v4().to_string();
        let done: HashSet<String> = HashSet::new();
        let batch = Self::ready_batch(&agent_tasks, &done);
        if batch.is_empty() { return Err(GraphDemoError::BuildError("没有可执行的任务".into())); }

        let results = Self::run_batch_with_framework(config, &task, &batch, &[], &rag_context, vector_store).await?;

        // 路由逻辑：如果本批有决策节点，跳过未选中的分支
        let mut skipped_names: HashSet<String> = HashSet::new();
        let mut skipped_results: Vec<AgentExecResult> = Vec::new();
        for r in &results {
            if let Some(decided) = Self::parse_decision(&r.output) {
                if let Some(task_def) = agent_tasks.iter().find(|t| t.name == r.task_name) {
                    let mut matched = false;
                    for (route_key, next_task) in &task_def.routes {
                        if route_key == &decided {
                            matched = true;
                        } else {
                            Self::skip_downstream(&agent_tasks, next_task, &mut skipped_names, &mut skipped_results);
                        }
                    }
                    if !matched {
                        // 决策结果未匹配任何 route 时不跳过任务，让后续任务正常执行
                        tracing::info!("决策结果「{}」未匹配 route，后续任务继续执行", decided);
                    }
                }
            }
        }

        let review_pending: Vec<AgentTask> = batch.iter()
            .filter(|t| t.task_type == "human_review")
            .cloned()
            .collect();

        let mut completed_names: HashSet<String> = results.iter()
            .filter(|r| !review_pending.iter().any(|p| p.name == r.task_name))
            .map(|r| r.task_name.clone())
            .collect();
        completed_names.extend(skipped_names);

        let mut done_results: Vec<AgentExecResult> = results.iter()
            .filter(|r| !review_pending.iter().any(|p| p.name == r.task_name))
            .cloned()
            .collect();
        done_results.extend(skipped_results);

        if let Some(ref p) = pool {
            let _ = ensure_agent_tables(p).await;
            save_agent_session(p, &sid, &task).await;
            for r in &done_results {
                save_agent_result(p, &sid, r).await;
            }
        }

        let has_more = review_pending.is_empty() && {
            let remaining = Self::ready_batch(&agent_tasks, &completed_names);
            remaining.iter().any(|t| !completed_names.contains(&t.name))
        };

        let all = agent_tasks.clone();
        store().lock().unwrap().get_or_insert_with(HashMap::new).insert(sid.clone(), BatchState {
            task, all, done: done_results.clone(), completed_names, start: Instant::now(), rag_context, review_pending,
        });

        Ok((sid, results, has_more))
    }

    /// ── 下一批 ──
    /// 使用框架 invoke_parallel 并行执行下一批就绪任务
    pub async fn execute_batch_next(config: &Config, sid: &str, vector_store: Option<Arc<QdrantStore>>, pool: Option<SqlitePool>) -> Result<(Vec<AgentExecResult>, bool), GraphDemoError> {
        {
            let g = store().lock().unwrap();
            if let Some(s) = g.as_ref().unwrap().get(sid) {
                if !s.review_pending.is_empty() {
                    return Err(GraphDemoError::BuildError("有待审批的人工审核任务，请先审批".into()));
                }
            }
        }

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

        let new_review_pending: Vec<AgentTask> = batch.iter()
            .filter(|t| t.task_type == "human_review")
            .cloned()
            .collect();

        let has_more;
        {
            let mut g = store().lock().unwrap();
            if let Some(s) = g.as_mut().unwrap().get_mut(sid) {
                for r in &results {
                    let is_review = new_review_pending.iter().any(|p| p.name == r.task_name);
                    if !is_review {
                        s.done.push(r.clone());
                        s.completed_names.insert(r.task_name.clone());
                    }
                }
                // 路由逻辑：决策节点结果出来后，跳过未选中的分支
                for r in &results {
                    if let Some(decided) = Self::parse_decision(&r.output) {
                        if let Some(task_def) = all.iter().find(|t| t.name == r.task_name) {
                            let mut matched = false;
                            for (route_key, next_task) in &task_def.routes {
                                if route_key == &decided {
                                    matched = true;
                                } else {
                                    Self::skip_downstream(&all, next_task, &mut s.completed_names, &mut s.done);
                                }
                            }
                            if !matched {
                                tracing::info!("决策结果「{}」未匹配 route，后续任务继续执行", decided);
                            }
                        }
                    }
                }
                for t in &new_review_pending {
                    if !s.review_pending.iter().any(|p| p.name == t.name) {
                        s.review_pending.push(t.clone());
                    }
                }
            }
            has_more = g.as_ref().unwrap().get(sid)
                .map(|s| s.review_pending.is_empty() && s.completed_names.len() < s.all.len())
                .unwrap_or(false);
        }

        if let Some(ref p) = pool {
            for r in &results {
                if !new_review_pending.iter().any(|t| t.name == r.task_name) {
                    save_agent_result(p, sid, r).await;
                }
            }
            if !has_more && new_review_pending.is_empty() {
                update_agent_session_status(p, sid, "completed").await;
            }
        }

        Ok((results, has_more))
    }

    /// 从决策节点的输出中解析出选中的路由 key
    fn parse_decision(output: &str) -> Option<String> {
        let lower = output.to_lowercase();
        // 按优先级匹配关键词，返回匹配到的原文（与 route key 保持一致）
        let keywords = ["通过", "充分", "enough", "yes", "不通过", "不充分", "not enough", "no", "tech", "general", "other"];
        for keyword in &keywords {
            if lower.contains(keyword) {
                return Some(keyword.to_string());
            }
        }
        // 如果包含中文的"充足"、"足够"等也判定为"充分"
        if lower.contains("充足") || lower.contains("足够") || lower.contains("够") {
            return Some("充分".to_string());
        }
        // 如果包含"不足"、"缺少"等判定为"不充分"
        if lower.contains("不足") || lower.contains("缺少") {
            return Some("不充分".to_string());
        }
        None
    }

    /// 把指定任务及其所有下游任务标记为"跳过"（加入 done）
    fn skip_downstream(tasks: &[AgentTask], start: &str, done: &mut HashSet<String>, results: &mut Vec<AgentExecResult>) {
        if done.contains(start) { return; }
        done.insert(start.to_string());
        results.push(AgentExecResult {
            task_name: start.to_string(),
            tool: String::new(),
            input_summary: String::new(),
            output: "⏭️ 已跳过（决策未选中该路径）".to_string(),
            duration_ms: 0,
            tokens: 0,
        });
        // 递归标记下游
        for task in tasks {
            if task.depends_on.contains(&start.to_string()) {
                Self::skip_downstream(tasks, &task.name, done, results);
            }
        }
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

                // 决策节点：根据 LLM 的决策结果跳过未选中的后续任务
                if let Some(decided) = Self::parse_decision(&r.output) {
                    // 找到这个决策任务对应的 AgentTask
                    if let Some(task_def) = agent_tasks.iter().find(|t| t.name == r.task_name) {
                        let mut matched = false;
                        // 遍历 routes，把非选中的分支对应的后续任务标记为已完成（跳过）
                        for (route_key, next_task) in &task_def.routes {
                            if route_key == &decided {
                                matched = true;
                            } else {
                                // 找到这个 next_task 及其所有下游任务，标记为跳过
                                Self::skip_downstream(&agent_tasks, next_task, &mut done, &mut all_results);
                            }
                        }
                        if !matched {
                            tracing::warn!("决策结果「{}」未匹配任何 route，无对应分支可执行", decided);
                        }
                    }
                }
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
    pub async fn batch_finalize(sid: &str, pool: Option<SqlitePool>) -> Result<AgentExecResponse, GraphDemoError> {
        let state = {
            let g = store().lock().unwrap();
            let m = g.as_ref().unwrap();
            m.get(sid).map(|s| (s.task.clone(), s.done.clone(), s.start))
        };
        store().lock().unwrap().as_mut().unwrap().remove(sid);

        if let Some(ref p) = pool {
            update_agent_session_status(p, sid, "completed").await;
        }

        match state {
            Some((_task, results, start)) => {
                let fa = results.last().map(|r| r.output.clone()).unwrap_or_default();
                let total_tokens: usize = results.iter().map(|r| r.tokens).sum();
                Ok(AgentExecResponse{results, final_answer: fa, total_duration_ms: start.elapsed().as_millis() as u64, total_tokens})
            }
            None => Ok(AgentExecResponse{results:vec![], final_answer:"完成".into(), total_duration_ms:0, total_tokens:0}),
        }
    }

    /// ── 人工审批 ──
    pub async fn approve_review(sid: &str, task_name: &str, approved: bool, feedback: &str) -> Result<(), GraphDemoError> {
        let mut g = store().lock().unwrap();
        let m = g.as_mut().unwrap();
        let s = m.get_mut(sid).ok_or_else(|| GraphDemoError::BuildError("session不存在".into()))?;

        let idx = s.review_pending.iter().position(|t| t.name == task_name)
            .ok_or_else(|| GraphDemoError::BuildError("找不到待审批任务".into()))?;
        let task = s.review_pending.remove(idx);

        if approved {
            s.completed_names.insert(task_name.to_string());
            s.done.push(AgentExecResult {
                task_name: task_name.to_string(),
                tool: "human_review".to_string(),
                input_summary: task.description.clone(),
                output: format!("✅ 人工审批通过。反馈：{}", feedback),
                duration_ms: 0,
                tokens: 0,
            });
        } else {
            s.done.push(AgentExecResult {
                task_name: task_name.to_string(),
                tool: "human_review".to_string(),
                input_summary: task.description.clone(),
                output: format!("❌ 人工审批拒绝。反馈：{}", feedback),
                duration_ms: 0,
                tokens: 0,
            });
            s.completed_names.insert(task_name.to_string());
        }

        Ok(())
    }

    /// ── 兼容旧接口（一次性跑完所有批次） ──
    pub async fn plan_and_execute(config: &Config, task: String) -> Result<AgentExecResponse, GraphDemoError> {
        let plan = Self::plan(config, task.clone(), String::new(), false, false).await?;
        let (sid, mut all, _) = Self::execute_batch_start(config, task, plan.tasks, String::new(), None, None).await?;
        loop {
            let (batch, has_more) = Self::execute_batch_next(config, &sid, None, None).await?;
            all.extend(batch);
            if !has_more { break; }
        }
        Self::batch_finalize(&sid, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(name: &str, deps: Vec<&str>) -> AgentTask {
        AgentTask {
            name: name.to_string(), description: String::new(),
            tool: "llm_query".into(), depends_on: deps.into_iter().map(|s| s.to_string()).collect(),
            input_template: String::new(), task_type: "normal".into(), routes: std::collections::HashMap::new(),
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
            task, all, done, completed_names: completed, start: Instant::now(), rag_context: String::new(), review_pending: Vec::new(),
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
