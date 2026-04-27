use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LangGraphRequest {
    pub input: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParallelDemoResult {
    pub input: String,
    pub parallel_tasks: Vec<ParallelTaskResult>,
    pub merged_result: String,
    pub total_time_ms: u64,
    pub sequential_time_estimate_ms: u64,
    pub time_saved_percent: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParallelTaskResult {
    pub task_name: String,
    pub result: String,
    pub duration_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConditionalDemoResult {
    pub input: String,
    pub route_decision: String,
    pub path_taken: String,
    pub output: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StreamDemoEvent {
    pub node_name: String,
    pub event_type: String,
    pub timestamp_ms: u64,
    pub state_snapshot: Option<StateSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub input: String,
    pub output: Option<String>,
    pub messages: Vec<String>,
}
