//! Hacker News 采集工具
//!
//! 调用 HN Algolia Search API，搜索 AI/Agent/LLM 相关的热门讨论
//! API: GET /api/v1/search?query={keyword}&tags=story

use crate::agents::{ContentSource, CollectedItem};
use crate::errors::AgentError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HNStory {
    pub objectID: String,
    pub title: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub points: i64,
    pub created_at_i: i64,
    pub num_comments: Option<i64>,
}

pub struct HackerNewsTool {
    client: Client,
    api_base: String,
}

impl HackerNewsTool {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            api_base: "https://hn.algolia.com/api/v1".to_string(),
        }
    }
    
    /// 搜索热门故事（按关键词）
    pub async fn fetch_top_stories(&self, keywords: &[&str]) -> Result<Vec<CollectedItem>, AgentError> {
        let mut items = Vec::new();
        for keyword in keywords {
            let stories = self.search_stories(keyword).await?;
            for story in stories {
                let url = story.url.clone().unwrap_or_else(|| {
                    format!("https://news.ycombinator.com/item?id={}", story.objectID)
                });
                let item = CollectedItem {
                    id: Uuid::new_v4().to_string(),
                    source: ContentSource::HackerNews,
                    title: story.title.clone(),
                    content: format!("Points: {}, Comments: {}", story.points, story.num_comments.unwrap_or(0)),
                    url,
                    author: story.author.clone(),
                    published_at: Some(story.created_at_i),
                    metadata: serde_json::json!({
                        "hn_id": story.objectID,
                        "points": story.points,
                        "comments": story.num_comments.unwrap_or(0),
                    }),
                };
                items.push(item);
            }
        }
        Ok(items)
    }
    
    async fn search_stories(&self, keyword: &str) -> Result<Vec<HNStory>, AgentError> {
        let url = format!("{}/search?query={}&tags=story&hitsPerPage=20", self.api_base, keyword);
        let response = self.client.get(&url).send().await
            .map_err(|e| AgentError::NetworkError(e.to_string()))?;
        
        if !response.status().is_success() {
            return Err(AgentError::ApiError(format!("HN API返回: {}", response.status())));
        }
        
        let body: HNSearchResponse = response.json().await
            .map_err(|e| AgentError::ParseError(e.to_string()))?;
        Ok(body.hits)
    }
}

#[derive(Debug, Deserialize)]
struct HNSearchResponse {
    hits: Vec<HNStory>,
}

impl Default for HackerNewsTool {
    fn default() -> Self { Self::new() }
}
