//! AI Agent 信息聚合采集模块
//!
//! 支持数据源：
//! - GitHub Trending
//! - Hacker News
//! - RSS博客订阅
//! - ArXiv论文

pub mod github;
pub mod hackernews;
pub mod rss;
pub mod arxiv;

pub use github::GitHubTool;
pub use hackernews::HackerNewsTool;
pub use rss::RSSTool;
pub use arxiv::ArXivTool;

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedItem {
    pub id: String,
    pub source: ContentSource,
    pub title: String,
    pub content: String,
    pub url: String,
    pub author: Option<String>,
    pub published_at: Option<i64>,
    pub metadata: serde_json::Value,
}