//! 对话历史存储模块（SQLite + 分层压缩策略）

use crate::config::{Config, ConversationConfig};
use crate::errors::ConversationError;
use crate::models::{
    ChatRequest, ChatResponse, SessionInfo, SourceInfo, 
    ConversationMessage, CompressMode, SearchMode, CompressModeInfo,
};
use langchainrust::{
    language_models::OpenAIChat,
    schema::Message,
    core::runnables::Runnable,
};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use std::sync::Arc;
use chrono::Utc;
use futures_util::Stream;
use serde::{Serialize, Deserialize};

/// AFM 保真度等级
#[derive(Debug, Clone, Copy)]
enum FidelityLevel {
    Full,        // 完整保留
    Compressed,  // 精简保留
    Placeholder, // 占位符
}

#[derive(Clone)]
pub struct ConversationStore {
    pool: SqlitePool,
    llm: Arc<OpenAIChat>,
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
        
        let compress_config = config.conversation.clone().unwrap_or_default();
        
        Ok(Self {
            pool,
            llm,
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
                compressed_until INTEGER DEFAULT 0,
                important_context TEXT DEFAULT '',
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
        
        sqlx::query("ALTER TABLE session ADD COLUMN compressed_until INTEGER DEFAULT 0")
            .execute(pool).await.ok();
        sqlx::query("ALTER TABLE session ADD COLUMN important_context TEXT DEFAULT ''")
            .execute(pool).await.ok();
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_session_updated ON session(time_updated)")
            .execute(pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS api_stats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_type TEXT NOT NULL,
                tokens_used INTEGER DEFAULT 0,
                duration_ms INTEGER DEFAULT 0,
                success INTEGER DEFAULT 1,
                time_created INTEGER NOT NULL
            )"
        )
        .execute(pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_api_stats_time ON api_stats(time_created)")
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
            &session_id,
            history,
            request.message.clone(),
            search_mode,
            rag_sources.clone(),
            compress_mode.clone(),
        ).await?;
        
        let reply = self.llm.invoke(messages, None).await
            .map_err(|e| ConversationError::LLMError(e.to_string()))?
            .content;
        
        self.save_message(&session_id, "user", &request.message).await?;
        self.save_message(&session_id, "assistant", &reply).await?;
        
        self.update_session_stats(&session_id).await?;
        
        // 后台压缩并持久化（不阻塞响应）
        let store = self.clone();
        let sid = session_id.clone();
        tokio::spawn(async move {
            if let Err(e) = store.compress_and_persist(&sid, compress_mode).await {
                tracing::warn!("压缩持久化失败: {:?}", e);
            }
        });
        
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
            &session_id,
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
        session_id: &str,
        history: Vec<ConversationMessage>,
        current_message: String,
        search_mode: SearchMode,
        rag_sources: Vec<SourceInfo>,
        compress_mode: CompressMode,
    ) -> Result<(Vec<Message>, bool, Option<String>), ConversationError> {
        let mut messages: Vec<Message> = Vec::new();
        
        let sys_prompt = if let Ok(ctx) = self.get_important_context(session_id).await {
            if !ctx.is_empty() {
                format!("请用中文回答用户问题。记住用户之前告诉你的信息。\n\n【重要设定】{}", ctx)
            } else {
                "请用中文回答用户问题。记住用户之前告诉你的信息。".to_string()
            }
        } else {
            "请用中文回答用户问题。记住用户之前告诉你的信息。".to_string()
        };
        messages.push(Message::system(&sys_prompt));
        
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
            CompressMode::SlidingWindow(n) => self.apply_sliding_window(history, n),
            CompressMode::TokenLimit(n) => self.apply_token_limit(history, n),
            CompressMode::Summary(n) => self.apply_summary_compression(history, n).await,
            CompressMode::Layered => self.apply_layered_compression(history).await,
            CompressMode::AdaptiveFocus(n) => self.apply_afm_compression(history, n).await,
            CompressMode::TopicSegment => self.apply_topic_compression(history).await,
            CompressMode::None => Ok((history, false, None)),
        }
    }

    fn apply_sliding_window(
        &self,
        history: Vec<ConversationMessage>,
        keep_count: Option<usize>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let max_messages = keep_count.unwrap_or(self.compress_config.max_history_messages);
        
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
        _max_tokens_override: Option<usize>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let max_tokens = _max_tokens_override.unwrap_or(self.compress_config.max_tokens);
        let keep_first_n = self.compress_config.keep_first_n_messages;
        let original_len = history.len();
        
        let first_n = history.iter().take(keep_first_n).cloned().collect::<Vec<_>>();
        let remaining = history.into_iter().skip(keep_first_n).collect::<Vec<_>>();
        
        let mut recent: Vec<ConversationMessage> = Vec::new();
        let mut total_tokens = 0;
        
        for msg in remaining.into_iter().rev() {
            let msg_tokens = estimate_tokens(&msg.content);
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
        threshold_override: Option<usize>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let keep_recent = self.compress_config.keep_recent_messages;
        let threshold = threshold_override.unwrap_or(self.compress_config.compress_threshold);
        if history.len() <= keep_recent || history.len() <= threshold {
            return Ok((history, false, None));
        }
        
        let to_compress_count = history.len() - keep_recent;
        let to_compress = history.iter().take(to_compress_count).cloned().collect::<Vec<_>>();
        let recent = history.into_iter().skip(to_compress_count).collect::<Vec<_>>();
        
        // 限制每次最多传 20 条给 LLM，防止第一次压缩太久
        let summary_batch = if to_compress.len() > 20 {
            to_compress.iter().skip(to_compress.len() - 20).cloned().collect::<Vec<_>>()
        } else {
            to_compress.clone()
        };
        let summary = match self.generate_summary(&summary_batch).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("摘要生成失败，降级为简单截断: {:?}", e);
                let recent = recent.clone();
                return Ok((recent, true, Some(format!("摘要生成失败，保留最近 {} 条", keep_recent))));
            }
        };
        let summary_tokens = estimate_tokens(&summary);
        
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

    /// 压缩并持久化到 SQLite：删掉旧消息，插入摘要消息
    /// 下次 get_history 直接读到压缩后的数据，不再重复压缩
    pub async fn compress_and_persist(
        &self,
        session_id: &str,
        mode: CompressMode,
    ) -> Result<(), ConversationError> {
        let compressed_until: i64 = sqlx::query(
            "SELECT COALESCE(compressed_until, 0) FROM session WHERE id = ?"
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?
        .map(|r| r.get::<i64, _>(0))
        .unwrap_or(0);

        // 只查未压缩的消息（不含 summary）
        let uncomprised = sqlx::query(
            "SELECT id, session_id, role, content, tokens, time_created 
             FROM message WHERE session_id = ? AND time_created > ? AND role != 'summary'
             ORDER BY time_created"
        )
        .bind(session_id)
        .bind(compressed_until)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?
        .into_iter()
        .map(|row| ConversationMessage {
            id: row.get::<String, _>(0),
            session_id: row.get::<String, _>(1),
            role: row.get::<String, _>(2),
            content: row.get::<String, _>(3),
            tokens: row.get::<i64, _>(4),
            time_created: row.get::<i64, _>(5),
        })
        .collect::<Vec<_>>();

        tracing::info!("compress_and_persist: session={}, uncomprised={}, compressed_until={}, mode={:?}",
            session_id, uncomprised.len(), compressed_until, mode);

        if uncomprised.len() < 2 {
            return Ok(());
        }

        // 分层压缩：提取重要消息的关键信息
        let is_context_mode = matches!(mode, CompressMode::Layered | CompressMode::AdaptiveFocus(_) | CompressMode::TopicSegment);
        if is_context_mode {
            let keywords = &self.compress_config.important_keywords;
            let important: Vec<ConversationMessage> = uncomprised.iter()
                .filter(|m| keywords.iter().any(|k| m.content.contains(k)))
                .cloned()
                .collect();
            if !important.is_empty() {
                self.extract_and_save_important_context(session_id, &important).await.ok();
            }
        }

        let new_until = uncomprised.iter().map(|m| m.time_created).max().unwrap_or(0);
        let (compressed, was_compressed, info) = self.apply_compression(uncomprised, mode).await?;
        tracing::info!("compress_and_persist: was_compressed={}, info={:?}", was_compressed, info);
        if !was_compressed {
            return Ok(());
        }

        // 只插入新生成的摘要，不删旧消息
        for msg in &compressed {
            if msg.role == "summary" {
                let exists = sqlx::query("SELECT COUNT(*) FROM message WHERE id = ?")
                    .bind(&msg.id)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|e| ConversationError::SqliteError(e.to_string()))?
                    .get::<i64, _>(0) > 0;
                if !exists {
                    sqlx::query(
                        "INSERT INTO message (id, session_id, role, content, tokens, time_created) VALUES (?, ?, ?, ?, ?, ?)"
                    )
                    .bind(&msg.id).bind(session_id).bind(&msg.role).bind(&msg.content).bind(msg.tokens).bind(msg.time_created)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
                }
            }
        }

        // 更新压缩进度
        sqlx::query("UPDATE session SET compressed_until = ? WHERE id = ?")
            .bind(new_until).bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;

        self.update_session_stats(session_id).await?;
        Ok(())
    }

    /// 读取重要上下文
    pub async fn get_important_context(&self, session_id: &str) -> Result<String, ConversationError> {
        let row = sqlx::query("SELECT COALESCE(important_context, '') FROM session WHERE id = ?")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        Ok(row.map(|r| r.get::<String, _>(0)).unwrap_or_default())
    }

    /// 写入重要上下文
    pub async fn set_important_context(&self, session_id: &str, context: &str) -> Result<(), ConversationError> {
        sqlx::query("UPDATE session SET important_context = ? WHERE id = ?")
            .bind(context).bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        Ok(())
    }

    /// 从重要消息中提取关键信息，更新 important_context
    pub async fn extract_and_save_important_context(
        &self,
        session_id: &str,
        important_messages: &[ConversationMessage],
    ) -> Result<(), ConversationError> {
        if important_messages.is_empty() {
            return Ok(());
        }
        let text = important_messages.iter()
            .map(|m| format!("{}(用户): {}", m.time_created, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!("从以下对话中提取关键设定和重要信息（50字以内），只输出要点：\n\n{}", text);
        match self.llm.invoke(vec![Message::human(&prompt)], None).await {
            Ok(result) => {
                let old = self.get_important_context(session_id).await?;
                let new = if old.is_empty() { result.content } else { format!("{}；{}", old, result.content) };
                self.set_important_context(session_id, &new).await?;
            }
            Err(e) => tracing::warn!("提取重要上下文失败: {:?}", e),
        }
        Ok(())
    }
    
    async fn apply_layered_compression(
        &self,
        history: Vec<ConversationMessage>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let max_messages = self.compress_config.max_history_messages;
        let keep_recent = self.compress_config.keep_recent_messages;
        let threshold = self.compress_config.compress_threshold;
        let keywords = &self.compress_config.important_keywords;
        
        if history.len() <= keep_recent || history.len() <= threshold {
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
        
        // 限制每次最多传 20 条给 LLM
        let summary_batch = if to_compress.len() > 20 {
            to_compress.iter().skip(to_compress.len() - 20).cloned().collect::<Vec<_>>()
        } else {
            to_compress.clone()
        };
        
        let summary = if summary_batch.len() > 3 {
            match self.generate_summary(&summary_batch).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("分层压缩摘要生成失败，降级: {:?}", e);
                    format!("[历史摘要: 省略了 {} 条消息]", to_compress.len())
                }
            }
        } else {
            summary_batch.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n")
        };
        let summary_tokens = estimate_tokens(&summary) as i64;
        
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

    /// AFM 自适应保真度压缩
    /// 每条消息分为三档：Full（完整保留）/ Compressed（LLM精简）/ Placeholder（一行占位）
    async fn apply_afm_compression(
        &self,
        history: Vec<ConversationMessage>,
        budget_override: Option<usize>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let keep_recent = self.compress_config.keep_recent_messages;
        let threshold = self.compress_config.compress_threshold;
        if history.len() <= keep_recent || history.len() <= threshold {
            return Ok((history, false, None));
        }

        // 分离最近消息（完整保留）
        let recent: Vec<ConversationMessage> = history.iter()
            .rev().take(keep_recent).rev().cloned().collect();
        let recent_ids: Vec<String> = recent.iter().map(|m| m.id.clone()).collect();

        // 剩余消息按保真度压缩
        let to_classify: Vec<&ConversationMessage> = history.iter()
            .filter(|m| !recent_ids.contains(&m.id))
            .collect();

        let budget = budget_override.unwrap_or(self.compress_config.max_tokens);

        // 调 LLM 对每条消息做三档分类
        let classification = self.classify_messages(&to_classify).await;

        let mut full_msgs: Vec<ConversationMessage> = Vec::new();
        let mut compressed_msgs: Vec<String> = Vec::new();
        let mut token_count = 0;
        let mut placeholder_count = 0;

        for (msg, cls) in to_classify.into_iter().zip(classification.iter()) {
            match cls {
                FidelityLevel::Full => {
                    full_msgs.push(msg.clone());
                    token_count += msg.tokens as usize;
                }
                FidelityLevel::Compressed => {
                    let condensed = self.condense_message(msg).await;
                    let t = estimate_tokens(&condensed);
                    if token_count + t <= budget {
                        compressed_msgs.push(condensed);
                        token_count += t;
                    } else {
                        placeholder_count += 1;
                    }
                }
                FidelityLevel::Placeholder => {
                    placeholder_count += 1;
                }
            }
        }

        let placeholder_msg = if placeholder_count > 0 {
            vec![ConversationMessage {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: "".to_string(),
                role: "summary".to_string(),
                content: format!("[省略 {} 条低优先级消息]", placeholder_count),
                tokens: 5,
                time_created: Utc::now().timestamp_millis(),
            }]
        } else {
            vec![]
        };

        let full_count = full_msgs.len();
        let compressed_count = compressed_msgs.len();
        let mut result: Vec<ConversationMessage> = Vec::new();
        result.append(&mut full_msgs);
        for c in compressed_msgs.drain(..) {
            let t = estimate_tokens(&c) as i64;
            result.push(ConversationMessage {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: "".to_string(),
                role: "summary".to_string(),
                content: c,
                tokens: t,
                time_created: Utc::now().timestamp_millis(),
            });
        }
        result.extend(placeholder_msg);
        result.extend(recent);

        let info = format!("AFM: {} 完整保留, {} 精简, {} 占位, {} 最近",
            full_count, compressed_count, placeholder_count, keep_recent);
        Ok((result, true, Some(info)))
    }

    /// LLM 对一批消息做三档保真度分类
    async fn classify_messages(&self, messages: &[&ConversationMessage]) -> Vec<FidelityLevel> {
        if messages.is_empty() { return vec![]; }
        let text = messages.iter()
            .enumerate()
            .map(|(i, m)| format!("[{}] {}: {}", i, m.role, m.content.chars().take(100).collect::<String>()))
            .collect::<Vec<_>>().join("\n");
        let prompt = format!(
            "以下是一段对话的多条消息。请对每条消息分类：\n\
             F=完整保留（含关键设定、用户约束、人物信息）\n\
             C=精简保留（有参考价值但非关键）\n\
             P=占位符（闲聊或无关内容）\n\n\
             按顺序输出每条的分类（F/C/P），逗号分隔。\n\n{}", text);
        match self.llm.invoke(vec![Message::human(&prompt)], None).await {
            Ok(r) => {
                let parts: Vec<&str> = r.content.trim().split(|c| c == ',' || c == '、')
                    .map(|s| s.trim()).collect();
                messages.iter().enumerate().map(|(i, _)| {
                    match parts.get(i).copied().unwrap_or("P") {
                        "F" | "f" => FidelityLevel::Full,
                        "C" | "c" => FidelityLevel::Compressed,
                        _ => FidelityLevel::Placeholder,
                    }
                }).collect()
            }
            Err(_) => vec![FidelityLevel::Placeholder; messages.len()],
        }
    }

    /// 将单条消息精简为一句话
    async fn condense_message(&self, msg: &ConversationMessage) -> String {
        let prompt = format!("用一句话总结这条消息（20字内）：{}", msg.content.chars().take(200).collect::<String>());
        if let Ok(r) = self.llm.invoke(vec![Message::human(&prompt)], None).await {
            r.content.chars().take(80).collect()
        } else {
            msg.content.chars().take(40).collect()
        }
    }

    /// 话题分段压缩：检测话题边界，每段独立摘要
    async fn apply_topic_compression(
        &self,
        history: Vec<ConversationMessage>,
    ) -> Result<(Vec<ConversationMessage>, bool, Option<String>), ConversationError> {
        let keep_recent = self.compress_config.keep_recent_messages;
        let threshold = self.compress_config.compress_threshold;
        if history.len() <= keep_recent || history.len() <= threshold {
            return Ok((history, false, None));
        }

        // 分离最近消息
        let recent: Vec<ConversationMessage> = history.iter()
            .rev().take(keep_recent).rev().cloned().collect();
        let to_segment: Vec<&ConversationMessage> = history.iter()
            .take(history.len() - keep_recent).collect();

        // LLM 检测话题边界
        let boundaries = self.detect_topic_boundaries(&to_segment).await;

        // 按边界切分，每段独立摘要
        let mut segments: Vec<Vec<&ConversationMessage>> = Vec::new();
        let mut start = 0;
        for &end in &boundaries {
            if end > start {
                segments.push(to_segment[start..end].to_vec());
                start = end;
            }
        }
        if start < to_segment.len() {
            segments.push(to_segment[start..].to_vec());
        }

        let mut result: Vec<ConversationMessage> = Vec::new();
        let mut segment_count = 0;
        for seg in &segments {
            if seg.len() < 2 { continue; }
            let text = seg.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n");
            let prompt = format!("这是对话中的一个话题。用一句话概括这个话题（30字内）：\n\n{}",
                text.chars().take(500).collect::<String>());
            if let Ok(r) = self.llm.invoke(vec![Message::human(&prompt)], None).await {
                let summary = r.content.chars().take(80).collect::<String>();
                result.push(ConversationMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: "".to_string(),
                    role: "summary".to_string(),
                    content: format!("[话题] {}", summary),
                    tokens: estimate_tokens(&summary) as i64 + 4,
                    time_created: Utc::now().timestamp_millis(),
                });
                segment_count += 1;
            }
        }

        result.extend(recent);
        let info = format!("话题分段: {} 个话题摘要, {} 最近", segment_count, keep_recent);
        Ok((result, true, Some(info)))
    }

    /// LLM 检测话题边界：返回每条消息后是否发生话题切换
    async fn detect_topic_boundaries(&self, messages: &[&ConversationMessage]) -> Vec<usize> {
        if messages.len() < 4 { return vec![messages.len()]; }
        let text: String = messages.iter().enumerate()
            .map(|(i, m)| format!("[{}] {}", i, m.content.chars().take(60).collect::<String>()))
            .collect::<Vec<_>>().join("\n");
        let prompt = format!(
            "以下对话中，话题切换发生在哪条消息之后？\n\
             输出切换点的序号（从0开始），逗号分隔。如果没有切换输出 -1。\n\n{}", text);
        match self.llm.invoke(vec![Message::human(&prompt)], None).await {
            Ok(r) => {
                r.content.trim().split(',')
                    .filter_map(|s| s.trim().parse::<usize>().ok())
                    .filter(|&n| n > 0 && n < messages.len())
                    .chain(std::iter::once(messages.len()))
                    .collect()
            }
            Err(_) => vec![messages.len()],
        }
    }

    async fn generate_summary(
        &self,
        messages: &[ConversationMessage],
    ) -> Result<String, ConversationError> {
        let history_text = messages.iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>().join("\n");
        
        let prompt = format!("请将以下对话压缩成摘要（100字内），保留关键设定：\n\n{}", history_text);
        
        let summary = self.llm.invoke(vec![Message::human(&prompt)], None).await
            .map_err(|e| ConversationError::LLMError(e.to_string()))?
            .content;
        
        Ok(summary)
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
    
    pub async fn update_session_title(&self, session_id: &str, title: &str) -> Result<(), ConversationError> {
        let now = Utc::now().timestamp_millis();
        
        sqlx::query("UPDATE session SET title = ?, time_updated = ? WHERE id = ?")
            .bind(title).bind(now).bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    fn generate_title_from_message(content: &str) -> String {
        let title = content.trim();
        if title.len() > 50 {
            title.chars().take(50).collect::<String>() + "..."
        } else {
            title.to_string()
        }
    }
    
    async fn save_message(&self, session_id: &str, role: &str, content: &str) -> Result<(), ConversationError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let tokens = estimate_tokens(content) as i64;
        
        let count_row = sqlx::query("SELECT COUNT(*) FROM message WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        let msg_count: i64 = count_row.get(0);
        
        if msg_count == 0 && role == "user" {
            let title = Self::generate_title_from_message(content);
            self.update_session_title(session_id, &title).await?;
        }
        
        sqlx::query("INSERT INTO message (id, session_id, role, content, tokens, time_created) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&id).bind(session_id).bind(role).bind(content).bind(tokens).bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    pub async fn edit_message(&self, message_id: &str, content: &str) -> Result<(), ConversationError> {
        let tokens = estimate_tokens(content) as i64;
        
        sqlx::query("UPDATE message SET content = ?, tokens = ? WHERE id = ?")
            .bind(content).bind(tokens).bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    pub async fn delete_message(&self, message_id: &str) -> Result<(), ConversationError> {
        sqlx::query("DELETE FROM message WHERE id = ?")
            .bind(message_id)
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
        let compressed_until: i64 = sqlx::query(
            "SELECT COALESCE(compressed_until, 0) FROM session WHERE id = ?"
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ConversationError::SqliteError(e.to_string()))?
        .map(|r| r.get::<i64, _>(0))
        .unwrap_or(0);
        
        let rows = sqlx::query(
            "SELECT id, session_id, role, content, tokens, time_created 
             FROM message 
             WHERE session_id = ? AND (time_created > ? OR role = 'summary')
             ORDER BY time_created"
        )
        .bind(session_id)
        .bind(compressed_until)
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
            CompressModeInfo { name: "afm".to_string(), label: "AFM自适应".to_string(), description: "LLM分类+F量完整保留+C精简+P占位".to_string() },
            CompressModeInfo { name: "topic".to_string(), label: "话题分段".to_string(), description: "LLM检测话题边界，每段独立摘要".to_string() },
        ]
    }
    
    pub async fn record_api_call(&self, api_type: &str, tokens: i64, duration_ms: i64, success: bool) -> Result<(), ConversationError> {
        let now = Utc::now().timestamp_millis();
        
        sqlx::query("INSERT INTO api_stats (api_type, tokens_used, duration_ms, success, time_created) VALUES (?, ?, ?, ?, ?)")
            .bind(api_type).bind(tokens).bind(duration_ms).bind(success as i64).bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        Ok(())
    }
    
    pub async fn get_api_stats(&self) -> Result<ApiStatsSummary, ConversationError> {
        let now = Utc::now().timestamp_millis();
        let day_ago = now - 86400000;
        let week_ago = now - 604800000;
        
        let total_row = sqlx::query("SELECT COUNT(*), SUM(tokens_used), SUM(duration_ms), SUM(CASE WHEN success=1 THEN 1 ELSE 0 END) FROM api_stats")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        let day_row = sqlx::query("SELECT COUNT(*), SUM(tokens_used), AVG(duration_ms) FROM api_stats WHERE time_created > ?")
            .bind(day_ago)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        let week_row = sqlx::query("SELECT COUNT(*), SUM(tokens_used) FROM api_stats WHERE time_created > ?")
            .bind(week_ago)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        let type_rows = sqlx::query("SELECT api_type, COUNT(*), SUM(tokens_used), AVG(duration_ms) FROM api_stats GROUP BY api_type")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        let api_types: Vec<ApiTypeStats> = type_rows.into_iter().map(|row| ApiTypeStats {
            api_type: row.get::<String, _>(0),
            call_count: row.get::<i64, _>(1),
            tokens_used: row.get::<i64, _>(2),
            avg_duration_ms: row.get::<f64, _>(3) as i64,
        }).collect();
        
        Ok(ApiStatsSummary {
            total_calls: total_row.get::<i64, _>(0),
            total_tokens: total_row.get::<i64, _>(1),
            total_duration_ms: total_row.get::<i64, _>(2),
            success_count: total_row.get::<i64, _>(3),
            calls_today: day_row.get::<i64, _>(0),
            tokens_today: day_row.get::<i64, _>(1),
            avg_duration_today_ms: day_row.get::<f64, _>(2) as i64,
            calls_this_week: week_row.get::<i64, _>(0),
            tokens_this_week: week_row.get::<i64, _>(1),
            api_types,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiStatsSummary {
    pub total_calls: i64,
    pub total_tokens: i64,
    pub total_duration_ms: i64,
    pub success_count: i64,
    pub calls_today: i64,
    pub tokens_today: i64,
    pub avg_duration_today_ms: i64,
    pub calls_this_week: i64,
    pub tokens_this_week: i64,
    pub api_types: Vec<ApiTypeStats>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiTypeStats {
    pub api_type: String,
    pub call_count: i64,
    pub tokens_used: i64,
    pub avg_duration_ms: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecentCall {
    pub id: i64,
    pub api_type: String,
    pub tokens_used: i64,
    pub duration_ms: i64,
    pub success: bool,
    pub time_created: String,
}

pub fn estimate_tokens(text: &str) -> usize {
    let char_count = text.chars().count();
    let chinese_chars = text.chars().filter(|c| *c > '\u{4E00}' && *c < '\u{9FFF}').count();
    (chinese_chars * 2 + (char_count - chinese_chars)) / 4 + 1
}

impl ConversationStore {
    pub async fn regenerate_message(&self, message_id: &str) -> Result<(String, String, String), ConversationError> {
        let row = sqlx::query("SELECT session_id, role FROM message WHERE id = ?")
            .bind(message_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        let session_id: String = row.get(0);
        let role: String = row.get(1);
        
        if role != "assistant" {
            return Err(ConversationError::InvalidOperation("只能重新生成AI回复".to_string()));
        }
        
        self.delete_message(message_id).await?;
        
        let history = self.get_history(&session_id).await?;
        let last_user_msg = history.iter().rev().find(|m| m.role == "user");
        
        if let Some(user_msg) = last_user_msg {
            let messages = vec![
                Message::system("请用中文回答用户问题。"),
                Message::human(&user_msg.content),
            ];
            
            let reply = self.llm.invoke(messages, None).await
                .map_err(|e| ConversationError::LLMError(e.to_string()))?
                .content;
            
            let new_id = uuid::Uuid::new_v4().to_string();
            self.save_message(&session_id, "assistant", &reply).await?;
            
            Ok((session_id, new_id, reply))
        } else {
            Err(ConversationError::InvalidOperation("没有找到用户消息".to_string()))
        }
    }
    
    pub async fn get_session_info(&self, session_id: &str) -> Result<SessionInfo, ConversationError> {
        let row = sqlx::query("SELECT id, title, time_created, message_count FROM session WHERE id = ?")
            .bind(session_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?;
        
        let time_created = row.get::<i64, _>(2);
        let created_at = chrono::DateTime::from_timestamp_millis(time_created)
            .map(|dt| dt.to_rfc3339()).unwrap_or_default();
        
        Ok(SessionInfo {
            session_id: row.get::<String, _>(0),
            title: row.get::<String, _>(1),
            created_at,
            message_count: row.get::<i64, _>(3) as usize,
            preview: String::new(),
        })
    }
    
    pub async fn import_session(&self, import: crate::models::SessionImport) -> Result<String, ConversationError> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let title = import.title.unwrap_or_else(|| "导入会话".to_string());
        self.create_session(&session_id, &title).await?;
        
        for msg in import.messages {
            self.save_message(&session_id, &msg.role, &msg.content).await?;
        }
        
        self.update_session_stats(&session_id).await?;
        Ok(session_id)
    }
    
    pub async fn search_sessions(&self, query: &str) -> Result<Vec<SessionInfo>, ConversationError> {
        let rows = sqlx::query(
            "SELECT s.id, s.title, s.time_created, s.message_count 
             FROM session s 
             JOIN message m ON s.id = m.session_id 
             WHERE m.content LIKE ? OR s.title LIKE ?
             GROUP BY s.id 
             ORDER BY s.time_updated DESC 
             LIMIT 20"
        )
        .bind(format!("%{}%", query))
        .bind(format!("%{}%", query))
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
    
    pub async fn branch_session(&self, session_id: &str, from_message_id: &str) -> Result<(String, String, usize), ConversationError> {
        let original_title = sqlx::query("SELECT title FROM session WHERE id = ?")
            .bind(session_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ConversationError::SqliteError(e.to_string()))?
            .get::<String, _>(0);
        
        let messages = self.get_history(session_id).await?;
        let cutoff_time = messages.iter()
            .find(|m| m.id == from_message_id)
            .map(|m| m.time_created)
            .ok_or_else(|| ConversationError::InvalidOperation("消息不存在".to_string()))?;
        
        let new_session_id = uuid::Uuid::new_v4().to_string();
        let new_title = format!("{} (分支)", original_title);
        self.create_session(&new_session_id, &new_title).await?;
        
        let messages_to_copy: Vec<ConversationMessage> = messages.into_iter()
            .filter(|m| m.time_created <= cutoff_time)
            .collect();
        
        let count = messages_to_copy.len();
        
        for msg in messages_to_copy {
            self.save_message(&new_session_id, &msg.role, &msg.content).await?;
        }
        
        self.update_session_stats(&new_session_id).await?;
        
        Ok((new_session_id, new_title, count))
    }
}