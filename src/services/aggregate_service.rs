//! 聚合内容采集服务

use crate::config::Config;
use crate::errors::AgentError;
use crate::models::{
    CollectRequest, CollectResponse, CollectRecord,
    AggregateSearchRequest, AggregateSearchResponse,
    AggregateStatsResponse, AggregateListResponse,
};
use crate::agents::{GitHubTool, HackerNewsTool, RSSTool, ArXivTool, CollectedItem};
use crate::stores::ContentStore;
use langchainrust::{
    language_models::OpenAIChat,
    schema::Message,
    OpenAIConfig,
    Runnable,
};
use std::sync::Arc;

pub struct AggregateService {
    content_store: Arc<ContentStore>,
    llm: Arc<OpenAIChat>,
}

impl AggregateService {
    pub async fn new(config: Config) -> Result<Self, AgentError> {
        let content_store = Arc::new(ContentStore::new(&config).await?);
        
        let llm_config = OpenAIConfig {
            api_key: config.openai.api_key.clone(),
            base_url: config.openai.base_url.clone(),
            model: config.openai.chat_model.clone(),
            streaming: false,
            temperature: Some(0.3),
            max_tokens: Some(400),
            ..Default::default()
        };
        let llm = Arc::new(OpenAIChat::new(llm_config));
        
        Ok(Self {
            content_store,
            llm,
        })
    }
    
    pub async fn collect(&self, request: CollectRequest) -> Result<CollectResponse, AgentError> {
        let sources = request.sources.unwrap_or_else(|| {
            vec!["github".to_string(), "hackernews".to_string(), "rss".to_string(), "arxiv".to_string()]
        });
        
        let mut records: Vec<CollectRecord> = Vec::new();
        let mut all_items: Vec<CollectedItem> = Vec::new();
        
        for source in &sources {
            let items = match source.as_str() {
                "github" => self.collect_github().await?,
                "hackernews" => self.collect_hackernews().await?,
                "rss" => self.collect_rss().await?,
                "arxiv" => self.collect_arxiv().await?,
                _ => Vec::new(),
            };
            
            records.push(CollectRecord {
                source: source.clone(),
                count: items.len(),
                status: "success".to_string(),
            });
            
            all_items.extend(items);
        }
        
        for item in &all_items {
            let summary = self.generate_item_summary(item).await.ok().unwrap_or_else(|| item.content.chars().take(200).collect());
            self.content_store.save_item_with_summary(item.clone(), &summary).await?;
        }
        
        Ok(CollectResponse {
            success: true,
            collected_count: all_items.len(),
            records,
        })
    }
    
    async fn generate_item_summary(&self, item: &CollectedItem) -> Result<String, AgentError> {
        let prompt = format!(
            "请用中文简要总结以下AI/Agent相关内容，突出核心要点（不超过200字）：\n\n标题：{}\n\n内容：{}",
            item.title,
            item.content.chars().take(800).collect::<String>()
        );
        
        let summary = self.llm.invoke(vec![Message::human(&prompt)], None).await
            .map_err(|e| AgentError::LLMError(e.to_string()))?
            .content;
        
        Ok(summary)
    }
    
    async fn collect_github(&self) -> Result<Vec<CollectedItem>, AgentError> {
        let tool = GitHubTool::new();
        let languages = ["rust", "python"];
        tool.fetch_trending(&languages).await
    }
    
    async fn collect_hackernews(&self) -> Result<Vec<CollectedItem>, AgentError> {
        let tool = HackerNewsTool::new();
        let keywords = ["ai", "agent", "llm", "gpt", "langchain"];
        tool.fetch_top_stories(&keywords).await
    }
    
    async fn collect_rss(&self) -> Result<Vec<CollectedItem>, AgentError> {
        let tool = RSSTool::new();
        let feeds = [
            "https://openai.com/blog/rss.xml",
            "https://www.anthropic.com/index/rss.xml",
        ];
        tool.fetch_all_feeds(&feeds).await
    }
    
    async fn collect_arxiv(&self) -> Result<Vec<CollectedItem>, AgentError> {
        let tool = ArXivTool::new();
        let categories = ["cs.AI", "cs.CL"];
        tool.fetch_papers(&categories, 10).await
    }
    
    pub async fn list(&self, source: Option<&str>, limit: usize, offset: usize) -> Result<AggregateListResponse, AgentError> {
        let (total, items) = self.content_store.list(source, limit, offset).await?;
        
        Ok(AggregateListResponse {
            total,
            items,
        })
    }
    
    pub async fn search(&self, request: AggregateSearchRequest) -> Result<AggregateSearchResponse, AgentError> {
        let top_k = request.top_k.unwrap_or(10);
        let results = self.content_store.search(&request.query, top_k).await?;
        
        Ok(AggregateSearchResponse {
            results,
        })
    }
    
    pub async fn stats(&self) -> Result<AggregateStatsResponse, AgentError> {
        let (total, by_source, last_collected) = self.content_store.stats().await?;
        
        Ok(AggregateStatsResponse {
            total_items: total,
            by_source,
            last_collected_at: last_collected,
            keywords_count: 0,
        })
    }
}