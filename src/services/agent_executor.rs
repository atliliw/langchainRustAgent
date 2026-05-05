use crate::config::Config;
use crate::errors::GraphDemoError;
use crate::models::*;
use langchainrust::{
    language_models::OpenAIChat, schema::Message, core::runnables::Runnable,
    StateGraph, START, END, AgentState, StateUpdate, MemoryCheckpointer,
    langgraph::{GraphError, CompiledGraph, GraphExecution},
};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Instant;

pub const AVAILABLE_TOOLS: &[(&str, &str)] = &[
    ("llm_query","直接用 LLM 回答"),("web_search","搜索网络"),
    ("code_execute","执行代码"),("read_file","读取文件"),("summarize","总结"),
];

static EXEC_STORE: Mutex<Option<HashMap<String, StoredExec>>> = Mutex::new(None);
fn with_store<F,R>(f: F) -> R where F: FnOnce(&mut HashMap<String, StoredExec>) -> R {
    f(Mutex::lock(&EXEC_STORE).unwrap().get_or_insert_with(HashMap::new))
}

struct StoredExec {
    original_task: String,
    agent_tasks: Vec<AgentTask>,
    checkpoint: Option<GraphExecution<AgentState>>,
}

pub struct AgentEngine;

impl AgentEngine {
    pub async fn plan(config: &Config, task: String) -> Result<AgentPlan, GraphDemoError> {
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(2048));
        let tj = AVAILABLE_TOOLS.iter().map(|(n,d)| format!("{{\"name\":\"{}\",\"description\":\"{}\"}}",n,d)).collect::<Vec<_>>().join(",");
        let p = format!("将任务拆解为2-5个子任务并分配工具。可用工具：[{}] 返回JSON：[{{\"name\":\"中文子任务名（不要英文）\",\"description\":\"做什么\",\"tool\":\"工具名\",\"depends_on\":[\"前置\"],\"input_template\":\"需要什么\"}}] 任务：{} 只返回JSON。", tj, task);
        let r = llm.invoke(vec![Message::human(&p)], None).await.map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        let c = r.content.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        let tasks: Vec<AgentTask> = serde_json::from_str(c).map_err(|e| GraphDemoError::BuildError(format!("格式错误: {} — {}", e, &c.chars().take(200).collect::<String>())))?;
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
            let t = &tasks[i];
            for d in &t.depends_on { if nodes.contains(d) { edges.push(serde_json::json!({"type":"fixed","source":d,"target":t.name})); } }
            edges.push(serde_json::json!({"type":"fixed","source":t.name,"target":"__end__"}));
        }
        serde_json::json!({"entry_point":tasks[0].name,"nodes":nodes,"edges":edges,"routers":[]})
    }

    fn build_graph_exec(config: &Config, task: &str, agent_tasks: &[AgentTask]) -> Result<CompiledGraph<AgentState>, GraphDemoError> {
        let llm = Arc::new(OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024)));
        let mut graph: StateGraph<AgentState> = StateGraph::new();
        let ns: std::collections::HashSet<&str> = agent_tasks.iter().map(|t| t.name.as_str()).collect();

        for at in agent_tasks {
            let n = at.name.clone(); let d = at.description.clone();
            let tool = at.tool.clone(); let tmpl = at.input_template.clone();
            let l = llm.clone(); let t = task.to_string();
            graph.add_async_node(n.clone(), move |state: &AgentState| {
                let s=state.clone(); let nn=n.clone(); let dd=d.clone();
                let tool=tool.clone(); let tmpl=tmpl.clone(); let l=l.clone(); let t=t.clone();
                async move {
                    let ctx: String = s.messages.iter().map(|m| match m.role {
                        langchainrust::MessageRole::AI => format!("[结果]{}", m.content),
                        _ => format!("[输入]{}", m.content),
                    }).collect::<Vec<_>>().join("\n");
                    let p = format!("任务：{t}\n子任务：{dd}\n工具：{tool}\n输入：{tmpl}\n\n上下文：\n{ctx}\n\n输出。", t=t, dd=dd, tool=tool, tmpl=tmpl, ctx=ctx);
                    let r = l.invoke(vec![Message::human(&p)], None).await.map_err(|e| GraphError::NodeError(e.to_string()))?;
                    let o = r.content.chars().take(500).collect::<String>();
                    let mut ns2 = s;
                    ns2.add_message(langchainrust::MessageEntry::ai(format!("[{}]{}", nn, o)));
                    ns2.add_step(langchainrust::StepEntry::new(nn, o.clone()));
                    Ok(StateUpdate::full(ns2))
                }
            });
        }

        // 收集依赖信息（先拷贝，避免借用冲突）
        let deps_list: Vec<(String, Vec<String>)> = agent_tasks.iter().map(|t| (t.name.clone(), t.depends_on.clone())).collect();
        // 链式边（保证遍历顺序）+ 依赖边
        let mut seen = std::collections::HashSet::<(String,String)>::new();
        graph.add_edge(START, &agent_tasks[0].name);
        seen.insert((START.to_string(), agent_tasks[0].name.clone()));
        for i in 1..agent_tasks.len() {
            let k = (agent_tasks[i-1].name.clone(), agent_tasks[i].name.clone());
            if seen.insert(k) { graph.add_edge(&agent_tasks[i-1].name, &agent_tasks[i].name); }
        }

        for (name, deps) in &deps_list {
            for d in deps { if ns.contains(d.as_str()) { let k=(d.clone(),name.clone()); if seen.insert(k) { graph.add_edge(d, name); } } }
        }

        // 不被依赖的节点 → END
        let refd: std::collections::HashSet<&str> = deps_list.iter().flat_map(|(_,deps)| deps.iter().map(|d| d.as_str())).filter(|d| ns.contains(d)).collect();
        for (name, _) in &deps_list { if !refd.contains(name.as_str()) { let k=(name.clone(),END.to_string()); if seen.insert(k) { graph.add_edge(name, END); } } }

        let names: Vec<String> = agent_tasks.iter().map(|t| t.name.clone()).collect();
        let c = graph.compile().map_err(|e| GraphDemoError::BuildError(e.to_string()))?;
        Ok(c.with_checkpointer(MemoryCheckpointer::new()).with_interrupt_after(names))
    }

    pub async fn execute_start(config: &Config, task: String, agent_tasks: Vec<AgentTask>) -> Result<(String, AgentExecResult, bool), GraphDemoError> {
        let sid = uuid::Uuid::new_v4().to_string();
        Self::step(&config, &sid, &task, &agent_tasks, None).await
    }

    pub async fn execute_next(config: &Config, sid: &str) -> Result<(AgentExecResult, bool), GraphDemoError> {
        let (task, agent_tasks, cp) = with_store(|m| {
            let s = m.get(sid).ok_or_else(|| GraphDemoError::BuildError("session不存在".to_string()))?;
            Ok::<_,GraphDemoError>((s.original_task.clone(), s.agent_tasks.clone(), s.checkpoint.clone()))
        })?;
        let (_, result, has_next) = Self::step(config, sid, &task, &agent_tasks, cp).await?;
        Ok((result, has_next))
    }

    async fn step(config: &Config, sid: &str, task: &str, agent_tasks: &[AgentTask], resume: Option<GraphExecution<AgentState>>) -> Result<(String, AgentExecResult, bool), GraphDemoError> {
        let compiled = Self::build_graph_exec(config, task, agent_tasks)?;
        let start = Instant::now();

        let result = if let Some(exec) = resume { compiled.resume(exec).await } else { compiled.invoke(AgentState::new(task.to_string())).await };

        match result {
            Ok(inv) => {
                with_store(|m| { m.remove(sid); });
                let results: Vec<AgentExecResult> = inv.final_state.steps.iter().filter_map(|s| {
                    agent_tasks.iter().find(|t| t.name==s.action).map(|at| AgentExecResult{
                        task_name: s.action.clone(), tool: at.tool.clone(),
                        input_summary: at.input_template.clone(), output: s.observation.clone(),
                        duration_ms: 0, tokens: 0,
                    })
                }).collect();
                let last = results.into_iter().last().unwrap();
                Ok((sid.to_string(), last, false))
            }
            Err(GraphError::ExecutionInterrupted(after)) => {
                // after 格式: "after_taskname"，提取 taskname
                let node = after.strip_prefix("after_").unwrap_or(&after).to_string();
                // 从 checkpoint 获取刚完成的节点的执行结果
                let last_output = compiled.last_checkpoint_state().await
                    .and_then(|s| s.steps.last().cloned())
                    .map(|step| step.observation)
                    .unwrap_or_default();

                let cp = compiled.create_resume_execution(&after).await;
                with_store(|m| { m.insert(sid.to_string(), StoredExec{
                    original_task: task.to_string(), agent_tasks: agent_tasks.to_vec(),
                    checkpoint: cp,
                }); });
                let has_more = agent_tasks.iter().any(|t| t.name != node);
                let result = AgentExecResult{
                    task_name: node, tool: String::new(), input_summary: String::new(),
                    output: last_output,
                    duration_ms: start.elapsed().as_millis() as u64, tokens: 0,
                };
                Ok((sid.to_string(), result, has_more))
            }
            Err(e) => Err(GraphDemoError::ExecutionError(e.to_string())),
        }
    }

    pub async fn finalize(config: &Config, sid: &str) -> Result<AgentExecResponse, GraphDemoError> {
        let task = with_store(|m| m.get(sid).map(|s| s.original_task.clone()).unwrap_or_default());
        with_store(|m| { m.remove(sid); });
        let v = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(1024));
        let vp = format!("任务：{} 已完成。请确认结果。返回JSON：{{\"completed\":true,\"final_answer\":\"答案\"}}", task);
        let vr = v.invoke(vec![Message::human(&vp)], None).await.map_err(|e| GraphDemoError::ExecutionError(e.to_string()))?;
        let c = vr.content.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
        #[derive(serde::Deserialize)] struct R { #[serde(default)]final_answer: String }
        let r: R = serde_json::from_str(c).unwrap_or(R{final_answer:"完成".into()});
        Ok(AgentExecResponse{results:vec![],final_answer:r.final_answer,total_duration_ms:0,total_tokens:0})
    }
}
