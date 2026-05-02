//! Agent 采集内容存储模块
//!
//! 存储从 GitHub/HackerNews/RSS/ArXiv 采集的信息。
//! 使用 SQLite 持久化，支持按来源查询、关键词搜索、统计分析

use crate::config::Config;
use crate::errors::AgentError;
use crate::models::{AggregatedContent, AggregateSearchResult};
use crate::agents::CollectedItem;
use langchainrust::OpenAIEmbeddings;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use std::sync::Arc;
use std::collections::HashMap;
use chrono::Utc;

pub struct ContentStore {
    pool: SqlitePool,
    embeddings: Arc<OpenAIEmbeddings>,
}

impl ContentStore {
    /// 初始化：连接 SQLite + 建表
    pub async fn new(config: &Config) -> Result<Self, AgentError> {
        let db_path = "aggregate_content.db";
        
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}?mode=rwc", db_path))
            .await?;
        
        Self::create_tables(&pool).await?;
        
        let embeddings_config = config.to_langchain_embeddings_config();
        let embeddings = Arc::new(OpenAIEmbeddings::new(embeddings_config));
        
        Ok(Self { pool, embeddings })
    }
    
    async fn create_tables(pool: &SqlitePool) -> Result<(), AgentError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS aggregate_content (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,       -- github/hackernews/rss/arxiv
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                url TEXT NOT NULL,
                author TEXT,
                published_at INTEGER,
                collected_at INTEGER NOT NULL,
                summary TEXT,
                keywords TEXT,
                metadata TEXT               -- JSON 字符串
            )"
        ).execute(pool).await?;
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_content_source ON aggregate_content(source)").execute(pool).await.ok();
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_content_collected ON aggregate_content(collected_at)").execute(pool).await.ok();
        
        Ok(())
    }
    
    /// 保存单条采集内容（带 LLM 生成的摘要）
    pub async fn save_item_with_summary(&self, item: CollectedItem, summary: &str) -> Result<(), AgentError> {
        let keywords_json = serde_json::to_string(&Vec::<String>::new()).unwrap();
        let metadata_json = serde_json::to_string(&item.metadata).unwrap();
        
        sqlx::query(
            "INSERT OR IGNORE INTO aggregate_content 
            (id, source, title, content, url, author, published_at, collected_at, summary, keywords, metadata)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&item.id)
        .bind(item.source.as_str())
        .bind(&item.title)
        .bind(&item.content)
        .bind(&item.url)
        .bind(&item.author)
        .bind(item.published_at)
        .bind(Utc::now().timestamp_millis())
        .bind(summary)
        .bind(&keywords_json)
        .bind(&metadata_json)
        .execute(&self.pool).await?;
        
        Ok(())
    }
    
    /// 列表查询（支持按来源过滤和分页）
    pub async fn list(&self, source: Option<&str>, limit: usize, offset: usize) -> Result<(usize, Vec<AggregatedContent>), AgentError> {
        // 先查总数
        let total: usize = if let Some(s) = source {
            sqlx::query("SELECT COUNT(*) FROM aggregate_content WHERE source = ?").bind(s)
        } else {
            sqlx::query("SELECT COUNT(*) FROM aggregate_content")
        }.fetch_one(&self.pool).await?.get::<i64, _>(0) as usize;
        
        // 查数据
        let rows = if let Some(s) = source {
            sqlx::query(
                "SELECT id, source, title, content, url, author, published_at, collected_at, summary, keywords, metadata
                FROM aggregate_content WHERE source = ? ORDER BY collected_at DESC LIMIT ? OFFSET ?"
            ).bind(s).bind(limit as i64).bind(offset as i64)
        } else {
            sqlx::query(
                "SELECT id, source, title, content, url, author, published_at, collected_at, summary, keywords, metadata
                FROM aggregate_content ORDER BY collected_at DESC LIMIT ? OFFSET ?"
            ).bind(limit as i64).bind(offset as i64)
        }.fetch_all(&self.pool).await?;
        
        let items: Vec<AggregatedContent> = rows.into_iter().map(|row| {
            AggregatedContent {
                id: row.get::<String, _>(0),
                source: row.get::<String, _>(1),
                title: row.get::<String, _>(2),
                content: row.get::<String, _>(3),
                url: row.get::<String, _>(4),
                author: row.get::<Option<String>, _>(5),
                published_at: row.get::<Option<i64>, _>(6),
                collected_at: row.get::<i64, _>(7),
                summary: row.get::<Option<String>, _>(8),
                keywords: serde_json::from_str(&row.get::<String, _>(9)).unwrap_or_default(),
                metadata: serde_json::from_str(&row.get::<String, _>(10)).unwrap_or_default(),
            }
        }).collect();
        
        Ok((total, items))
    }
    
    /// 简单关键词搜索（基于词匹配分数）
    pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<AggregateSearchResult>, AgentError> {
        let all_items = self.list(None, 100, 0).await?.1;
        
        // 计算每条内容与查询词的匹配度（简单版：单词重叠比例）
        let mut scored: Vec<(f32, AggregatedContent)> = all_items.into_iter()
            .map(|item| {
                let text = format!("{} {}", item.title, item.content);
                let score = Self::simple_similarity(query, &text);
                (score, item)
            })
            .collect();
        
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        
        let results: Vec<AggregateSearchResult> = scored.into_iter()
            .take(top_k)
            .map(|(score, item)| AggregateSearchResult {
                id: item.id,
                source: item.source,
                title: item.title,
                content: item.content,
                url: item.url,
                score,
                summary: item.summary,
            })
            .collect();
        
        Ok(results)
    }
    
    // 简单的词重叠匹配（不以 Embedding，节省成本）
    fn simple_similarity(query: &str, text: &str) -> f32 {
        let query_words: Vec<&str> = query.split_whitespace().collect();
        let text_words: Vec<&str> = text.split_whitespace().collect();
        
        let matches = query_words.iter()
            .filter(|w| text_words.iter().any(|t| t.contains(*w)))
            .count();
        
        if query_words.is_empty() { 0.0 } else { matches as f32 / query_words.len() as f32 }
    }
    
    /// 统计（总量、按来源分布、最后采集时间）
    pub async fn stats(&self) -> Result<(usize, HashMap<String, usize>, Option<i64>), AgentError> {
        let total: usize = sqlx::query("SELECT COUNT(*) FROM aggregate_content")
            .fetch_one(&self.pool).await?
            .get::<i64, _>(0) as usize;
        
        let rows = sqlx::query("SELECT source, COUNT(*) FROM aggregate_content GROUP BY source")
            .fetch_all(&self.pool).await?;
        
        let by_source: HashMap<String, usize> = rows.into_iter()
            .map(|row| (row.get::<String, _>(0), row.get::<i64, _>(1) as usize))
            .collect();
        
        let last: Option<i64> = sqlx::query("SELECT MAX(collected_at) FROM aggregate_content")
            .fetch_one(&self.pool).await?
            .get::<Option<i64>, _>(0);
        
        Ok((total, by_source, last))
    }
}
