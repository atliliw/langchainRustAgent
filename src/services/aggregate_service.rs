//! Agent 数据采集服务
//!
//! 这个服务自动从多个渠道采集 AI/Agent 相关的信息：
//! - GitHub Trending: 搜 Rust 和 Python 热门仓库
//! - HackerNews: 搜 AI/Agent/LLM 相关讨论
//! - RSS: OpenAI、Anthropic 博客
//! - ArXiv: 最新 AI 论文
//!
//! 采集流程：
//!   各渠道 → CollectedItem（统一格式）→ LLM 摘要 → SQLite 存储 → 前端展示

use crate::config::Config;
use crate::errors::AgentError;
use crate::models::{
    CollectRequest, CollectResponse, CollectRecord,
    AggregateSearchRequest, AggregateSearchResponse,
    AggregateStatsResponse, AggregateListResponse,
};
use crate::agents::{GitHubTool, HackerNewsTool, RSSTool, ArXivTool, CollectedItem};
use crate::stores::{ContentStore, ConversationStore};
use langchainrust::{
    language_models::OpenAIChat,
    schema::Message,
    OpenAIConfig,
    Runnable,
};
use std::sync::Arc;

/// 数据采集服务
/// 管理所有 Agent 工具，统一调度采集→摘要→存储
pub struct AggregateService {
    content_store: Arc<ContentStore>,  // 采集内容的存储
    llm: Arc<OpenAIChat>,               // 用于生成摘要的 LLM
    stats_store: Option<ConversationStore>, // 统计记录（可选）
}

impl AggregateService {
    /// 初始化：连接 SQLite + 初始化摘要用的 LLM
    pub async fn new(config: Config) -> Result<Self, AgentError> {
        Self::new_with_stats(config, None).await
    }

    /// 初始化（带统计记录）
    pub async fn new_with_stats(config: Config, stats_store: Option<ConversationStore>) -> Result<Self, AgentError> {
        let content_store = Arc::new(ContentStore::new(&config).await?);
        
        // 摘要用的 LLM 配置（温度低、token少、不流式）
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
            stats_store,
        })
    }
    
    /// 采集数据：从指定渠道（或所有渠道）抓取最新信息
    pub async fn collect(&self, request: CollectRequest) -> Result<CollectResponse, AgentError> {
        // 默认采集所有 4 个渠道
        let sources = request.sources.unwrap_or_else(|| {
            vec!["github".to_string(), "hackernews".to_string(), "rss".to_string(), "arxiv".to_string()]
        });
        
        let mut records: Vec<CollectRecord> = Vec::new();
        let mut all_items: Vec<CollectedItem> = Vec::new();
        
        // 遍历每个渠道，分别采集
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
        
        // 每条采集到的内容都调用 LLM 生成摘要，然后存储
        for item in &all_items {
            let summary = self.generate_item_summary(item).await
                .ok()
                .unwrap_or_else(|| item.content.chars().take(200).collect());
            self.content_store.save_item_with_summary(item.clone(), &summary).await?;
        }
        
        Ok(CollectResponse {
            success: true,
            collected_count: all_items.len(),
            records,
        })
    }
    
    /// 调用 LLM 生成内容摘要（限制200字内）
    async fn generate_item_summary(&self, item: &CollectedItem) -> Result<String, AgentError> {
        let prompt = format!(
            "请用中文简要总结以下AI/Agent相关内容，突出核心要点（不超过200字）：\n\n标题：{}\n\n内容：{}",
            item.title,
            item.content.chars().take(800).collect::<String>()
        );
        
        let start = std::time::Instant::now();
        let summary = self.llm.invoke(vec![Message::human(&prompt)], None).await
            .map_err(|e| AgentError::LLMError(e.to_string()))?;
        let duration = start.elapsed().as_millis() as i64;
        let tokens = crate::stores::estimate_tokens(&summary.content) as i64;
        
        if let Some(ref store) = self.stats_store {
            store.record_api_call("agent_summary", tokens, duration, true).await.ok();
        }
        
        Ok(summary.content)
    }
    
    /// ──────────────────── 各渠道采集方法 ────────────────────
    
    async fn collect_github(&self) -> Result<Vec<CollectedItem>, AgentError> {
        let tool = GitHubTool::new();
        let languages = ["rust", "python"];  // 搜 Rust 和 Python 语言的热门仓库
        tool.fetch_trending(&languages).await
    }
    
    async fn collect_hackernews(&self) -> Result<Vec<CollectedItem>, AgentError> {
        let tool = HackerNewsTool::new();
        let keywords = ["ai", "agent", "llm", "gpt", "langchain"];  // 搜这些关键词
        tool.fetch_top_stories(&keywords).await
    }
    
    async fn collect_rss(&self) -> Result<Vec<CollectedItem>, AgentError> {
        let tool = RSSTool::new();
        let feeds = [
            "https://openai.com/blog/rss.xml",        // OpenAI 博客
            "https://www.anthropic.com/index/rss.xml", // Anthropic 博客
        ];
        tool.fetch_all_feeds(&feeds).await
    }
    
    async fn collect_arxiv(&self) -> Result<Vec<CollectedItem>, AgentError> {
        let tool = ArXivTool::new();
        let categories = ["cs.AI", "cs.CL"];  // AI 和计算语言学方向
        tool.fetch_papers(&categories, 10).await  // 每类取10篇
    }
    
    /// ──────────────────── 查询 ────────────────────
    
    /// 列出已采集的内容，支持按来源过滤和分页
    pub async fn list(&self, source: Option<&str>, limit: usize, offset: usize) -> Result<AggregateListResponse, AgentError> {
        let (total, items) = self.content_store.list(source, limit, offset).await?;
        Ok(AggregateListResponse { total, items })
    }
    
    /// 搜索已采集的内容
    pub async fn search(&self, request: AggregateSearchRequest) -> Result<AggregateSearchResponse, AgentError> {
        let top_k = request.top_k.unwrap_or(10);
        let results = self.content_store.search(&request.query, top_k).await?;
        Ok(AggregateSearchResponse { results })
    }
    
    /// 获取采集统计（总量、按来源分布、最后采集时间）
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
