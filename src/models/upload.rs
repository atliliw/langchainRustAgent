use serde::{Deserialize, Serialize};

/// 文档切分策略
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ChunkStrategy {
    /// 递归分割（默认）：按段落→行→句→字符递归切分，chunk_size=500, overlap=50
    Recursive,
    /// 大块：适合需要长上下文理解的文档，chunk_size=1000, overlap=100
    Large,
    /// 小块：适合精准检索场景，chunk_size=200, overlap=30
    Small,
    /// 段落：按段落分割，保留完整段落，chunk_size=1500, overlap=0
    Paragraph,
    /// Token 分割：按 token 数切分，精确控制上下文窗口
    Token,
    /// 语义分割：用 Embedding 检测话题边界，按语义切分
    Semantic,
}

impl Default for ChunkStrategy {
    fn default() -> Self {
        Self::Recursive
    }
}

impl ChunkStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChunkStrategy::Recursive => "recursive",
            ChunkStrategy::Large => "large",
            ChunkStrategy::Small => "small",
            ChunkStrategy::Paragraph => "paragraph",
            ChunkStrategy::Token => "token",
            ChunkStrategy::Semantic => "semantic",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "large" => Self::Large,
            "small" => Self::Small,
            "paragraph" => Self::Paragraph,
            "token" => Self::Token,
            "semantic" => Self::Semantic,
            _ => Self::Recursive,
        }
    }
}

/// 上传响应
#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResponse {
    pub success: bool,
    pub document_count: usize,    // 原始文档数
    pub chunk_count: usize,       // 分块后的总chunk数
    pub message: String,          // 提示消息
    pub document_ids: Vec<String>, // 向量库中的文档ID
    pub chunk_strategy: String,   // 使用的切分策略
}
