use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use crate::services::mcp::mcp_bridge::McpBridge;
use crate::services::tools::{ToolRegistry, ToolContext, ToolIndex};
use crate::services::verify::{CompositeVerifyHook, VerifyHook};
use crate::stores::QdrantStore;
use langchainrust::langgraph::{
    AgentState, CompiledGraph, MessageEntry, ParallelInvocation,
    StateGraph, StateSchema, StateUpdate, START, END,
};
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable, core::tools::ToolDefinition};
use sqlx::SqlitePool;
use std::collections::{HashSet, HashMap};
use std::time::Duration;
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
                llm_prompt: String::new(),
                api_request: String::new(),
                llm_raw: String::new(),
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
            use_subgraph: false,
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
    use_subgraph: bool,
}
static STORE: Mutex<Option<HashMap<String, BatchState>>> = Mutex::new(None);
fn store() -> &'static Mutex<Option<HashMap<String, BatchState>>> { &STORE }

/// 尝试修复常见 JSON 格式错误
// ── 子图状态类型（独立于父图 AgentState） ──
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchExtractState {
    pub query: String,
    pub search_raw: String,
    pub extracted: String,
}
impl StateSchema for SearchExtractState {}

pub struct AgentEngine;

impl AgentEngine {
    /// ── 规划 ──
    pub async fn plan(config: &Config, task: String, rag_context: String, use_rag: bool, use_routing: bool, use_subgraph: bool, mcp_bridge: Option<&McpBridge>, tool_index: Option<&ToolIndex>) -> Result<AgentPlan, GraphDemoError> {
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));
        // 用工具检索索引找到 Top-20 相关工具（大量工具时避免 prompt 爆炸）
        let tool_entries: Vec<String> = if let Some(index) = tool_index {
            index.search(&task, 20).await.iter()
                .map(|(n, d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}", n, d))
                .collect()
        } else if let Some(bridge) = mcp_bridge {
            bridge.get_adapter_boxes().iter().map(|a|
                format!("{{\"name\":\"{}\",\"description\":\"{}\"}}", a.name(), a.description())
            ).collect()
        } else {
            ToolRegistry::default_registry().list_descriptions().iter()
                .map(|(n,d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}", n, d))
                .collect()
        };
        let tj = tool_entries.join(",");
        let routing_section = if use_routing {
            "【强制要求】你**必须**在规划中包含一个 type=decision 的「信息是否充分」判断节点。\n\
             决策节点不需要 tool，通过 routes 定义走向。\n\
             「充分」时直接去汇总，「不充分」时补充搜索再去汇总。\n\
             汇总任务（llm_query）的 depends_on 只应依赖决策节点，不要依赖条件分支里的任务。\n\
             强制结构如下：\n\
             1. 先做 2-3 个最初搜索（rag_search）\n\
             2. 然后「判断信息是否充分」（type=decision, depends_on=所有最初搜索）\n\
             3. 「充分」分支：routes 中对应空数组（不需要额外任务）\n\
             4. 「不充分」分支：routes 中对应补充搜索任务（web_search）\n\
             5. 最后「回答用户问题」（llm_query, depends_on=[\"判断信息是否充分\"]）\n\
             routes 格式：\"充分\":[], \"不充分\":[\"补充搜索A\",\"补充搜索B\"]\n\
             正确的完整示例：\n\
                 [\n\
                   {\"name\":\"调研Go\",\"tool\":\"rag_search\",\"depends_on\":[],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                   {\"name\":\"调研Python\",\"tool\":\"rag_search\",\"depends_on\":[],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                   {\"name\":\"判断信息是否充分\",\"task_type\":\"decision\",\"depends_on\":[\"调研Go\",\"调研Python\"],\"routes\":{\"充分\":[],\"不充分\":[\"补充搜索Go\",\"补充搜索Python\"]},\"input_template\":\"\"},\n\
                   {\"name\":\"补充搜索Go\",\"tool\":\"web_search\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                   {\"name\":\"补充搜索Python\",\"tool\":\"web_search\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\",\"input_template\":\"\"},\n\
                    {\"name\":\"回答用户问题\",\"tool\":\"llm_query\",\"depends_on\":[\"判断信息是否充分\"],\"task_type\":\"normal\",\"input_template\":\"\"}\n\
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
                  任务：{}\n\
                  通过 create_task_plan 函数输出规划结果。",
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
                 任务：{}\n\
                 通过 create_task_plan 函数输出规划结果。",
                routing_section, tj, task
            )
        };
        // ═══════════════════════════════════════════════════════════════
        //  Function Calling — 规划阶段
        // ═══════════════════════════════════════════════════════════════
        //
        //  作用：让 LLM 输出结构化的子任务列表，而不是自由文本
        //
        //  请求体结构（实际发给 API）：
        //  {
        //    "model": "qwen-turbo",
        //    "messages": [{"role": "user", "content": "..."}],
        //    "tools": [{                                ← 定义 LLM 可用的"函数"
        //      "type": "function",
        //      "function": {
        //        "name": "create_task_plan",            ← 函数名
        //        "description": "将用户任务拆解为子任务列表",
        //        "parameters": {                        ← JSON Schema 定义参数格式
        //          "type": "object",
        //          "properties": {
        //            "tasks": {                         ← 数组，每个元素是一个子任务
        //              "type": "array",
        //              "items": {
        //                "properties": {
        //                  "name":  {"type": "string"},
        //                  "tool":  {"type": "string"},
        //                  "task_type": {"enum": ["normal","decision","human_review"]},
        //                  "depends_on": {"type": "array", "items": {"type": "string"}}
        //                },
        //                "required": ["name", "tool", "depends_on"]
        //              }
        //            }
        //          },
        //          "required": ["tasks"]
        //        }
        //      }
        //    }],
        //    "tool_choice": "required"                   ← 强制 LLM 必须调函数，不能返回文本
        //  }
        //
        //  LLM 返回（content 为 null，数据在 tool_calls 里）：
        //  {
        //    "content": null,
        //    "tool_calls": [{
        //      "function": {
        //        "name": "create_task_plan",
        //        "arguments": "{\"tasks\":[{...},{...}]}"  ← arguments 是 JSON 字符串
        //      }
        //    }]
        //  }
        //
        //  解析流程：
        //    arguments (字符串) → serde_json::from_str → Value
        //    → value["tasks"] → serde_json::from_value → Vec<AgentTask>
        //
        // ───────────────────────────────────────────────────────────
        //  定义 tool（答题卡）
        // ───────────────────────────────────────────────────────────
        // 定义 tool：create_task_plan ──────────────────────────────
        // 这个 tool 告诉 LLM "你必须输出 tasks 数组，里面放拆解好的子任务"
        // LLM 收到后不会返回文本，而是在 tool_calls 里返回 arguments
        // ──────────────────────────────────────────────────────────
        let plan_tool = ToolDefinition::new("create_task_plan", "将用户任务拆解为子任务列表")
            .with_parameters(serde_json::json!({
                "type": "object",                          // 参数类型固定为 object
                "properties": {
                    "tasks": {                              // 唯一的顶级字段：tasks 数组
                        "type": "array",                    // tasks 是个数组
                        "items": {                          // 数组里每个元素的结构
                            "type": "object",
                            "properties": {
                                "name": {"type": "string", "description": "子任务名（中文）"},
                                "description": {"type": "string", "description": "做什么"},
                                "tool": {"type": "string", "description": "工具名（rag_search/web_search/llm_query 等）"},
                                "task_type": {               // 任务类型
                                    "type": "string",
                                    "enum": ["normal", "decision", "human_review"]  // 只能选这三个
                                },
                                "depends_on": {               // 依赖的前置任务名列表
                                    "type": "array",
                                    "items": {"type": "string"}
                                },
                                "input_template": {"type": "string"},  // 输入模板
                                "routes": {                              // 仅 decision 类型需要
                                    "type": "object",
                                    "description": "决策节点的路由表",
                                    "additionalProperties": {
                                        "type": "array",
                                        "items": {"type": "string"}
                                    }
                                }
                            },
                            "required": ["name", "tool", "depends_on"]  // 这三个字段必填
                        }
                    }
                },
                "required": ["tasks"]                      // 顶级必须包含 tasks 字段
            }));
        // ───────────────────────────────────────────────────────────
        //  绑定 tool + 强制调函数 → 调用 LLM
        // ───────────────────────────────────────────────────────────
        //  bind_tools() → 把 tool 定义注入 LLM 的 config
        //  with_tool_choice("required") → LLM 必须用 tool_calls 返回，不能返回文本 content
        //  invoke() → 发 HTTP 请求给 API
        // ───────────────────────────────────────────────────────────
        let fc_llm = llm.bind_tools(vec![plan_tool]).with_tool_choice("required");
        let r = fc_llm.invoke(vec![Message::human(&p)], None).await.map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        // ───────────────────────────────────────────────────────────
        //  解析 tool_calls
        // ───────────────────────────────────────────────────────────
        //  r.tool_calls       → Option<Vec<ToolCall>>
        //  calls.first()      → Option<&ToolCall>（取第一个）
        //  call.arguments()   → &str（JSON 字符串）
        //  from_str → Value   → 解析 JSON 字符串
        //  value["tasks"]     → 取 tasks 字段
        //  from_value → Vec<AgentTask> → 反序列化为 Rust 结构体
        // ───────────────────────────────────────────────────────────
        let mut tasks: Vec<AgentTask> = r.tool_calls.as_ref()
            .and_then(|calls| calls.first())
            .and_then(|call| {
                let args: serde_json::Value = serde_json::from_str(call.arguments()).ok()?;
                serde_json::from_value(args["tasks"].clone()).ok()
            })
            .unwrap_or_default();

        if tasks.is_empty() { return Err(GraphDemoError::BuildError("规划为空".into())); }
        // 自动填充决策节点的 routes（LLM 有时会忘记填）
        for i in 0..tasks.len() {
            if tasks[i].task_type == "decision" && tasks[i].routes.is_empty() {
                let task_name = tasks[i].name.clone();
                let mut web_searches: Vec<String> = Vec::new();
                for t in &tasks {
                    if t.depends_on.contains(&task_name) && t.tool == "web_search" {
                        web_searches.push(t.name.clone());
                    }
                }
                let mut routes = std::collections::HashMap::new();
                routes.insert("充分".to_string(), Vec::new());
                routes.insert("不充分".to_string(), web_searches);
                tasks[i].routes = routes;
                tracing::info!("自动填充路由：{} 充分=[], 不充分={:?}", tasks[i].name, tasks[i].routes.get("不充分"));
            }
        }
        let gs = Self::build_graph_with_subgraph(&tasks, use_subgraph);
        Ok(AgentPlan{original_task:task, tasks, graph_structure:gs})
    }

    fn build_graph(tasks: &[AgentTask]) -> serde_json::Value {
        Self::build_graph_with_subgraph(tasks, false)
    }

    fn build_graph_with_subgraph(tasks: &[AgentTask], use_subgraph: bool) -> serde_json::Value {
        let names: HashSet<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
        let nodes: Vec<String> = tasks.iter().map(|t| t.name.clone()).collect();
        let mut edges = vec![];
        let mut routers = vec![];

        // 收集决策节点（即使 routes 为空也加入，前端需要它来显示橙色路由器节点）
        for task in tasks {
            if task.task_type == "decision" {
                routers.push(serde_json::json!({
                    "name": task.name,
                    "routes": task.routes,
                }));
            }
        }

        // START → 没有依赖的任务（可以直接开始）
        for task in tasks {
            if task.depends_on.iter().all(|d| !names.contains(d.as_str())) {
                edges.push(serde_json::json!({"type":"fixed","source":"__start__","target":task.name}));
            }
        }

        // depends_on 边（决策节点的路由分支标 type=route）
        for task in tasks {
            for d in &task.depends_on {
                if names.contains(d.as_str()) {
                    if let Some(dec_task) = tasks.iter().find(|t| t.name == *d && t.task_type == "decision") {
                        let mut route_label = "";
                        for (key, next_tasks) in &dec_task.routes {
                            if next_tasks.contains(&task.name) {
                                route_label = key;
                                break;
                            }
                        }
                        if !route_label.is_empty() {
                            edges.push(serde_json::json!({"type":"route","source":d,"target":task.name,"label":route_label}));
                        } else {
                            edges.push(serde_json::json!({"type":"fixed","source":d,"target":task.name}));
                        }
                    } else {
                        edges.push(serde_json::json!({"type":"fixed","source":d,"target":task.name}));
                    }
                }
            }
        }

        // 非路由分支任务到 END
        for task in tasks {
            if task.task_type != "decision" && !routers.iter().any(|r| {
                r["routes"].as_object().map_or(false, |routes| {
                    routes.values().any(|v| v.as_array().map_or(false, |arr| arr.contains(&serde_json::Value::String(task.name.clone()))))
                })
            }) {
                edges.push(serde_json::json!({"type":"fixed","source":task.name,"target":"__end__"}));
            }
        }
        // 决策节点到 END
        for task in tasks {
            if task.task_type == "decision" {
                edges.push(serde_json::json!({"type":"fixed","source":task.name,"target":"__end__"}));
            }
        }

        if use_subgraph {
            // 标记哪些节点是子图（rag_search 任务 → 搜索+提取子图）
            let mut subgraph_nodes = serde_json::Map::new();
            for t in tasks {
                if t.tool == "rag_search" {
                    subgraph_nodes.insert(t.name.clone(), serde_json::json!([
                        {"name":"知识库检索","tool":"rag_search"},
                        {"name":"提取关键信息","tool":"llm_query"}
                    ]));
                }
            }
            serde_json::json!({
                "entry_point": tasks[0].name,
                "nodes": nodes,
                "edges": edges,
                "routers": routers,
                "subgraph": true,
                "subgraph_nodes": subgraph_nodes,
            })
        } else {
            serde_json::json!({"entry_point": tasks[0].name, "nodes": nodes, "edges": edges, "routers": routers})
        }
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

    // ──────────────────────── 子图系统 ────────────────────────
    //
    // 搜索→提取子图：独立状态类型，可复用，可独立测试
    // 定义一次，通过 add_subgraph + mapper 嵌入任意父图

    /// 构建搜索→提取子图（定义一次，可独立测试）
    fn build_search_extract_subgraph_template(
        config: &Config,
        vector_store: Option<Arc<QdrantStore>>,
    ) -> Result<CompiledGraph<SearchExtractState>, GraphDemoError> {
        let config = config.clone();
        let config2 = config.clone();
        let vector_store = vector_store.clone();
        let mut sub = StateGraph::<SearchExtractState>::new();

        sub.add_async_node("检索", move |state: &SearchExtractState| {
            let config = config.clone();
            let vector_store = vector_store.clone();
            let state = state.clone();
            async move {
                let query = &state.query;
                let search_result = match &vector_store {
                    Some(store) => match tokio::time::timeout(Duration::from_secs(30), store.search_rag(query, 3)).await {
                        Ok(Ok(results)) => {
                            let filtered: Vec<_> = results.iter().filter(|r| r.score >= 0.3).collect();
                            if filtered.is_empty() { format!("知识库中未找到相关文档（搜索词：{}）", query) }
                            else {
                                let content: Vec<String> = filtered.iter().map(|r| format!("[相关性 {:.1}%]\n{}", r.score * 100.0, r.document.content)).collect();
                                format!("检索结果（{}）：\n\n{}", query, content.join("\n\n---\n\n"))
                            }
                        }
                        _ => format!("搜索失败（{}）", query),
                    },
                    None => "知识库未配置".to_string(),
                };
                let mut ns = state;
                ns.search_raw = search_result;
                Ok(StateUpdate::full(ns))
            }
        });
        sub.add_async_node("提取", move |state: &SearchExtractState| {
            let config = config2.clone();
            let state = state.clone();
            async move {
                let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(2048));
                let prompt = format!("从以下检索结果中提取关键信息并总结。\n\n检索结果：\n{}\n\n请提取关键信息：", state.search_raw);
                let (output, _tokens) = match llm.invoke(vec![Message::human(&prompt)], None).await {
                    Ok(r) => (r.content.clone(), r.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0)),
                    Err(e) => (format!("提取失败: {}", e), 0),
                };
                let mut ns = state;
                ns.extracted = output;
                Ok(StateUpdate::full(ns))
            }
        });
        sub.add_edge(START, "检索");
        sub.add_edge("检索", "提取");
        sub.add_edge("提取", END);
        sub.compile().map_err(|e| GraphDemoError::BuildError(e.to_string()))
    }

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
        use_subgraph: bool,
        mcp_bridge: Option<Arc<McpBridge>>,
    ) -> Result<CompiledGraph<AgentState>, GraphDemoError> {
        let mut graph = StateGraph::<AgentState>::new();
        let semaphore = Arc::new(Semaphore::new(max_concurrency));
        let cancel = cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        // 空节点，作为 FanOut 的入口
        graph.add_node_fn("__dispatch__", |state| {
            Ok(StateUpdate::full(state.clone()))
        });

        // 每个任务一个节点（rag_search 且子图模式 → 搜索+提取子图）
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
            let mcp_bridge = mcp_bridge.clone();

            // 子图模式：每个 rag_search 任务使用搜索→提取子图（独立状态类型）
            if use_subgraph && at.tool == "rag_search" {
                let task_name = at.name.clone();
                let query = if at.input_template.is_empty() { at.name.clone() } else { at.input_template.clone() };
                let sub = Self::build_search_extract_subgraph_template(&config, vector_store.clone())?;
                graph.add_subgraph(
                    at.name.clone(), sub,
                    move |_: &AgentState| -> SearchExtractState {
                        SearchExtractState { query: query.clone(), search_raw: String::new(), extracted: String::new() }
                    },
                    move |sub_state: &SearchExtractState, parent: &mut AgentState| {
                        let msg = serde_json::json!({
                            "task": task_name, "output": sub_state.extracted,
                            "tokens": 0, "duration_ms": 0, "tool": "rag_search",
                            "input_summary": "", "verify_retries": 0,
                        }).to_string();
                        parent.add_message(MessageEntry::ai(msg));
                    },
                );
                continue;
            }

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
                let mcp_bridge = mcp_bridge.clone();

                async move {
                    if cancel.load(Ordering::Relaxed) {
                        tracing::info!(task_name = %at.name, "任务已取消，跳过执行");
                        return Ok(StateUpdate::full(state));
                    }
                    let _permit = match sem.acquire().await {
                        Ok(p) => p,
                        Err(_) => { tracing::warn!(task_name = %at.name, "并发许可获取失败"); return Ok(StateUpdate::full(state)); }
                    };
                    if cancel.load(Ordering::Relaxed) {
                        tracing::info!(task_name = %at.name, "任务在等待许可时被取消");
                        return Ok(StateUpdate::full(state));
                    }
                    let task_name = at.name.clone();
                    let tool_name = at.tool.clone();
                    let input_template = at.input_template.clone();
                    tracing::info!(task_name = %task_name, tool = %tool_name, "任务开始执行");
                    let task_start = Instant::now();
                    if let Some(ref tx) = progress_tx {
                        let _ = tx.send(serde_json::json!({"type":"task_start","task":at.name,"tool":at.tool}).to_string());
                    }
                    let mut reg = ToolRegistry::default_registry();
                    if let Some(ref bridge) = mcp_bridge {
                        for tool in bridge.get_adapter_boxes() {
                            reg.register(tool);
                        }
                    }
                    let registry = Arc::new(reg);
                    let spawn_progress_tx = progress_tx.clone();
                    let spawn_verify = verify_hook.clone();
                    let llm_prompt = if ctx.is_empty() {
                        format!("任务：{}\n当前子任务：{}\n\n{}", task, at.description, rag)
                    } else {
                        format!("任务：{}\n当前子任务：{}\n\n前置完成的任务结果：\n{}\n\n{}", task, at.description, ctx, rag)
                    };
                    let api_request = serde_json::to_string_pretty(&serde_json::json!({
                        "model": config.openai.chat_model,
                        "messages": [{"role": "user", "content": llm_prompt}],
                        "max_tokens": 2048,
                        "temperature": 0.7,
                    })).unwrap_or_default();
                    let handle = tokio::spawn(async move {
                        let (output, tokens, verify_retries) = match at.task_type.as_str() {
                            "human_review" => (format!("⏸️ 待人工审批：{}", at.description), 0, 0u32),
                            "decision" => {
                                let decision_llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));
                                let ctx_str = if ctx.is_empty() { "无前置结果".to_string() } else { format!("前置完成的任务结果：\n{}", ctx) };
                                let p = format!("基于以下信息判断是否足够回答用户问题。\n\n{}\n\n当前决策：{}", ctx_str, at.description);
                                // ═══════════════════════════════════════════════════
                                //  Function Calling — 决策节点
                                // ═══════════════════════════════════════════════════
                                //  作用：约束 LLM 只能输出 "充分" 或 "不充分"
                                //
                                //  发给 API 的请求体：
                                //  {
                                //    "messages": [{"role":"user","content":"..."}],
                                //    "tools": [{
                                //      "type":"function",
                                //      "function": {
                                //        "name": "make_decision",
                                //        "parameters": {
                                //          "properties": {
                                //            "route": {"enum": ["充分","不充分"]},  ← 锁死
                                //            "reason": {"type":"string"}
                                //          },
                                //          "required": ["route"]
                                //        }
                                //      }
                                //    }],
                                //    "tool_choice": "required"
                                //  }
                                //
                                //  LLM 返回：
                                //  {"tool_calls":[{"function":{"arguments":"{\"route\":\"充分\"}"}}]}
                                // ───────────────────────────────────────────────────
                                let route_keys: Vec<&str> = at.routes.keys().map(|k| k.as_str()).collect();
                                let first_key = route_keys.first().copied().unwrap_or("充分");
                                let second_key = route_keys.get(1).copied().unwrap_or("不充分");
                                // 定义 tool：make_decision ────────────────────
                                // 约束 LLM 只能输出 {"route":"充分"} 或 {"route":"不充分"}
                                // ─────────────────────────────────────────────
                                //  bind_tools → 把 tool 定义注入 LLM 的 config
                                //  with_tool_choice("required") → 强制走 tool_calls
                                //  invoke → 发 HTTP 请求给 API
                                // ─────────────────────────────────────────────
                                let fc_llm = decision_llm.bind_tools(vec![
                                    ToolDefinition::new("make_decision", "输出决策结果")
                                        .with_parameters(serde_json::json!({
                                            "type": "object",              // 参数固定为 object
                                            "properties": {
                                                "route": {                  // 路由结果
                                                    "type": "string",
                                                    "enum": [first_key, second_key]  // ← LLM 只能二选一
                                                },
                                                "reason": {                 // 决策理由（LLM 自由发挥）
                                                    "type": "string",
                                                    "description": "决策理由"
                                                }
                                            },
                                            "required": ["route", "reason"]  // 两个字段都要填
                                        }))
                                ]).with_tool_choice("required");
                                match fc_llm.invoke(vec![Message::human(&p)], None).await {
                                    Ok(r) => {
                                        let t = r.token_usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);
                                        // ──────────────────────────────────
                                        //  解析 tool_calls：
                                        //    r.tool_calls → Option<Vec<ToolCall>>
                                        //    calls.first() → 取第一个
                                        //    call.arguments() → JSON 字符串
                                        //    from_str → Value → 解析
                                        //    args["route"] → "充分"或"不充分"
                                        // ──────────────────────────────────
                                        let route = r.tool_calls.as_ref()
                                            .and_then(|calls| calls.first())
                                            .and_then(|call| serde_json::from_str::<serde_json::Value>(call.arguments()).ok())
                                            .and_then(|args| args["route"].as_str().map(|s| s.to_string()))
                                            .unwrap_or_else(|| "充分".to_string());
                                        (route, t, 0u32)
                                    }
                                    Err(_e) => ("判断失败".to_string(), 0, 0u32),
                                }
                            }
                            _ => {
                                let mut verify_retries: u32 = 0;
                                let tool_ctx = ToolContext {
                                    config: config.clone(), task: task.clone(), description: at.description.clone(),
                                    ctx: ctx.clone(), rag: rag.clone(), input_template: at.input_template.clone(),
                                    vector_store: vector_store.clone(), cancel: cancel.clone(), progress: spawn_progress_tx.clone(),
                                };
                                let t_name = if at.tool.is_empty() { "llm_query" } else { &at.tool };
                                match registry.get(t_name) {
                                    Some(tool) => {
                                        use crate::services::verify::VerifyResult;
                                        let mut last_result = tool.execute(&tool_ctx).await;
                                        if let Some(ref hook) = spawn_verify {
                                            for attempt in 1..=3 {
                                                match &last_result {
                                                    Ok((ref out, _)) => match hook.verify(out, &at).await {
                                                        VerifyResult::Pass => break,
                                                        VerifyResult::Fail(reason) => {
                                                            verify_retries = attempt as u32;
                                                            if attempt < 3 {
                                                                let retry_ctx = ToolContext { description: format!("{}\n\n---\n【上次验证失败】\n失败原因: {}\n请修正后重新输出。", at.description, reason), ..tool_ctx.clone() };
                                                                last_result = tool.execute(&retry_ctx).await;
                                                            } else { if let Ok((ref mut out, _)) = last_result { out.push_str(&format!("\n\n⚠️ 验证不通过(已重试3次): {}", reason)); } }
                                                        }
                                                    },
                                                    Err(_) => break,
                                                }
                                            }
                                        }
                                        let (out, tok) = match last_result { Ok(r) => r, Err(e) => (e, 0) };
                                        (out, tok, verify_retries)
                                    }
                                    None => {
                                        tracing::warn!("工具 {} 不存在，降级为 llm_query", at.tool);
                                        match registry.get("llm_query") {
                                            Some(fallback) => {
                                                let fb_ctx = ToolContext { description: format!("(工具:{} 不可用，请直接用 LLM 执行)\n{}", at.tool, at.description), ..tool_ctx };
                                                let r = fallback.execute(&fb_ctx).await.unwrap_or_else(|e| (e, 0));
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
                            if let Some(ref tx) = progress_tx {
                                let _ = tx.send(serde_json::json!({"type":"task_complete","task":task_name,"tool":tool_name,"duration_ms":elapsed,"tokens":tokens}).to_string());
                            }
                            let mut ns = state;
                            ns.add_message(MessageEntry::ai(serde_json::json!({"task":task_name,"output":output,"tokens":tokens,"duration_ms":elapsed,"tool":tool_name,"input_summary":input_template,"verify_retries":verify_retries,"llm_prompt":llm_prompt,"api_request":api_request,"llm_raw":output}).to_string()));
                            Ok(StateUpdate::full(ns))
                        }
                        Err(e) => {
                            let elapsed = task_start.elapsed().as_millis() as u64;
                            tracing::warn!(task_name = %task_name, error = %e, "任务执行失败");
                            if let Some(ref tx) = progress_tx {
                                let _ = tx.send(serde_json::json!({"type":"task_error","task":task_name,"error":e.to_string()}).to_string());
                            }
                            let mut ns = state;
                            ns.add_message(MessageEntry::ai(serde_json::json!({"task":task_name,"output":format!("任务 panic: {}",e),"tokens":0,"duration_ms":elapsed,"tool":tool_name,"input_summary":input_template,"llm_prompt":"","api_request":"","llm_raw":""}).to_string()));
                            Ok(StateUpdate::full(ns))
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
                            llm_prompt: parsed["llm_prompt"].as_str().unwrap_or("").to_string(),
                            api_request: parsed["api_request"].as_str().unwrap_or("").to_string(),
                            llm_raw: parsed["llm_raw"].as_str().unwrap_or("").to_string(),
                        });
                        break;
                    }
                }
            }
        }
        results
    }

    /// 计算任务的依赖层级
    fn calculate_levels(tasks: &[AgentTask]) -> Vec<Vec<AgentTask>> {
        let names: std::collections::HashSet<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
        let mut levels: Vec<Vec<AgentTask>> = Vec::new();
        let mut assigned: std::collections::HashSet<String> = std::collections::HashSet::new();

        loop {
            let ready: Vec<AgentTask> = tasks.iter()
                .filter(|t| !assigned.contains(&t.name))
                .filter(|t| t.depends_on.iter().filter(|d| names.contains(d.as_str())).all(|d| assigned.contains(d)))
                .cloned()
                .collect();
            if ready.is_empty() { break; }
            for t in &ready { assigned.insert(t.name.clone()); }
            levels.push(ready);
        }
        levels
    }

    /// 用框架执行一批任务（构建图 + invoke_parallel）
    /// 当 use_subgraph 时，构建层级子图（不是平铺 FanOut）
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
        use_subgraph: bool,
        mcp_bridge: Option<Arc<McpBridge>>,
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
            use_subgraph,
            mcp_bridge,
        )?;

        let initial = AgentState::new(task.to_string());
        let invocation = compiled.invoke_parallel(initial).await
            .map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;

        Ok(Self::extract_results(&invocation))
    }

    /// ── 开始执行（返回第一批结果 + session_id） ──
    /// 使用框架 invoke_parallel 并行执行第一批就绪任务
    pub async fn execute_batch_start(config: &Config, task: String, agent_tasks: Vec<AgentTask>, rag_context: String, vector_store: Option<Arc<QdrantStore>>, pool: Option<SqlitePool>, use_verify: bool, use_subgraph: bool, mcp_bridge: Option<Arc<McpBridge>>) -> Result<(String, Vec<AgentExecResult>, bool), GraphDemoError> {
        let sid = Uuid::new_v4().to_string();
        let done: HashSet<String> = HashSet::new();
        let batch = Self::ready_batch(&agent_tasks, &done);
        if batch.is_empty() { return Err(GraphDemoError::BuildError("没有可执行的任务".into())); }

        let cancel_flag = get_cancel_flag(&sid);
        let (progress_tx, _) = broadcast::channel::<String>(32);
        let progress_tx_clone = progress_tx.clone();
        let results = Self::run_batch_with_framework(config, &task, &batch, &[], &rag_context, vector_store, Some(cancel_flag.clone()), Some(progress_tx), use_verify, use_subgraph, mcp_bridge).await?;

        // 路由逻辑：跳过未选中的分支
        let mut skipped_names: HashSet<String> = HashSet::new();
        let mut skipped_results: Vec<AgentExecResult> = Vec::new();
        let route_members_set = Self::route_members(&agent_tasks);
        for r in &results {
            let task_def = match agent_tasks.iter().find(|t| t.name == r.task_name) {
                Some(t) if t.task_type == "decision" => t,
                _ => continue,
            };
            let decided = match Self::match_route_key(&r.output, &task_def.routes) {
                Some(d) => d,
                None => { tracing::info!("决策「{}」未匹配 route key", r.output); continue; }
            };
            for (route_key, next_tasks) in &task_def.routes {
                if route_key != &decided {
                    for task_name in next_tasks {
                        Self::skip_downstream(&agent_tasks, task_name, &route_members_set, &mut skipped_names, &mut skipped_results);
                    }
                }
            }
            // 自动跳过只依赖决策节点的 web_search 条件分支任务
            for task in &agent_tasks {
                if task.depends_on.contains(&r.task_name) && !route_members_set.contains(&task.name) && task.depends_on.len() == 1 && task.tool == "web_search" {
                    Self::skip_downstream(&agent_tasks, &task.name, &route_members_set, &mut skipped_names, &mut skipped_results);
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
            use_subgraph,
        };
        store().lock().unwrap().get_or_insert_with(HashMap::new).insert(sid.clone(), batch_state);

        Ok((sid, results, has_more))
    }

    /// ── 下一批 ──
    /// 使用框架 invoke_parallel 并行执行下一批就绪任务
    pub async fn execute_batch_next(config: &Config, sid: &str, vector_store: Option<Arc<QdrantStore>>, pool: Option<SqlitePool>, mcp_bridge: Option<Arc<McpBridge>>) -> Result<(Vec<AgentExecResult>, bool), GraphDemoError> {
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

        let (context, progress_tx, use_verify, use_subgraph) = {
            let g = store().lock().unwrap();
            let s = g.as_ref().unwrap().get(sid);
            let ctx = s.map(|s| s.done.clone()).unwrap_or_default();
            let tx = s.and_then(|s| s.progress_tx.clone());
            let verify = s.map(|s| s.use_verify).unwrap_or(false);
            let sub = s.map(|s| s.use_subgraph).unwrap_or(false);
            (ctx, tx, verify, sub)
        };
        let results = Self::run_batch_with_framework(config, &task, &batch, &context, &rag_context, vector_store, Some(get_cancel_flag(sid)), progress_tx, use_verify, use_subgraph, mcp_bridge).await?;

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
                // 路由逻辑：跳过未选中的分支
                let route_members_set = Self::route_members(&all);
                for r in &results {
                    let task_def = match all.iter().find(|t| t.name == r.task_name) {
                        Some(t) if t.task_type == "decision" => t,
                        _ => continue,
                    };
                    let decided = match Self::match_route_key(&r.output, &task_def.routes) {
                        Some(d) => d,
                        None => { tracing::info!("决策「{}」未匹配 route key", r.output); continue; }
                    };
                    for (route_key, next_tasks) in &task_def.routes {
                        if route_key != &decided {
                            for task_name in next_tasks {
                                Self::skip_downstream(&all, task_name, &route_members_set, &mut s.completed_names, &mut s.done);
                            }
                        }
                    }
                    for task in &all {
                        if task.depends_on.contains(&r.task_name) && !route_members_set.contains(&task.name) && task.depends_on.len() == 1 && task.tool == "web_search" {
                            Self::skip_downstream(&all, &task.name, &route_members_set, &mut s.completed_names, &mut s.done);
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
        let keywords = ["不充分", "不通过", "not enough", "enough", "充分", "通过", "yes", "no", "tech", "general", "other"];
        for keyword in &keywords {
            if lower.contains(keyword) {
                return Some(keyword.to_string());
            }
        }
        if lower.contains("充足") || lower.contains("足够") || lower.contains("够") {
            return Some("充分".to_string());
        }
        if lower.contains("不足") || lower.contains("缺少") {
            return Some("不充分".to_string());
        }
        None
    }

    /// 用决策输出原文匹配 route key
    fn match_route_key(output: &str, routes: &HashMap<String, Vec<String>>) -> Option<String> {
        tracing::info!("match_route_key: output={}, routes={:?}", output, routes);
        let lower = output.to_lowercase();
        // 1. 精确匹配：route key 出现在输出中
        for key in routes.keys() {
            if lower.contains(&key.to_lowercase()) {
                tracing::info!("  -> exact match: {}", key);
                return Some(key.clone());
            }
        }
        // 2. 判断是充分还是不充分，按 routes 的 value 空/非空来匹配
        let is_insufficient = lower.contains("不充分") || lower.contains("不足") || lower.contains("缺少")
            || lower.contains("不够") || lower.contains("需要补充") || lower.contains("还需")
            || lower.contains("信息不足");
        let is_sufficient = !is_insufficient && (lower.contains("足够") || lower.contains("充足") || lower.contains("够")
            || lower.contains("不需要") || lower.contains("无需") || lower.contains("不必")
            || lower.contains("已足够") || lower.contains("已经可以") || lower.contains("够了")
            || lower.contains("信息充分") || lower.contains("不需补充") || lower.contains("不补充")
            || lower.contains("充分") || lower.contains("满足") || lower.contains("已满足")
            || lower.contains("可以了"));
        if is_sufficient {
            // 找个空 value 的 route（充分分支不需要额外任务）
            for (k, v) in routes {
                if v.is_empty() { tracing::info!("  -> sufficient, matched empty route: {}", k); return Some(k.clone()); }
            }
            // 没有空 value 的，返回第一个 key
            if let Some(k) = routes.keys().next() { tracing::info!("  -> sufficient, first route: {}", k); return Some(k.clone()); }
        }
        if is_insufficient {
            // 找个非空 value 的 route（不充分分支需要补充搜索）
            for (k, v) in routes {
                if !v.is_empty() { tracing::info!("  -> insufficient, matched non-empty route: {}", k); return Some(k.clone()); }
            }
            if let Some(k) = routes.keys().next() { tracing::info!("  -> insufficient, first route: {}", k); return Some(k.clone()); }
        }
        // 3. parse_decision fallback
        let fallback = Self::parse_decision(output);
        tracing::info!("  -> parse_decision fallback: {:?}", fallback);
        fallback
    }

    /// 收集所有路由成员（出现在任意 routes value 中的任务名）
    fn route_members(tasks: &[AgentTask]) -> HashSet<String> {
        let mut members = HashSet::new();
        for t in tasks {
            for (_key, next_tasks) in &t.routes {
                for task_name in next_tasks {
                    members.insert(task_name.clone());
                }
            }
        }
        members
    }

    /// 跳过指定任务及其下游路由成员（汇总任务不受影响）
    fn skip_downstream(tasks: &[AgentTask], start: &str, route_members: &HashSet<String>, done: &mut HashSet<String>, results: &mut Vec<AgentExecResult>) {
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
            llm_prompt: String::new(),
            api_request: String::new(),
            llm_raw: String::new(),
        });
        // 只级联跳过同样在路由中的下游任务
        for task in tasks {
            if task.depends_on.contains(&start.to_string()) && route_members.contains(&task.name) {
                Self::skip_downstream(tasks, &task.name, route_members, done, results);
            }
        }
    }

    /// ── 按依赖分批并行执行：同批任务 invoke_parallel 并行，不同批串行 ──
    /// 例: A → [B, C] → D  => 第一批: A, 第二批: B+C(并行), 第三批: D
    pub async fn execute_all_batches(config: &Config, task: String, agent_tasks: Vec<AgentTask>, rag_context: String, vector_store: Option<Arc<QdrantStore>>, use_verify: bool, use_subgraph: bool, mcp_bridge: Option<Arc<McpBridge>>) -> Result<AgentExecResponse, GraphDemoError> {
        let total_start = Instant::now();

        let mut done: HashSet<String> = HashSet::new();
        let mut all_results: Vec<AgentExecResult> = Vec::new();

        while done.len() < agent_tasks.len() {
            let batch = Self::ready_batch(&agent_tasks, &done);
            if batch.is_empty() {
                break;
            }

            let results = Self::run_batch_with_framework(config, &task, &batch, &all_results, &rag_context, vector_store.clone(), None, None, use_verify, use_subgraph, mcp_bridge.clone()).await?;

            for r in &results {
                done.insert(r.task_name.clone());
                all_results.push(r.clone());

                // 决策节点：跳过未选中的分支
                let task_def = match agent_tasks.iter().find(|t| t.name == r.task_name) {
                    Some(t) if t.task_type == "decision" => t,
                    _ => continue,
                };
                let route_members_set = Self::route_members(&agent_tasks);
                let decided = match Self::match_route_key(&r.output, &task_def.routes) {
                    Some(d) => d,
                    None => { tracing::info!("决策「{}」未匹配 route key", r.output); continue; }
                };
                for (route_key, next_tasks) in &task_def.routes {
                    if route_key != &decided {
                        for task_name in next_tasks {
                            Self::skip_downstream(&agent_tasks, task_name, &route_members_set, &mut done, &mut all_results);
                        }
                    }
                }
                for task in &agent_tasks {
                    if task.depends_on.contains(&r.task_name) && !route_members_set.contains(&task.name) && task.depends_on.len() == 1 && task.tool == "web_search" {
                        Self::skip_downstream(&agent_tasks, &task.name, &route_members_set, &mut done, &mut all_results);
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
                llm_prompt: String::new(),
                api_request: String::new(),
                llm_raw: String::new(),
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
                llm_prompt: String::new(),
                api_request: String::new(),
                llm_raw: String::new(),
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
    pub async fn plan_and_execute(config: &Config, task: String, mcp_bridge: Option<Arc<McpBridge>>) -> Result<AgentExecResponse, GraphDemoError> {
        let plan = Self::plan(config, task.clone(), String::new(), false, false, false, mcp_bridge.as_deref(), None).await?;
        let (sid, mut all, _) = Self::execute_batch_start(config, task, plan.tasks, String::new(), None, None, false, false, mcp_bridge.clone()).await?;
        loop {
            let (batch, has_more) = Self::execute_batch_next(config, &sid, None, None, mcp_bridge.clone()).await?;
            all.extend(batch);
            if !has_more { break; }
        }
        Self::batch_finalize(&sid, None).await
    }

    /// ── 向正在执行的 session 注入新任务 ──
    /// 获取当前已完成的结果作为 context，调 LLM 规划子任务，
    /// 合并到 BatchState.all 中，后续 execute_batch_next 会自然拾取。
    pub async fn inject_new_tasks(
        config: &Config,
        sid: &str,
        new_task: String,
        mcp_bridge: Option<&McpBridge>,
    ) -> Result<crate::models::InjectResponse, GraphDemoError> {
        // 1. 获取当前 session 状态
        let (original_task, all, done, completed_names, has_review) = {
            let g = store().lock().unwrap();
            let m = g.as_ref().ok_or_else(|| GraphDemoError::BuildError("没有正在执行的 session".into()))?;
            let s = m.get(sid).ok_or_else(|| GraphDemoError::BuildError("session 不存在".into()))?;
            (
                s.task.clone(),
                s.all.clone(),
                s.done.clone(),
                s.completed_names.clone(),
                !s.review_pending.is_empty(),
            )
        };

        if has_review {
            return Err(GraphDemoError::BuildError("有待审批的人工审核任务，请先审批".into()));
        }

        // 2. 构建 context（已完成任务摘要）
        let done_summary: String = done.iter()
            .map(|r| format!("【{}】\n{}", r.task_name, r.output.chars().take(200).collect::<String>()))
            .collect::<Vec<_>>().join("\n\n");
        let pending_names: Vec<String> = all.iter()
            .filter(|t| !completed_names.contains(&t.name))
            .map(|t| t.name.clone())
            .collect();

        // 3. 调 LLM 规划新任务的子任务
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));
        let registry = crate::services::tools::ToolRegistry::default_registry();
        let mut tool_entries: Vec<String> = registry.list_descriptions().iter()
            .map(|(n,d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}", n, d))
            .collect();
        if let Some(bridge) = mcp_bridge {
            for adapter in bridge.get_adapter_boxes() {
                tool_entries.push(format!(
                    "{{\"name\":\"{}\",\"description\":\"{}\"}}",
                    adapter.name(),
                    adapter.description()
                ));
            }
        }
        let tj = tool_entries.join(",");

        let prompt = format!(
            r#"你正在执行一个多步骤任务。用户在执行过程中提出了新需求。

原始任务：{original}

已完成的任务及结果：
{done_summary}

还未执行的任务：{pending}

新增需求：{new_task}

要求：
- 基于已完成的结果和新增需求，规划子任务来满足新需求
- 已完成的任务不要重复
- 可以用 depends_on 引用已有任务（包括已完成的和未完成的）
- 如果新任务需要已有结果，depends_on 设为对应已完成任务名
- depends_on 为空的子任务会立即执行
- 每个子任务分配一个工具
- 可用工具：[{tools}]
- 返回 JSON 数组，只返回 JSON

返回格式：
[{{"name":"子任务名","description":"做什么","tool":"工具名","depends_on":["前置任务名"],"task_type":"normal","input_template":"需要什么"}}]
"#,
            original = original_task,
            done_summary = done_summary,
            pending = pending_names.join(", "),
            new_task = new_task,
            tools = tj,
        );

        let r = llm.invoke(vec![Message::human(&prompt)], None).await
            .map_err(|e| GraphDemoError::ExecutionError(format!("LLM 失败: {}", e)))?;
        let cleaned = r.content
            .trim_start_matches("```json").trim_start_matches("```")
            .trim_end_matches("```").trim();

        let mut new_tasks: Vec<AgentTask> = serde_json::from_str(cleaned)
            .map_err(|e| GraphDemoError::BuildError(format!(
                "LLM 返回格式错误: {} — 原始内容: {}",
                e, &cleaned.chars().take(300).collect::<String>()
            )))?;

        if new_tasks.is_empty() {
            return Err(GraphDemoError::BuildError("LLM 未返回子任务".into()));
        }

        // 4. 合并到 session 中
        let mut all_names: std::collections::HashSet<String> =
            all.iter().map(|t| t.name.clone()).collect();

        // 去重：跳过名称已存在的任务
        new_tasks.retain(|t| !all_names.contains(&t.name));
        for t in &new_tasks {
            all_names.insert(t.name.clone());
        }

        // 5. 已完成的依赖自动满足
        let new_completed = completed_names.clone();

        // 6. 新任务若 `depends_on` 为空，设为最后一个已完成任务，确保图渲染在正确层级
        //    不影响执行：该依赖已在 completed_names 中，ready_batch 会认为已满足
        let last_done_name = {
            let mut dn: Vec<String> = done.iter().map(|r| r.task_name.clone()).collect();
            dn.sort();
            dn.dedup();
            dn.last().cloned()
        };
        for t in &mut new_tasks {
            if t.depends_on.is_empty() {
                if let Some(ref last) = last_done_name {
                    t.depends_on.push(last.clone());
                }
            }
        }

        // 7. 构建图结构
        let graph_structure = Self::build_graph_with_subgraph(
            &[all.as_slice(), new_tasks.as_slice()].concat(),
            false,
        );

        let has_more = {
            let mut g = store().lock().unwrap();
            let m = g.as_mut().ok_or_else(|| GraphDemoError::BuildError("session 已结束".into()))?;
            let s = m.get_mut(sid).ok_or_else(|| GraphDemoError::BuildError("session 不存在".into()))?;
            s.all.extend(new_tasks.clone());
            s.completed_names = new_completed;
            s.review_pending.is_empty() && s.completed_names.len() < s.all.len()
        };

        Ok(crate::models::InjectResponse {
            injected_tasks: new_tasks,
            graph_structure,
            has_more,
        })
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
            llm_prompt: String::new(), api_request: String::new(), llm_raw: String::new(),
        }];
        let completed: HashSet<String> = ["A"].into_iter().map(|s| s.to_string()).collect();

        let sid = uuid::Uuid::new_v4().to_string();
        store().lock().unwrap().get_or_insert_with(HashMap::new).insert(sid.clone(), BatchState {
            task, all, done, completed_names: completed, start: Instant::now(), rag_context: String::new(), review_pending: Vec::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            progress_tx: None,
            use_verify: false,
            use_subgraph: false,
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
        let graph1 = AgentEngine::build_batch_graph(config.clone(), "test".into(), batch1, String::new(), String::new(), None, 3, None, None, None);
        assert!(graph1.is_ok(), "单任务批次应该编译成功");
        let compiled = graph1.unwrap();
        let nodes = compiled.node_names();
        assert!(nodes.contains(&"A".to_string()));
        assert!(nodes.contains(&"__dispatch__".to_string()));

        // 多任务
        let batch2 = vec![make_task("A", vec![]), make_task("B", vec![]), make_task("C", vec![])];
        let graph2 = AgentEngine::build_batch_graph(config, "test".into(), batch2, String::new(), String::new(), None, 3, None, None, None);
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
            None,
            None,
        );
        assert!(graph.is_ok(), "批次图应该编译成功");
    }
}
