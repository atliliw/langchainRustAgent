//! 对话相关数据模型
//!
//! 定义了聊天 API 的所有请求/响应数据结构

use serde::{Deserialize, Serialize};

// ────────────────────── 请求 ──────────────────────

/// 对话请求
/// 前端 POST /api/chat 时发送的 JSON 格式
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatRequest {
    pub session_id: Option<String>,   // 会话ID（空=新建会话）
    pub message: String,              // 用户消息
    #[serde(default)]
    pub use_vector: bool,             // 是否启用向量检索
    #[serde(default)]
    pub use_bm25: bool,               // 是否启用 BM25 检索
    #[serde(default = "default_top_k")]
    pub top_k: usize,                 // 检索返回几条结果（默认3）
    #[serde(default = "default_compress_mode")]
    pub compress_mode: String,        // 历史压缩模式（默认"none"）
}

fn default_top_k() -> usize { 3 }
fn default_compress_mode() -> String { "none".to_string() }

// ────────────────────── 压缩模式 ──────────────────────

/// 压缩模式枚举
/// 控制对话历史怎么压缩后再发给 LLM
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompressMode {
    None,                              // 不压缩
    SlidingWindow(Option<usize>),       // 滑动窗口：保留最近 N 条
    TokenLimit(Option<usize>),          // Token 限制
    Summary(Option<usize>),             // 摘要压缩
    Layered,                            // 分层压缩（重要+摘要+最近）
    AdaptiveFocus(Option<usize>),       // AFM 自适应保真度（LLM 重要性评分）
    TopicSegment,                        // 话题分段压缩（按话题切分，独立摘要）
}

impl CompressMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "none" => CompressMode::None,
            "sliding_window" => CompressMode::SlidingWindow(None),
            "summary" => CompressMode::Summary(None),
            "layered" => CompressMode::Layered,
            "afm" | "adaptive_focus" => CompressMode::AdaptiveFocus(None),
            "topic" | "topic_segment" | "episodic" => CompressMode::TopicSegment,
            _ => {
                if s.starts_with("sliding_window_") {
                    let num = s.trim_start_matches("sliding_window_").parse().ok();
                    CompressMode::SlidingWindow(num)
                } else if s.starts_with("token_limit_") {
                    let num = s.trim_start_matches("token_limit_").parse().ok();
                    CompressMode::TokenLimit(num)
                } else if s.starts_with("summary_") {
                    let num = s.trim_start_matches("summary_").parse().ok();
                    CompressMode::Summary(num)
                } else if s.starts_with("afm_") || s.starts_with("adaptive_focus_") {
                    let key = if s.starts_with("afm_") { "afm_" } else { "adaptive_focus_" };
                    let num = s.trim_start_matches(key).parse().ok();
                    CompressMode::AdaptiveFocus(num)
                } else {
                    CompressMode::None
                }
            }
        }
    }
}

// ────────────────────── 检索模式 ──────────────────────

/// 检索模式
/// 决定本次对话要不要检索知识库，以及用什么方式检索
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchMode {
    None,    // 不检索，纯 LLM 对话
    Vector,  // 向量检索（语义匹配）
    BM25,    // BM25 检索（关键词匹配）
    Hybrid,  // 混合检索（RRF 融合）
}

impl SearchMode {
    /// 根据前端传的两个布尔值决定检索模式
    pub fn from_flags(use_vector: bool, use_bm25: bool) -> Self {
        match (use_vector, use_bm25) {
            (false, false) => SearchMode::None,
            (true, false) => SearchMode::Vector,
            (false, true) => SearchMode::BM25,
            (true, true) => SearchMode::Hybrid,
        }
    }
}

// ────────────────────── 响应 ──────────────────────

/// 对话响应
/// LLM 生成的回答 + 引用的来源 + 压缩信息
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    pub session_id: String,            // 会话ID
    pub reply: String,                 // LLM 生成的回答
    pub sources: Vec<SourceInfo>,       // 引用的来源（RAG检索到的文档）
    pub compressed: bool,              // 是否压缩了历史
    pub compression_info: Option<String>, // 压缩说明
}

/// RAG 检索到的来源信息
/// 每条来源 = 文档内容 + 相似度分数 + 来源类型
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SourceInfo {
    pub content: String,   // 文档内容
    pub score: f32,        // 相似度分数
    pub source: String,    // 来源：vector/bm25/hybrid
}

/// 会话列表中的一条
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,    // 会话ID
    pub title: String,         // 标题
    pub created_at: String,    // 创建时间
    pub message_count: usize,  // 消息数
    pub preview: String,       // 预览
}

/// 一条对话消息
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationMessage {
    pub id: String,            // 消息ID
    pub session_id: String,    // 所属会话
    pub role: String,          // 角色: user/assistant/summary
    pub content: String,       // 消息内容
    pub tokens: i64,           // token数
    pub time_created: i64,     // 创建时间（毫秒时间戳）
}

/// 数据库中的会话
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub message_count: i64,
    pub tokens_used: i64,
    pub time_created: i64,
    pub time_updated: i64,
}

/// 压缩模式信息（前端展示说明用）
#[derive(Debug, Serialize, Deserialize)]
pub struct CompressModeInfo {
    pub name: String,         // 英文名
    pub label: String,        // 中文标签
    pub description: String,  // 说明
}

// ────────────────────── 编辑/重生成 ──────────────────────

/// 编辑消息请求
#[derive(Debug, Serialize, Deserialize)]
pub struct EditMessageRequest {
    pub content: String,  // 修改后的内容
}

/// 重生成响应
#[derive(Debug, Serialize, Deserialize)]
pub struct RegenerateResponse {
    pub message_id: String,  // 新消息ID
    pub reply: String,       // 新生成的回答
}

// ────────────────────── 导入/导出 ──────────────────────

/// 会话导出（完整会话 + 所有消息）
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionExport {
    pub session_id: String,
    pub title: String,
    pub created_at: String,
    pub messages: Vec<ConversationMessage>,
}

/// 会话导入请求
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionImport {
    pub title: Option<String>,        // 可选的新标题
    pub messages: Vec<ImportMessage>,  // 要导入的消息
}

/// 导入格式中的一条消息
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportMessage {
    pub role: String,     // user 或 assistant
    pub content: String,  // 消息内容
}

// ────────────────────── 搜索/分支 ──────────────────────

/// 会话搜索请求
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSearchRequest {
    pub query: String,  // 搜索词
}

/// 分支会话请求
#[derive(Debug, Serialize, Deserialize)]
pub struct BranchRequest {
    pub session_id: String,      // 原始会话
    pub from_message_id: String, // 从此消息分叉
}

/// 分支会话响应
#[derive(Debug, Serialize, Deserialize)]
pub struct BranchResponse {
    pub new_session_id: String,  // 新会话ID
    pub title: String,           // 标题
    pub message_count: usize,    // 消息数
}

/// 会话列表请求
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionListRequest {
    pub page: Option<usize>,   // 页码
    pub limit: Option<usize>,  // 每页条数
}
