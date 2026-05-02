//! Agent 数据采集工具
//!
//! 从多个外部数据源采集 AI/Agent 相关的信息，统一成 CollectedItem 格式。
//!
//! GitHub         → GitHub Trending 仓库（按语言搜索）
//! HackerNews     → HN Algolia 搜索（AI/Agent 相关讨论）
//! RSS            → OpenAI/Anthropic 博客更新
//! ArXiv          → 最新 AI 论文

pub mod github;       // GitHub Trending 采集
pub mod hackernews;   // HackerNews 采集
pub mod rss;          // RSS 订阅采集
pub mod arxiv;        // ArXiv 论文采集

pub use github::GitHubTool;
pub use hackernews::HackerNewsTool;
pub use rss::RSSTool;
pub use arxiv::ArXivTool;

use serde::{Deserialize, Serialize};

/// 数据来源类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContentSource {
    GitHub,
    HackerNews,
    RSS,
    ArXiv,
}

impl ContentSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentSource::GitHub => "github",
            ContentSource::HackerNews => "hackernews",
            ContentSource::RSS => "rss",
            ContentSource::ArXiv => "arxiv",
        }
    }
    
    pub fn from_str(s: &str) -> Self {
        match s {
            "github" => ContentSource::GitHub,
            "hackernews" => ContentSource::HackerNews,
            "rss" => ContentSource::RSS,
            "arxiv" => ContentSource::ArXiv,
            _ => ContentSource::RSS,
        }
    }
}

/// 统一采集结果格式
/// 不管从哪个渠道采集，最终都转成这个结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedItem {
    pub id: String,                        // 唯一ID
    pub source: ContentSource,              // 来源
    pub title: String,                      // 标题
    pub content: String,                    // 内容
    pub url: String,                        // 原始链接
    pub author: Option<String>,             // 作者
    pub published_at: Option<i64>,          // 发布时间
    pub metadata: serde_json::Value,        // 附加元数据
}
