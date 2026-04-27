//! 对话历史存储模块（SQLite + 分层压缩策略）

use crate::config::{Config, ConversationConfig};
use crate::errors::ConversationError;
use crate::models::{
    ChatRequest, ChatResponse, SessionInfo, SourceInfo, 
    ConversationMessage, Session, CompressMode, SearchMode, CompressModeInfo,
};
use langchainrust::{
    language_models::OpenAIChat,
    schema::Message,
    core::runnables::Runnable,
    OpenAIConfig,
};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use std::sync::Arc;
use chrono::Utc;
use futures_util::Stream;

pub struct ConversationStore {
    pool: SqlitePool,
    llm: Arc<OpenAIChat>,
    summary_llm: Arc<OpenAIChat>,
    compress_config: ConversationConfig,
}

impl ConversationStore {
    pub async fn new(config: &Config) -> Result<Self, ConversationError> {
        let db_path = &config.sqlite.db_path;
        
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}?mode=rwc", db_path))
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        sqlx::query(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA cache_size = -64000;"
        )
        .execute(&pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Self::create_tables(&pool).await?;
        
        let llm = Arc::new(OpenAIChat::new(config.to_langchain_openai_config().with_streaming(true)));
        
        let summary_config = OpenAIConfig {
            api_key: config.openai.api_key.clone(),
            base_url: config.openai.base_url.clone(),
            model: config.conversation.as_ref()
                .map(|c| c.summary_model.clone())
                .unwrap_or_else(|| "gpt-3.5-turbo".to_string()),
            streaming: false,
            temperature: Some(0.3),
            max_tokens: Some(200),
            ..Default::default()
        };
        let summary_llm = Arc::new(OpenAIChat::new(summary_config));
        
        let compress_config = config.conversation.clone().unwrap_or_default();
        
        Ok(Self {
            pool,
            llm,
            summary_llm,
            compress_config,
        })
    }
    
    async fn create_tables(pool: &SqlitePool) -> Result<(), ConversationError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS session (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                message_count INTEGER DEFAULT 0,
                tokens_used INTEGER DEFAULT 0,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL
            )"
        )
        .execute(pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tokens INTEGER DEFAULT 0,
                time_created INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES session(id)
            )"
        )
        .execute(pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_message_session ON message(session_id)")
            .execute(pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_message_time ON message(time_created)")
            .execute(pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_session_updated ON session(time_updated)")
            .execute(pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    pub async fn chat(
        &self,
        request: ChatRequest,
        rag_sources: Vec<SourceInfo>,
    ) -> Result<ChatResponse, ConversationError> {
        let session_id = request.session_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        
        if !self.session_exists(&session_id).await? {
            self.create_session(&session_id, "新对话").await?;
        }
        
        let history = self.get_history(&session_id).await?;
        
        let compress_mode = CompressMode::from_str(&request.compress_mode);
        let search_mode = SearchMode::from_flags(request.use_vector, request.use_bm25);
        
        let (messages, compressed, compression_info) = self.build_messages(
            history,
            request.message.clone(),
            search_mode,
            rag_sources.clone(),
            compress_mode,
        ).await?;
        
        let reply = self.llm.invoke(messages, None).await
            .map_err(|e| ConversationError::LLMError(e.to_string()))?
            .content;
        
        self.save_message(&session_id, "user", &request.message).await?;
        self.save_message(&session_id, "assistant", &reply).await?;
        
        self.update_session_stats(&session_id).await?;
        
        Ok(ChatResponse {
            session_id,
            reply,
            sources: rag_sources,
            compressed,
            compression_info,
        })
    }
    
    pub async fn chat_stream(
        &self,
        request: ChatRequest,
        rag_sources: Vec<SourceInfo>,
    ) -> Result<(String, impl Stream<Item = Result<String, ConversationError>>), ConversationError> {
        use futures_util::StreamExt;
        
        let session_id = request.session_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        
        if !self.session_exists(&session_id).await? {
            self.create_session(&session_id, "新对话").await?;
        }
        
        let history = self.get_history(&session_id).await?;
        
        let compress_mode = CompressMode::from_str(&request.compress_mode);
        let search_mode = SearchMode::from_flags(request.use_vector, request.use_bm25);
        
        let (messages, _, _) = self.build_messages(
            history,
            request.message.clone(),
            search_mode,
            rag_sources.clone(),
            compress_mode,
        ).await?;
        
        let stream = self.llm.stream(messages, None).await
            .map_err(|e| ConversationError::LLMError(e.to_string()))?;
        
        let token_stream = stream.map(|r| {
            r.map(|llm_result| llm_result.content)
                .map_err(|e| ConversationError::LLMError(e.to_string()))
        });
        
        Ok((session_id, token_stream))
    }
    
    async fn build_messages(
        &self,
        history: Vec<ConversationMessage>,
        current_message: String,
        search_mode: SearchMode,
        rag_sources: Vec<SourceInfo>,
        compress_mode: CompressMode,
    ) -> Result<(Vec<Message>, bool, Option<String>), ConversationError> {
        let mut messages: Vec<Message> = Vec::new();
        
        messages.push(Message::system("请用中文回答用户问题。记住用户之前告诉你的信息。"));
        
        let (compressed_history, was_compressed, compression_info) = 
            self.apply_compression(history, compress_mode).await?;
        
        for msg in &compressed_history {
            if msg.role == "user" {
                messages.push(Message::human(&msg.content));
            } else if msg.role == "assistant" {
                messages.push(Message::ai(&msg.content));
            } else if msg.role == "summary" {
                messages.push(Message::system(&format!("[历史摘要] {}", msg.content)));
            }
        }
        
        if search_mode != SearchMode::None && !rag_sources.is_empty() {
            let context = rag_sources.iter().map(|s| s.content.clone()).collect::<Vec<_>>().join("\n\n---\n\n");
            messages.push(Message::human(&format!("参考信息:\n{}\n\n{}", context, current_message)));
        } else {
            messages.push(Message::human(&current_message));
        }
        
        Ok((messages, was_compressed, compression_info))
    }
    
    async fn apply_compression(
        &self,
        history: Vec<ConversationMessage>,
        mode: CompressMode,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        if mode == CompressMode::None || history.len() <= 2 {
            return Ok((history, false, None));
        }
        
        match mode {
            CompressMode::SlidingWindow => self.apply_sliding_window(history),
            CompressMode::TokenLimit => self.apply_token_limit(history),
            CompressMode::Summary => self.apply_summary_compression(history).await,
            CompressMode::Layered => self.apply_layered_compression(history).await,
            CompressMode::None => Ok((history, false, None)),
        }
    }
    
    fn apply_sliding_window(
        &self,
        history: Vec<ConversationMessage>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let max_messages = self.compress_config.max_history_messages;
        
        if history.len() <= max_messages {
            return Ok((history, false, None));
        }
        
        let truncated = history.into_iter().rev().take(max_messages).rev().collect();
        
        let info = format!("滑动窗口: 保留最近 {} 条", max_messages);
        Ok((truncated, true, Some(info)))
    }
    
    fn apply_token_limit(
        &self,
        history: Vec<ConversationMessage>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let max_tokens = self.compress_config.max_tokens;
        let keep_first_n = self.compress_config.keep_first_n_messages;
        let original_len = history.len();
        
        let first_n = history.iter().take(keep_first_n).cloned().collect::<Vec<_>>();
        let remaining = history.into_iter().skip(keep_first_n).collect::<Vec<_>>();
        
        let mut recent: Vec<ConversationMessage> = Vec::new();
        let mut total_tokens = 0;
        
        for msg in remaining.into_iter().rev() {
            let msg_tokens = Self::estimate_tokens(&msg.content);
            if total_tokens + msg_tokens <= max_tokens {
                recent.insert(0, msg);
                total_tokens += msg_tokens;
            } else {
                break;
            }
        }
        
        let result: Vec<ConversationMessage> = first_n.into_iter().chain(recent).collect();
        
        if result.len() < original_len {
            let info = format!("Token限制: 保留前 {} 条 + 最近 {} 条 ({} tokens)", keep_first_n, result.len() - keep_first_n, total_tokens);
            Ok((result, true, Some(info)))
        } else {
            Ok((result, false, None))
        }
    }
    
    async fn apply_summary_compression(
        &self,
        history: Vec<ConversationMessage>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let threshold = self.compress_config.compress_threshold;
        let keep_recent = self.compress_config.keep_recent_messages;
        
        if history.len() <= threshold {
            return Ok((history, false, None));
        }
        
        let to_compress_count = history.len() - keep_recent;
        let to_compress = history.iter().take(to_compress_count).cloned().collect::<Vec<_>>();
        let recent = history.into_iter().skip(to_compress_count).collect::<Vec<_>>();
        
        let summary = self.generate_summary(&to_compress).await?;
        let summary_tokens = Self::estimate_tokens(&summary);
        
        let summary_msg = ConversationMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: "".to_string(),
            role: "summary".to_string(),
            content: summary,
            tokens: summary_tokens as i64,
            time_created: Utc::now().timestamp_millis(),
        };
        
        let result = vec![summary_msg].into_iter().chain(recent).collect();
        
        let info = format!("摘要压缩: {} 条压缩为摘要，保留最近 {} 条", to_compress_count, keep_recent);
        Ok((result, true, Some(info)))
    }
    
    async fn apply_layered_compression(
        &self,
        history: Vec<ConversationMessage>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let max_messages = self.compress_config.max_history_messages;
        let threshold = self.compress_config.compress_threshold;
        let keep_recent = self.compress_config.keep_recent_messages;
        let keywords = &self.compress_config.important_keywords;
        
        if history.len() <= threshold {
            return Ok((history, false, None));
        }
        
        let important: Vec<ConversationMessage> = history.iter()
            .filter(|msg| keywords.iter().any(|k| msg.content.contains(k)))
            .cloned()
            .collect();
        
        let recent: Vec<ConversationMessage> = history.iter()
            .rev().take(keep_recent).rev().cloned().collect();
        
        let important_ids: Vec<String> = important.iter().map(|m| m.id.clone()).collect();
        let recent_ids: Vec<String> = recent.iter().map(|m| m.id.clone()).collect();
        
        let to_compress: Vec<ConversationMessage> = history.iter()
            .filter(|msg| !important_ids.contains(&msg.id) && !recent_ids.contains(&msg.id))
            .cloned()
            .collect();
        
        let summary = if to_compress.len() > 3 {
            self.generate_summary(&to_compress).await?
        } else {
            to_compress.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n")
        };
        let summary_tokens = Self::estimate_tokens(&summary) as i64;
        
        let summary_msg = ConversationMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: "".to_string(),
            role: "summary".to_string(),
            content: summary,
            tokens: summary_tokens,
            time_created: Utc::now().timestamp_millis(),
        };
        
        let result: Vec<ConversationMessage> = important.into_iter()
            .chain(std::iter::once(summary_msg))
            .chain(recent)
            .collect();
        
        let result = if result.len() > max_messages {
            result.into_iter().rev().take(max_messages).rev().collect()
        } else {
            result
        };
        
        let info = format!("分层压缩: 重要 {} 条 + 摘要 + 最近 {} 条", important_ids.len(), keep_recent);
        Ok((result, true, Some(info)))
    }
    
    async fn generate_summary(
        &self,
        messages: &[ConversationMessage],
    ) -> Result<String, ConversationError> {
        let history_text = messages.iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>().join("\n");
        
        let prompt = format!("请将以下对话压缩成摘要（100字内），保留关键设定：\n\n{}", history_text);
        
        let summary = self.summary_llm.invoke(vec![Message::human(&prompt)], None).await
            .map_err(|e| ConversationError::LLMError(e.to_string()))?
            .content;
        
        Ok(summary)
    }
    
    fn estimate_tokens(text: &str) -> usize {
        let char_count = text.chars().count();
        let chinese_chars = text.chars().filter(|c| *c > '\u{4E00}' && *c < '\u{9FFF}').count();
        (chinese_chars * 2 + (char_count - chinese_chars)) / 4 + 1
    }
    
    async fn session_exists(&self, session_id: &str) -> Result<bool, ConversationError> {
        let row = sqlx::query("SELECT COUNT(*) FROM session WHERE id = ?")
            .bind(session_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(row.get::<i64, _>(0) > 0)
    }
    
    async fn create_session(&self, session_id: &str, title: &str) -> Result<(), ConversationError> {
        let now = Utc::now().timestamp_millis();
        
        sqlx::query("INSERT INTO session (id, title, time_created, time_updated) VALUES (?, ?, ?, ?)")
            .bind(session_id).bind(title).bind(now).bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    async fn save_message(&self, session_id: &str, role: &str, content: &str) -> Result<(), ConversationError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let tokens = Self::estimate_tokens(content) as i64;
        
        sqlx::query("INSERT INTO message (id, session_id, role, content, tokens, time_created) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&id).bind(session_id).bind(role).bind(content).bind(tokens).bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    async fn update_session_stats(&self, session_id: &str) -> Result<(), ConversationError> {
        let now = Utc::now().timestamp_millis();
        
        sqlx::query(
            "UPDATE session SET 
                time_updated = ?,
                message_count = (SELECT COUNT(*) FROM message WHERE session_id = ?),
                tokens_used = (SELECT SUM(tokens) FROM message WHERE session_id = ?)
             WHERE id = ?"
        )
        .bind(now).bind(session_id).bind(session_id).bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    pub async fn get_history(&self, session_id: &str) -> Result<Vec<ConversationMessage>, ConversationError> {
        let rows = sqlx::query(
            "SELECT id, session_id, role, content, tokens, time_created 
             FROM message WHERE session_id = ? ORDER BY time_created"
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        let messages = rows.into_iter().map(|row| ConversationMessage {
            id: row.get::<String, _>(0),
            session_id: row.get::<String, _>(1),
            role: row.get::<String, _>(2),
            content: row.get::<String, _>(3),
            tokens: row.get::<i64, _>(4),
            time_created: row.get::<i64, _>(5),
        }).collect();
        
        Ok(messages)
    }
    
    pub async fn get_sessions(&self) -> Result<Vec<SessionInfo>, ConversationError> {
        let rows = sqlx::query(
            "SELECT id, title, time_created, message_count 
             FROM session ORDER BY time_updated DESC LIMIT 20"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        let sessions = rows.into_iter().map(|row| {
            let time_created = row.get::<i64, _>(2);
            let created_at = chrono::DateTime::from_timestamp_millis(time_created)
                .map(|dt| dt.to_rfc3339()).unwrap_or_default();
            
            SessionInfo {
                session_id: row.get::<String, _>(0),
                title: row.get::<String, _>(1),
                created_at,
                message_count: row.get::<i64, _>(3) as usize,
                preview: String::new(),
            }
        }).collect();
        
        Ok(sessions)
    }
    
    pub async fn clear_session(&self, session_id: &str) -> Result<(), ConversationError> {
        sqlx::query("DELETE FROM message WHERE session_id = ?")
            .bind(session_id).execute(&self.pool).await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        sqlx::query("DELETE FROM session WHERE id = ?")
            .bind(session_id).execute(&self.pool).await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    pub async fn clear_all(&self) -> Result<(), ConversationError> {
        sqlx::query("DELETE FROM message").execute(&self.pool).await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        sqlx::query("DELETE FROM session").execute(&self.pool).await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    pub async fn save_full_message(&self, session_id: &str, user_msg: &str, assistant_msg: &str) -> Result<(), ConversationError> {
        self.save_message(session_id, "user", user_msg).await?;
        self.save_message(session_id, "assistant", assistant_msg).await?;
        self.update_session_stats(session_id).await?;
        Ok(())
    }
    
    pub fn get_compress_modes() -> Vec<CompressModeInfo> {
        vec![
            CompressModeInfo { name: "none".to_string(), label: "不压缩".to_string(), description: "保留完整历史（可能超出token限制）".to_string() },
            CompressModeInfo { name: "sliding_window".to_string(), label: "滑动窗口".to_string(), description: "只保留最近N条消息".to_string() },
            CompressModeInfo { name: "token_limit".to_string(), label: "Token限制".to_string(), description: "控制总token数量".to_string() },
            CompressModeInfo { name: "summary".to_string(), label: "摘要压缩".to_string(), description: "旧消息压缩为摘要".to_string() },
            CompressModeInfo { name: "layered".to_string(), label: "分层压缩".to_string(), description: "保护重要+摘要+最近".to_string() },
        ]
    }
}