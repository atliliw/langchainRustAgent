//! ArXiv 论文采集工具

use crate::agents::{ContentSource, CollectedItem};
use crate::errors::AgentError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::DateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArXivPaper {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub authors: Vec<String>,
    pub published: String,
    pub link: String,
}

pub struct ArXivTool {
    client: Client,
    api_base: String,
}

impl ArXivTool {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            api_base: "http://export.arxiv.org/api/query".to_string(),
        }
    }
    
    pub async fn fetch_papers(&self, categories: &[&str], max_results: usize) -> Result<Vec<CollectedItem>, AgentError> {
        let category_query = categories.join(" OR ");
        let url = format!(
            "{}{}",
            self.api_base,
            format!(
                "?search_query=cat:{}&start=0&max_results={}",
                category_query, max_results
            )
        );
        
        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| AgentError::NetworkError(e.to_string()))?;
        
        if !response.status().is_success() {
            return Err(AgentError::ApiError(format!("ArXiv API返回: {}", response.status())));
        }
        
        let body = response
            .text()
            .await
            .map_err(|e| AgentError::ParseError(e.to_string()))?;
        
        self.parse_arxiv(&body)
    }
    
    fn parse_arxiv(&self, xml: &str) -> Result<Vec<CollectedItem>, AgentError> {
        let mut items = Vec::new();
        
        let entry_starts: Vec<usize> = xml.match_indices("<entry>")
            .map(|(i, _)| i)
            .collect();
        
        for start in entry_starts {
            if let Some(end) = xml[start..].find("</entry>") {
                let entry_xml = &xml[start..start + end + 8];
                
                let id = self.extract_tag(entry_xml, "id");
                let title = self.extract_tag(entry_xml, "title");
                let summary = self.extract_tag(entry_xml, "summary");
                let published = self.extract_tag(entry_xml, "published");
                
                let authors: Vec<String> = entry_xml
                    .match_indices("<author>")
                    .filter_map(|(a_start, _)| {
                        if let Some(a_end) = entry_xml[a_start..].find("</author>") {
                            let author_xml = &entry_xml[a_start..a_start + a_end + 8];
                            self.extract_tag(author_xml, "name")
                        } else {
                            None
                        }
                    })
                    .collect();
                
                let published_at = published.as_ref()
                    .and_then(|p| DateTime::parse_from_rfc3339(p).ok())
                    .map(|dt| dt.timestamp());
                
                let link = id.clone().unwrap_or_default();
                let arxiv_id = link
                    .replace("http://arxiv.org/abs/", "")
                    .replace("http://arxiv.org/pdf/", "")
                    .replace(".pdf", "");
                
                if let (Some(title), Some(summary)) = (title, summary) {
                    let item = CollectedItem {
                        id: Uuid::new_v4().to_string(),
                        source: ContentSource::ArXiv,
                        title: title.trim().to_string(),
                        content: summary.trim().to_string(),
                        url: format!("https://arxiv.org/abs/{}", arxiv_id),
                        author: authors.first().cloned(),
                        published_at,
                        metadata: serde_json::json!({
                            "arxiv_id": arxiv_id,
                            "authors": authors,
                            "published": published,
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
                return Some(xml[content_start..content_start + end].trim().to_string());
            }
        }
        None
    }
}

impl Default for ArXivTool {
    fn default() -> Self {
        Self::new()
    }
}