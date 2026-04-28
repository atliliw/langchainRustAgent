//! RSS 博客订阅采集工具

use crate::agents::{ContentSource, CollectedItem};
use crate::errors::AgentError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::DateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RSSItem {
    pub title: String,
    pub link: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub pub_date: Option<String>,
}

pub struct RSSTool {
    client: Client,
}

impl RSSTool {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
    
    pub async fn fetch_feed(&self, feed_url: &str) -> Result<Vec<CollectedItem>, AgentError> {
        let response = self.client
            .get(feed_url)
            .send()
            .await
            .map_err(|e| AgentError::NetworkError(e.to_string()))?;
        
        if !response.status().is_success() {
            return Err(AgentError::ApiError(format!("RSS返回: {}", response.status())));
        }
        
        let body = response
            .text()
            .await
            .map_err(|e| AgentError::ParseError(e.to_string()))?;
        
        self.parse_rss(&body)
    }
    
    pub async fn fetch_all_feeds(&self, feeds: &[&str]) -> Result<Vec<CollectedItem>, AgentError> {
        let mut all_items = Vec::new();
        
        for feed_url in feeds {
            match self.fetch_feed(feed_url).await {
                Ok(items) => all_items.extend(items),
                Err(e) => tracing::warn!("RSS feed {} 失败: {}", feed_url, e),
            }
        }
        
        Ok(all_items)
    }
    
    fn parse_rss(&self, xml: &str) -> Result<Vec<CollectedItem>, AgentError> {
        let mut items = Vec::new();
        
        let item_starts: Vec<usize> = xml.match_indices("<item>")
            .map(|(i, _)| i)
            .collect();
        
        for start in item_starts {
            if let Some(end) = xml[start..].find("</item>") {
                let item_xml = &xml[start..start + end + 7];
                
                let title = self.extract_tag(item_xml, "title");
                let link = self.extract_tag(item_xml, "link");
                let description = self.extract_tag(item_xml, "description");
                let author = self.extract_tag(item_xml, "author")
                    .or_else(|| self.extract_tag(item_xml, "dc:creator"));
                let pub_date = self.extract_tag(item_xml, "pubDate");
                
                let published_at = pub_date.as_ref()
                    .and_then(|d| DateTime::parse_from_rfc2822(d).ok())
                    .map(|dt| dt.timestamp());
                
                if let (Some(title), Some(link)) = (title, link) {
                    let item = CollectedItem {
                        id: Uuid::new_v4().to_string(),
                        source: ContentSource::RSS,
                        title,
                        content: description.unwrap_or_default(),
                        url: link,
                        author,
                        published_at,
                        metadata: serde_json::json!({
                            "pub_date": pub_date,
                        }),
                    };
                    items.push(item);
                }
            }
        }
        
        Ok(items)
    }
    
    fn extract_tag(&self, xml: &str, tag: &str) -> Option<String> {
        let open = format!("<{}>", tag);
        let close = format!("</{}>", tag);
        
        if let Some(start) = xml.find(&open) {
            let content_start = start + open.len();
            if let Some(end) = xml[content_start..].find(&close) {
                let content = &xml[content_start..content_start + end];
                return Some(self.clean_content(content));
            }
        }
        None
    }
    
    fn clean_content(&self, content: &str) -> String {
        content
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .trim()
            .to_string()
    }
}

impl Default for RSSTool {
    fn default() -> Self {
        Self::new()
    }
}