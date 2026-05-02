//! GitHub Trending 采集工具
//!
//! 调用 GitHub Search API，搜索按语言分类的热门仓库
//! API: GET /search/repositories?q=language:{lang}&sort=stars

use crate::agents::{ContentSource, CollectedItem};
use crate::errors::AgentError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRepo {
    pub name: String,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub stargazers_count: i64,
    pub language: Option<String>,
    pub owner: Option<GitHubOwner>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubOwner {
    pub login: String,
    pub html_url: String,
}

pub struct GitHubTool {
    client: Client,
    api_base: String,
}

impl GitHubTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("langchainrust-agent")
                .build()
                .unwrap(),
            api_base: "https://api.github.com".to_string(),
        }
    }
    
    /// 获取热门仓库（按语言筛选）
    pub async fn fetch_trending(&self, languages: &[&str]) -> Result<Vec<CollectedItem>, AgentError> {
        let mut items = Vec::new();
        for lang in languages {
            let repos = self.search_repositories(lang).await?;
            for repo in repos {
                let item = CollectedItem {
                    id: Uuid::new_v4().to_string(),
                    source: ContentSource::GitHub,
                    title: repo.full_name.clone(),
                    content: repo.description.clone().unwrap_or_default(),
                    url: repo.html_url.clone(),
                    author: repo.owner.map(|o| o.login),
                    published_at: None,
                    metadata: serde_json::json!({
                        "stars": repo.stargazers_count,
                        "language": repo.language,
                    }),
                };
                items.push(item);
            }
        }
        Ok(items)
    }
    
    /// 搜索仓库（按语言、Star数降序）
    async fn search_repositories(&self, language: &str) -> Result<Vec<GitHubRepo>, AgentError> {
        let url = format!("{}/search/repositories?q=language:{}&sort=stars&order=desc&per_page=20",
            self.api_base, language);
        
        let response = self.client.get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send().await
            .map_err(|e| AgentError::NetworkError(e.to_string()))?;
        
        if !response.status().is_success() {
            return Err(AgentError::ApiError(format!("GitHub API返回: {}", response.status())));
        }
        
        let body: GitHubSearchResponse = response.json().await
            .map_err(|e| AgentError::ParseError(e.to_string()))?;
        
        Ok(body.items)
    }
    
    /// 获取单个仓库详情
    pub async fn fetch_repo_details(&self, owner: &str, repo: &str) -> Result<GitHubRepo, AgentError> {
        let url = format!("{}/repos/{}/{}", self.api_base, owner, repo);
        let response = self.client.get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send().await
            .map_err(|e| AgentError::NetworkError(e.to_string()))?;
        
        if !response.status().is_success() {
            return Err(AgentError::ApiError(format!("GitHub API返回: {}", response.status())));
        }
        
        let repo: GitHubRepo = response.json().await
            .map_err(|e| AgentError::ParseError(e.to_string()))?;
        Ok(repo)
    }
}

#[derive(Debug, Deserialize)]
struct GitHubSearchResponse {
    items: Vec<GitHubRepo>,
}

impl Default for GitHubTool {
    fn default() -> Self { Self::new() }
}
