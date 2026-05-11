use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use crate::services::tools::{ToolRegistry, ToolContext};
use crate::services::verify::{CompositeVerifyHook, VerifyHook};
use crate::stores::QdrantStore;
use langchainrust::langgraph::{
    AgentState, CompiledGraph, MessageEntry, ParallelInvocation,
    StateGraph, StateUpdate, START, END,
};
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};
use sqlx::SqlitePool;
use std::collections::{HashSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{broadcast, Semaphore};
use uuid::Uuid;

// ── SQLite 持久化辅助 ──
async fn ensure_agent_tables(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS agent_sessions (
            session_id TEXT PRIMARY KEY,
            task TEXT NOT NULL,
            plan_json TEXT NOT NULL DEFAULT '[]',
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

async fn save_agent_session(pool: &SqlitePool, session_id: &str, task: &str, plan_json: &str) {
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO agent_sessions (session_id, task, plan_json, status) VALUES (?, ?, ?, 'running')"
    )
    .bind(session_id)
    .bind(task)
    .bind(plan_json)
    .execute(pool)
    .await;
}

/// 服务启动时恢复未完成的 session
pub async fn recover_sessions(pool: &SqlitePool) -> Vec<serde_json::Value> {
    let _ = ensure_agent_tables(pool).await;
    let mut recovered = Vec::new();

    // 查出所有 running 状态的 session
    let rows = match sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT session_id, task, plan_json, created_at FROM agent_sessions WHERE status = 'running'"
    )
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(_) => return recovered,
    };

    for (sid, task, plan_json, created_at) in rows {
        // 反序列化 plan，重建 BatchState
        let tasks: Vec<AgentTask> = serde_json::from_str(&plan_json).unwrap_or_default();

        // 读取已完成的 results
        let results = match sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT task_name, tool, output, duration_ms, tokens FROM agent_results WHERE session_id = ? ORDER BY id ASC"
        )
        .bind(&sid)
        .fetch_all(pool)
        .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        let done: Vec<AgentExecResult> = results.into_iter().map(|(name, tool, output, dur, tok)| {
            AgentExecResult {
                task_name: name,
                tool,
                output,
                duration_ms: dur as u64,
                tokens: tok as usize,
                input_summary: String::new(),
                verify_retries: 0,
            }
        }).collect();

        let completed_names: HashSet<String> = done.iter().map(|r| r.task_name.clone()).collect();
        let cancel_flag = get_cancel_flag(&sid);

        let state = BatchState {
            task: task.clone(),
            all: tasks,
            done: done.clone(),
            completed_names,
            start: Instant::now(),
            rag_context: String::new(),
            review_pending: Vec::new(),
            cancel: cancel_flag,
            progress_tx: None,
            use_verify: false,
        };

        // 标记已取消（不自动恢复执行，避免意外）
        let _ = sqlx::query("UPDATE agent_sessions SET status = 'interrupted' WHERE session_id = ?")
            .bind(&sid)
            .execute(pool)
            .await;

        store().lock().unwrap().get_or_insert_with(HashMap::new).insert(sid.clone(), state);
        recovered.push(serde_json::json!({
            "session_id": sid,
            "task": task,
            "created_at": created_at,
            "completed_tasks": done.len(),
            "status": "interrupted",
        }));
    }

    recovered
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

// ── 取消信号存储 ──
static CANCELLATION: Mutex<Option<HashMap<String, Arc<AtomicBool>>>> = Mutex::new(None);
fn cancel_store() -> &'static Mutex<Option<HashMap<String, Arc<AtomicBool>>>> { &CANCELLATION }

fn set_cancelled(sid: &str) {
    if let Ok(mut g) = cancel_store().lock() {
        if let Some(ref mut m) = *g {
            if let Some(flag) = m.get(sid) {
                flag.store(true, Ordering::Relaxed);
                tracing::info!(session_id = %sid, "Agent 任务已标记取消");
            }
        }
    }
}

fn get_cancel_flag(sid: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut g) = cancel_store().lock() {
        let m = g.get_or_insert_with(HashMap::new);
        m.entry(sid.to_string()).or_insert_with(|| flag.clone());
    }
    flag
}

fn remove_cancel_flag(sid: &str) {
    if let Ok(mut g) = cancel_store().lock() {
        if let Some(ref mut m) = *g {
            m.remove(sid);
        }
    }
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
    #[allow(dead_code)]
    cancel: Arc<AtomicBool>,
    progress_tx: Option<broadcast::Sender<String>>,
    use_verify: bool,
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
        let registry = ToolRegistry::default_registry();
        let tj = registry.list_descriptions().iter()
            .map(|(n,d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}", n, d))
            .collect::<Vec<_>>().join(",");

        let routing_section = if use_routing {
            "注意：如果任务需要根据中间结果决定下一步，必须创建 type=decision 的决策节点。\n\
             决策节点不需要 tool，通过 routes 定义走向（key=结果, value=[任务1, 任务2, ...]）。\n\
             routes 中每个 value 是任务名数组，只有数组里的任务才会受路由控制。\n\
             不选中的路由上的所有任务会被自动跳过（包括它们下游的任务）。\
             不在任何路由里的任务不受影响，依赖满足就会正常执行。\n\
             规则：只把「需要条件判断才执行」的任务放路由里，始终要执行的（如最终汇总）不要放路由里。\n\
             如果任务需要人工审批才能继续，创建 type=human_review 的审核节点。\n\
             正确的示例：调研Go和Python后，判断信息是否充分：\n\
                 [\n\
                   {\"name\":\"调研Go\",\"tool\":\"rag_search\",\"depends_on\":[],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                   {\"name\":\"调研Python\",\"tool\":\"rag_search\",\"depends_on\":[],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                   {\"name\":\"判断信息是否充分\",\"task_type\":\"decision\",\"depends_on\":[\"调研Go\",\"调研Python\"],\"routes\":{\"充分\":[],\"不充分\":[\"补充搜索Go\",\"补充搜索Python\"]},\"input_template\":\"\"},\n\
                   {\"name\":\"补充搜索Go\",\"tool\":\"web_search\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                   {\"name\":\"补充搜索Python\",\"tool\":\"web_search\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                   {\"name\":\"写对比\",\"tool\":\"llm_query\",\"depends_on\":[\"判断信息是否充分\",\"补充搜索Go\",\"补充搜索Python\"],\"task_type\":\"normal\",\"input_template\":\"\"}\n\
                 ]\n\
             重要：汇总任务不要放在路由里，这样无论如何都会执行。\n\
             决策为充分时补充搜索被自动跳过，汇总任务依然正常执行。\n\
             决策为不充分时补充搜索正常执行，汇总等待补充搜索完成后执行。\n"
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
                "先用 rag_search 从知识库检索相关信息，然后用其他工具执行剩余任务。\n\
                 rag_search 可以创建 2-3 个，每个覆盖一个主要方面，不要拆太细。\n\
                 例如对比Go和Python：拆成「搜索Go核心特性」「搜索Python核心特性」「搜索对比数据」3个 rag_search 即可。\n\
                 后续子任务基于这些检索结果执行。{}\n\
                 将任务拆解为3-6个子任务并分配工具。\n\
                 {}\
                 要求：\n\
                 1. 把大任务拆成小步骤，每个子任务职责单一\n\
                 2. 可以用 depends_on 表示任务之间的依赖关系\n\
                 3. depends_on 为空的子任务可以并行执行\n\
                 可用工具：[{}]\n\
                 返回JSON：[{{ \"name\": \"子任务名（中文）\", \"description\": \"做什么\", \"tool\": \"工具名\", \"task_type\": \"normal\", \"depends_on\": [\"前置\"], \"input_template\": \"需要什么\" }}]\n\
                 任务：{}\n\
                 只返回JSON。",
                rag_context_block, routing_section, tj, task
            )
        } else {
            format!(
                "将任务拆解为3-8个子任务并分配工具。\n\
                 {}\
                 要求：\n\
                 1. 把大任务拆成小步骤，每个子任务职责单一\n\
                 2. 可以用 depends_on 表示任务之间的依赖关系\n\
                 3. depends_on 为空的子任务可以并行执行\n\
                 可用工具：[{}]\n\
                 返回JSON：[{{ \"name\": \"子任务名（中文）\", \"description\": \"做什么\", \"tool\": \"工具名\", \"task_type\": \"normal\", \"depends_on\": [\"前置\"], \"input_template\": \"需要什么\" }}]\n\
                 任务：{}\n\
                 只返回JSON。",
                routing_section, tj, task
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
        max_concurrency: usize,
        cancel: Option<Arc<AtomicBool>>,
        progress_tx: Option<broadcast::Sender<String>>,
        verify_hook: Option<Arc<CompositeVerifyHook>>,
    ) -> Result<CompiledGraph<AgentState>, GraphDemoError> {
        let mut graph = StateGraph::<AgentState>::new();
        let semaphore = Arc::new(Semaphore::new(max_concurrency));
        let cancel = cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

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
            let sem = semaphore.clone();

            let cancel = cancel.clone();
            let progress_tx = progress_tx.clone();
            let verify_hook = verify_hook.clone();
            graph.add_async_node(at.name.clone(), move |state: &AgentState| {
                let at = at.clone();
                let config = config.clone();
                let task = task.clone();
                let ctx = ctx.clone();
                let rag = rag.clone();
                let state = state.clone();
                let vector_store = vector_store.clone();
                let sem = sem.clone();
                let cancel = cancel.clone();
                let progress_tx = progress_tx.clone();
                let verify_hook = verify_hook.clone();

                async move {
                    // 检查是否已取消
                    if cancel.load(Ordering::Relaxed) {
                        tracing::info!(task_name = %at.name, "任务已取消，跳过执行");
                        return Ok(StateUpdate::full(state));
                    }

                    // 获取并发许可
                    let _permit = match sem.acquire().await {
                        Ok(p) => p,
                        Err(_) => {
                            tracing::warn!(task_name = %at.name, "并发许可获取失败");
                            return Ok(StateUpdate::full(state));
                        }
                    };

                    // 再次检查取消（等待许可期间可能被取消）
                    if cancel.load(Ordering::Relaxed) {
                        tracing::info!(task_name = %at.name, "任务在等待许可时被取消");
                        return Ok(StateUpdate::full(state));
                    }
                    let task_name = at.name.clone();
                    let tool_name = at.tool.clone();
                    let input_template = at.input_template.clone();
                    tracing::info!(task_name = %task_name, tool = %tool_name, "任务开始执行");

                    let task_start = Instant::now();

                    // 推送任务开始事件
                    if let Some(ref tx) = progress_tx {
                        let event = serde_json::json!({
                            "type": "task_start",
                            "task": at.name.clone(),
                            "tool": at.tool.clone(),
                        }).to_string();
                        let _ = tx.send(event);
                    }

                    let registry = Arc::new(ToolRegistry::default_registry());
                    let spawn_progress_tx = progress_tx.clone();
                    let spawn_verify = verify_hook.clone();

                    let handle = tokio::spawn(async move {
                        let (output, tokens, verify_retries) = match at.task_type.as_str() {
                            "human_review" => {
                                (format!("⏸️ 待人工审批：{}", at.description), 0, 0u32)
                            }
                            "decision" => {
                                let decision_llm = OpenAIChat::new(
                                    config.to_langchain_openai_config().with_max_tokens(1024)
                                );
                                let ctx_str = if ctx.is_empty() { "无前置结果".to_string() } else {
                                    format!("前置完成的任务结果：\n{}", ctx)
                                };
                                let route_options: Vec<String> = at.routes.keys().map(|k| format!("「{}」", k)).collect();
                                let p = format!(
                                    "基于以下信息做出判断，只返回决策结果（{}），不要多余内容。\n\n{}\n\n当前决策：{}",
                                    route_options.join(" 或 "), ctx_str, at.description
                                );
                                match decision_llm.invoke(vec![Message::human(&p)], None).await {
                                    Ok(r) => {
                                        let t = r.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);
                                        (r.content.clone(), t, 0u32)
                                    }
                                    Err(_e) => ("判断失败".to_string(), 0, 0u32),
                                }
                            }
                            _ => {
                                let mut verify_retries: u32 = 0;
                                let tool_ctx = ToolContext {
                                    config: config.clone(),
                                    task: task.clone(),
                                    description: at.description.clone(),
                                    ctx: ctx.clone(),
                                    rag: rag.clone(),
                                    input_template: at.input_template.clone(),
                                    vector_store: vector_store.clone(),
                                    cancel: cancel.clone(),
                                    progress: spawn_progress_tx.clone(),
                                };
                                let tool_name = if at.tool.is_empty() { "llm_query" } else { &at.tool };
                                match registry.get(tool_name) {
                                    Some(tool) => {
                                        use crate::services::verify::VerifyResult;
                                        let mut last_result = tool.execute(&tool_ctx).await;

                                        if let Some(ref hook) = spawn_verify {
                                            for attempt in 1..=3 {
                                                match &last_result {
                                                    Ok((ref out, _)) => {
                                                        match hook.verify(out, &at).await {
                                                            VerifyResult::Pass => break,
                                                            VerifyResult::Fail(reason) => {
                                                                verify_retries = attempt as u32;
                                                                if attempt < 3 {
                                                                    tracing::info!(task_name = %at.name, attempt = attempt, reason = %reason, "验证失败，带反馈重试");
                                                                    let retry_ctx = ToolContext {
                                                                        description: format!("{}\n\n---\n【上次验证失败】\n失败原因: {}\n请修正后重新输出。",
                                                                            at.description, reason),
                                                                        ..tool_ctx.clone()
                                                                    };
                                                                    last_result = tool.execute(&retry_ctx).await;
                                                                } else {
                                                                    if let Ok((ref mut out, _)) = last_result {
                                                                        out.push_str(&format!("\n\n⚠️ 验证不通过(已重试3次): {}", reason));
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Err(_) => break,
                                                }
                                            }
                                        }

                                        let (output, tokens) = match last_result {
                                            Ok(r) => r,
                                            Err(e) => (e, 0),
                                        };
                                        (output, tokens, verify_retries)
                                    }
                                    None => {
                                        tracing::warn!("工具 {} 不存在，降级为 llm_query", at.tool);
                                        match registry.get("llm_query") {
                                            Some(fallback) => {
                                                let fallback_ctx = ToolContext {
                                                    description: format!("(工具:{} 不可用，请直接用 LLM 执行)\n{}", at.tool, at.description),
                                                    ..tool_ctx
                                                };
                                                let r = fallback.execute(&fallback_ctx).await.unwrap_or_else(|e| (e, 0));
                                                (r.0, r.1, verify_retries)
                                            }
                                            None => (format!("工具 {} 不可用且无降级方案", at.tool), 0, 0u32),
                                        }
                                    }
                                }
                            }
                        };

                        (output, tokens, verify_retries)
                    });

                    match handle.await {
                        Ok((output, tokens, verify_retries)) => {
                            let elapsed = task_start.elapsed().as_millis() as u64;
                            tracing::info!(task_name = %task_name, duration_ms = elapsed, tokens = tokens, "任务完成");
                            // 推送进度事件
                            if let Some(ref tx) = progress_tx {
                                let event = serde_json::json!({
                                    "type": "task_complete",
                                    "task": task_name,
                                    "tool": tool_name,
                                    "duration_ms": elapsed,
                                    "tokens": tokens,
                                }).to_string();
                                let _ = tx.send(event);
                            }
                            let mut new_state = state;
                            new_state.add_message(MessageEntry::ai(
                                serde_json::json!({
                                    "task": task_name,
                                    "output": output,
                                    "tokens": tokens,
                                    "duration_ms": elapsed,
                                    "tool": tool_name,
                                    "input_summary": input_template,
                                    "verify_retries": verify_retries,
                                }).to_string()
                            ));
                            Ok(StateUpdate::full(new_state))
                        }
                        Err(e) => {
                            let elapsed = task_start.elapsed().as_millis() as u64;
                            tracing::warn!(task_name = %task_name, error = %e, "任务执行失败");
                            // 推送失败事件
                            if let Some(ref tx) = progress_tx {
                                let event = serde_json::json!({
                                    "type": "task_error",
                                    "task": task_name,
                                    "error": e.to_string(),
                                }).to_string();
                                let _ = tx.send(event);
                            }
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
                            verify_retries: parsed["verify_retries"].as_u64().unwrap_or(0) as u32,
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
        cancel: Option<Arc<AtomicBool>>,
        progress_tx: Option<broadcast::Sender<String>>,
        use_verify: bool,
    ) -> Result<Vec<AgentExecResult>, GraphDemoError> {
        let ctx: String = context.iter()
            .map(|r| format!("【{}】\n{}", r.task_name, r.output))
            .collect::<Vec<_>>().join("\n\n");
        let rag = if rag_context.is_empty() {
            String::new()
        } else {
            format!("\n\n知识库检索结果：\n{}", rag_context)
        };
        let max_concurrency = config.agent.max_concurrency;
        let verify_hook = if use_verify {
            Some(Arc::new(crate::services::verify::create_verify_hook(config)))
        } else {
            None
        };

        let compiled = Self::build_batch_graph(
            config.clone(),
            task.to_string(),
            batch.to_vec(),
            ctx,
            rag,
            vector_store,
            max_concurrency,
            cancel,
            progress_tx,
            verify_hook,
        )?;

        let initial = AgentState::new(task.to_string());
        let invocation = compiled.invoke_parallel(initial).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;

        Ok(Self::extract_results(&invocation))
    }

    /// ── 开始执行（返回第一批结果 + session_id） ──
    /// 使用框架 invoke_parallel 并行执行第一批就绪任务
    pub async fn execute_batch_start(config: &Config, task: String, agent_tasks: Vec<AgentTask>, rag_context: String, vector_store: Option<Arc<QdrantStore>>, pool: Option<SqlitePool>, use_verify: bool) -> Result<(String, Vec<AgentExecResult>, bool), GraphDemoError> {
        let sid = Uuid::new_v4().to_string();
        let done: HashSet<String> = HashSet::new();
        let batch = Self::ready_batch(&agent_tasks, &done);
        if batch.is_empty() { return Err(GraphDemoError::BuildError("没有可执行的任务".into())); }

        let cancel_flag = get_cancel_flag(&sid);
        let (progress_tx, _) = broadcast::channel::<String>(32);
        let progress_tx_clone = progress_tx.clone();
        let results = Self::run_batch_with_framework(config, &task, &batch, &[], &rag_context, vector_store, Some(cancel_flag.clone()), Some(progress_tx), use_verify).await?;

        // 路由逻辑：如果本批有决策节点，跳过未选中的分支
        let mut skipped_names: HashSet<String> = HashSet::new();
        let mut skipped_results: Vec<AgentExecResult> = Vec::new();
        for r in &results {
            if let Some(decided) = Self::parse_decision(&r.output) {
                if let Some(task_def) = agent_tasks.iter().find(|t| t.name == r.task_name) {
                    let mut matched = false;
                    for (route_key, next_tasks) in &task_def.routes {
                        if route_key == &decided {
                            matched = true;
                        } else {
                            for task_name in next_tasks {
                                Self::skip_downstream(&agent_tasks, task_name, &mut skipped_names, &mut skipped_results);
                            }
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
            let plan_json = serde_json::to_string(&agent_tasks).unwrap_or_else(|_| "[]".to_string());
            save_agent_session(p, &sid, &task, &plan_json).await;
            for r in &done_results {
                save_agent_result(p, &sid, r).await;
            }
        }

        let has_more = review_pending.is_empty() && {
            let remaining = Self::ready_batch(&agent_tasks, &completed_names);
            remaining.iter().any(|t| !completed_names.contains(&t.name))
        };

        let all = agent_tasks.clone();
        let batch_state = BatchState {
            task, all, done: done_results.clone(), completed_names, start: Instant::now(), rag_context, review_pending,
            cancel: cancel_flag.clone(),
            progress_tx: Some(progress_tx_clone),
            use_verify,
        };
        store().lock().unwrap().get_or_insert_with(HashMap::new).insert(sid.clone(), batch_state);

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

        let (context, progress_tx, use_verify) = {
            let g = store().lock().unwrap();
            let s = g.as_ref().unwrap().get(sid);
            let ctx = s.map(|s| s.done.clone()).unwrap_or_default();
            let tx = s.and_then(|s| s.progress_tx.clone());
            let verify = s.map(|s| s.use_verify).unwrap_or(false);
            (ctx, tx, verify)
        };
        let results = Self::run_batch_with_framework(config, &task, &batch, &context, &rag_context, vector_store, Some(get_cancel_flag(sid)), progress_tx, use_verify).await?;

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
                            for (route_key, next_tasks) in &task_def.routes {
                                if route_key == &decided {
                                    matched = true;
                                } else {
                                    for task_name in next_tasks {
                                        Self::skip_downstream(&all, task_name, &mut s.completed_names, &mut s.done);
                                    }
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
        let keywords = ["不充分", "不通过", "not enough", "enough", "充分", "通过", "yes", "no", "tech", "general", "other"];
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
            verify_retries: 0,
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
    pub async fn execute_all_batches(config: &Config, task: String, agent_tasks: Vec<AgentTask>, rag_context: String, vector_store: Option<Arc<QdrantStore>>, use_verify: bool) -> Result<AgentExecResponse, GraphDemoError> {
        let total_start = Instant::now();

        let mut done: HashSet<String> = HashSet::new();
        let mut all_results: Vec<AgentExecResult> = Vec::new();

        while done.len() < agent_tasks.len() {
            let batch = Self::ready_batch(&agent_tasks, &done);
            if batch.is_empty() {
                break;
            }

            let results = Self::run_batch_with_framework(config, &task, &batch, &all_results, &rag_context, vector_store.clone(), None, None, use_verify).await?;

            for r in &results {
                done.insert(r.task_name.clone());
                all_results.push(r.clone());

                // 决策节点：根据 LLM 的决策结果跳过未选中的后续任务
                if let Some(decided) = Self::parse_decision(&r.output) {
                    // 找到这个决策任务对应的 AgentTask
                    if let Some(task_def) = agent_tasks.iter().find(|t| t.name == r.task_name) {
                        let mut matched = false;
                        // 遍历 routes，把非选中的分支对应的后续任务标记为已完成（跳过）
                        for (route_key, next_tasks) in &task_def.routes {
                            if route_key == &decided {
                                matched = true;
                            } else {
                                for task_name in next_tasks {
                                    Self::skip_downstream(&agent_tasks, task_name, &mut done, &mut all_results);
                                }
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
        remove_cancel_flag(sid);

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
                verify_retries: 0,
            });
        } else {
            s.done.push(AgentExecResult {
                task_name: task_name.to_string(),
                tool: "human_review".to_string(),
                input_summary: task.description.clone(),
                output: format!("❌ 人工审批拒绝。反馈：{}", feedback),
                duration_ms: 0,
                tokens: 0,
                verify_retries: 0,
            });
            s.completed_names.insert(task_name.to_string());
        }

        Ok(())
    }

    /// ── 查询执行日志 ──
    pub async fn get_session_logs(pool: &SqlitePool, sid: &str) -> Result<serde_json::Value, GraphDemoError> {
        let session = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT session_id, task, created_at, status FROM agent_sessions WHERE session_id = ?"
        )
        .bind(sid)
        .fetch_optional(pool)
        .await
        .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;

        let results = sqlx::query_as::<_, (String, String, String, i64, i64)>(
            "SELECT task_name, tool, output, duration_ms, tokens FROM agent_results WHERE session_id = ? ORDER BY id ASC"
        )
        .bind(sid)
        .fetch_all(pool)
        .await
        .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;

        match session {
            Some((session_id, task, created_at, status)) => {
                let items: Vec<serde_json::Value> = results.into_iter().map(|(name, tool, output, dur, tok)| {
                    serde_json::json!({
                        "task_name": name,
                        "tool": tool,
                        "output_preview": output.chars().take(200).collect::<String>(),
                        "duration_ms": dur,
                        "tokens": tok,
                    })
                }).collect();
                Ok(serde_json::json!({
                    "session_id": session_id,
                    "task": task,
                    "created_at": created_at,
                    "status": status,
                    "results": items,
                    "total": items.len(),
                }))
            }
            None => Err(GraphDemoError::BuildError("session不存在".into())),
        }
    }

    /// ── Token 用量统计 ──
    pub async fn get_token_stats(pool: &SqlitePool) -> Result<serde_json::Value, GraphDemoError> {
        let total_sessions = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_sessions")
            .fetch_one(pool).await.unwrap_or(0);
        let total_tokens = sqlx::query_scalar::<_, Option<i64>>("SELECT SUM(tokens) FROM agent_results")
            .fetch_one(pool).await.unwrap_or(None).unwrap_or(0);
        let total_duration = sqlx::query_scalar::<_, Option<i64>>("SELECT SUM(duration_ms) FROM agent_results")
            .fetch_one(pool).await.unwrap_or(None).unwrap_or(0);
        let avg_tokens = if total_sessions > 0 { total_tokens / total_sessions } else { 0 };

        Ok(serde_json::json!({
            "total_sessions": total_sessions,
            "total_tokens": total_tokens,
            "total_duration_ms": total_duration,
            "avg_tokens_per_session": avg_tokens,
        }))
    }

    /// ── 获取历史执行列表 ──
    pub async fn list_sessions(pool: &SqlitePool) -> Result<Vec<serde_json::Value>, GraphDemoError> {
        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT session_id, task, created_at, status FROM agent_sessions ORDER BY created_at DESC LIMIT 50"
        )
        .fetch_all(pool)
        .await
        .map_err(|e| GraphDemoError::BuildError(e.to_string()))?;

        Ok(rows.into_iter().map(|(sid, task, created_at, status)| {
            serde_json::json!({
                "session_id": sid,
                "task": task,
                "created_at": created_at,
                "status": status,
            })
        }).collect())
    }

    /// ── 获取进度推送 receiver（SSE用） ──
    pub fn get_progress_receiver(sid: &str) -> Result<broadcast::Receiver<String>, GraphDemoError> {
        let g = store().lock().unwrap();
        let m = g.as_ref().ok_or_else(|| GraphDemoError::BuildError("存储未初始化".into()))?;
        let s = m.get(sid).ok_or_else(|| GraphDemoError::BuildError("session不存在".into()))?;
        s.progress_tx.as_ref()
            .map(|tx| tx.subscribe())
            .ok_or_else(|| GraphDemoError::BuildError("该 session 不支持进度推送".into()))
    }

    /// ── 取消执行 ──
    pub fn cancel_session(sid: &str) -> Result<(), GraphDemoError> {
        set_cancelled(sid);
        Ok(())
    }

    /// ── 获取待审核任务列表 ──
    pub fn get_pending_reviews(sid: &str) -> Result<Vec<AgentTask>, GraphDemoError> {
        let g = store().lock().unwrap();
        let m = g.as_ref().ok_or_else(|| GraphDemoError::BuildError("存储未初始化".into()))?;
        let s = m.get(sid).ok_or_else(|| GraphDemoError::BuildError("session不存在".into()))?;
        Ok(s.review_pending.clone())
    }

    /// ── 兼容旧接口（一次性跑完所有批次） ──
    pub async fn plan_and_execute(config: &Config, task: String) -> Result<AgentExecResponse, GraphDemoError> {
        let plan = Self::plan(config, task.clone(), String::new(), false, false).await?;
        let (sid, mut all, _) = Self::execute_batch_start(config, task, plan.tasks, String::new(), None, None, false).await?;
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
            output: "ok".into(), duration_ms: 0, tokens: 0, verify_retries: 0,
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
        let graph1 = AgentEngine::build_batch_graph(config.clone(), "test".into(), batch1, String::new(), String::new(), None, 3, None);
        assert!(graph1.is_ok(), "单任务批次应该编译成功");
        let compiled = graph1.unwrap();
        let nodes = compiled.node_names();
        assert!(nodes.contains(&"A".to_string()));
        assert!(nodes.contains(&"__dispatch__".to_string()));

        // 多任务
        let batch2 = vec![make_task("A", vec![]), make_task("B", vec![]), make_task("C", vec![])];
        let graph2 = AgentEngine::build_batch_graph(config, "test".into(), batch2, String::new(), String::new(), None, 3, None);
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
            3,
            None,
        );
        assert!(graph.is_ok(), "批次图应该编译成功");
    }
}
