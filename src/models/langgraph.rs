use serde::{Deserialize, Serialize};

/// LangGraph 演示请求
#[derive(Deserialize)]
pub struct LangGraphRequest {
    pub input: String,  // 用户输入
}

/// LangGraph 图结构请求
#[derive(Deserialize)]
pub struct LangGraphStructureRequest {
    pub mode: String,  // "parallel" | "conditional" | "stream"
}

/// LangGraph 图结构响应（含 Mermaid 语法）
#[derive(Debug, Serialize, Deserialize)]
pub struct LangGraphStructureResponse {
    pub mode: String,
    pub mermaid: String,
    pub structure: serde_json::Value,
}

/// 任务拆解请求
#[derive(Deserialize)]
pub struct TaskDecomposeRequest {
    pub task: String,
}

/// 子任务定义（LLM 拆解结果）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubTaskDef {
    pub name: String,
    pub description: String,
    pub depends_on: Vec<String>,
}

/// 子任务执行结果
#[derive(Debug, Serialize, Deserialize)]
pub struct SubTaskExecResult {
    pub name: String,
    pub output: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub tokens: usize,
}

/// 任务拆解结果（不含执行）
#[derive(Debug, Serialize, Deserialize)]
pub struct TaskDecomposeResult {
    pub original_task: String,
    pub sub_tasks: Vec<SubTaskDef>,
    pub graph_structure: serde_json::Value,
}

/// 执行请求
#[derive(Deserialize)]
pub struct TaskExecuteRequest {
    pub task: String,
    pub sub_tasks: Vec<SubTaskDef>,
}

/// 执行结果
#[derive(Debug, Serialize, Deserialize)]
pub struct TaskExecuteResult {
    pub execution_results: Vec<SubTaskExecResult>,
}

/// 并行执行结果
#[derive(Debug, Serialize, Deserialize)]
pub struct ParallelDemoResult {
    pub input: String,
    pub parallel_tasks: Vec<ParallelTaskResult>,
    pub merged_result: String,
    pub total_time_ms: u64,
    pub sequential_time_estimate_ms: u64,
    pub time_saved_percent: f32,
}

/// 单个并行任务的结果
#[derive(Debug, Serialize, Deserialize)]
pub struct ParallelTaskResult {
    pub task_name: String,
    pub result: String,
    pub duration_ms: u64,
}

/// 条件路由结果
#[derive(Debug, Serialize, Deserialize)]
pub struct ConditionalDemoResult {
    pub input: String,
    pub route_decision: String,
    pub path_taken: String,
    pub output: String,
    pub steps: Vec<String>,
}

/// 流式执行中的一条事件
#[derive(Debug, Serialize, Deserialize)]
pub struct StreamDemoEvent {
    pub node_name: String,
    pub event_type: String,
    pub timestamp_ms: u64,
    pub state_snapshot: Option<StateSnapshot>,
}

/// 执行状态快照
#[derive(Debug, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub input: String,
    pub output: Option<String>,
    pub messages: Vec<String>,
}

/// ──────── 真实 Agent 系统模型 ────────

/// 工具定义
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ToolParam>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolParam {
    pub name: String,
    pub r#type: String,
    pub description: String,
}

/// Agent 规划中的单个任务节点
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentTask {
    pub name: String,
    pub description: String,
    pub tool: String,
    pub depends_on: Vec<String>,
    pub input_template: String,
}

/// Agent 规划结果
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentPlan {
    pub original_task: String,
    pub tasks: Vec<AgentTask>,
    pub graph_structure: serde_json::Value,
}

/// Agent 执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecResult {
    pub task_name: String,
    pub tool: String,
    pub input_summary: String,
    pub output: String,
    pub duration_ms: u64,
    pub tokens: usize,
}

/// Agent 执行响应
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentExecResponse {
    pub results: Vec<AgentExecResult>,
    pub final_answer: String,
    pub total_duration_ms: u64,
    pub total_tokens: usize,
}

/// 逐步执行：单步结果
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentStepResult {
    pub session_id: String,
    pub result: AgentExecResult,
    pub has_next: bool,
    pub is_final: bool,
}

/// 逐步执行：继续请求
#[derive(Deserialize)]
pub struct AgentStepRequest {
    pub session_id: String,
}
