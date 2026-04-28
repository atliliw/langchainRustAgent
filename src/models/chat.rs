use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatRequest {
    pub session_id: Option<String>,
    pub message: String,
    #[serde(default)]
    pub use_vector: bool,
    #[serde(default)]
    pub use_bm25: bool,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default = "default_compress_mode")]
    pub compress_mode: String,
}

fn default_top_k() -> usize {
    3
}
fn default_compress_mode() -> String {
    "none".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressMode {
    None,
    SlidingWindow,
    TokenLimit,
    Summary,
    Layered,
}

impl CompressMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "sliding_window" => CompressMode::SlidingWindow,
            "token_limit" => CompressMode::TokenLimit,
            "summary" => CompressMode::Summary,
            "layered" => CompressMode::Layered,
            _ => CompressMode::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchMode {
    None,
    Vector,
    BM25,
    Hybrid,
}

impl SearchMode {
    pub fn from_flags(use_vector: bool, use_bm25: bool) -> Self {
        match (use_vector, use_bm25) {
            (false, false) => SearchMode::None,
            (true, false) => SearchMode::Vector,
            (false, true) => SearchMode::BM25,
            (true, true) => SearchMode::Hybrid,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    pub session_id: String,
    pub reply: String,
    pub sources: Vec<SourceInfo>,
    pub compressed: bool,
    pub compression_info: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SourceInfo {
    pub content: String,
    pub score: f32,
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub title: String,
    pub created_at: String,
    pub message_count: usize,
    pub preview: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tokens: i64,
    pub time_created: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub message_count: i64,
    pub tokens_used: i64,
    pub time_created: i64,
    pub time_updated: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompressModeInfo {
    pub name: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EditMessageRequest {
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegenerateResponse {
    pub message_id: String,
    pub reply: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionExport {
    pub session_id: String,
    pub title: String,
    pub created_at: String,
    pub messages: Vec<ConversationMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionImport {
    pub title: Option<String>,
    pub messages: Vec<ImportMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSearchRequest {
    pub query: String,
}
