use crate::config::Config;
use crate::services::tools::{ToolRegistry, ToolContext};
use langchainrust::language_models::OpenAIChat;
use langchainrust::core::tools::ToolDefinition;
use langchainrust::schema::{Message, MessageType};
use langchainrust::core::runnables::Runnable;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::sync::broadcast;
use uuid::Uuid;
use futures_util::stream::Stream;

pub struct ToolCallingEngine;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SseEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<usize>,
}

fn sse_text(content: &str) -> String {
    let e = SseEvent { event_type: "text".into(), id: None, tool: None, args: None,
        content: Some(content.to_string()), result: None, duration_ms: None, tokens: None };
    serde_json::to_string(&e).unwrap_or_default()
}

fn sse_tool_call(id: &str, tool: &str, args: &serde_json::Value) -> String {
    let e = SseEvent { event_type: "tool_call".into(), id: Some(id.into()), tool: Some(tool.into()),
        args: Some(args.clone()), content: None, result: None, duration_ms: None, tokens: None };
    serde_json::to_string(&e).unwrap_or_default()
}

fn sse_tool_result(id: &str, result: &str, ms: u64, tokens: usize) -> String {
    let e = SseEvent { event_type: "tool_result".into(), id: Some(id.into()), tool: None,
        args: None, content: None, result: Some(result.to_string()), duration_ms: Some(ms), tokens: Some(tokens) };
    serde_json::to_string(&e).unwrap_or_default()
}

fn sse_done() -> String {
    let e = SseEvent { event_type: "done".into(), id: None, tool: None, args: None,
        content: None, result: None, duration_ms: None, tokens: None };
    serde_json::to_string(&e).unwrap_or_default()
}

impl ToolCallingEngine {
    /// Convert ToolRegistry tools to OpenAI ToolDefinition format
    fn build_tool_definitions() -> Vec<ToolDefinition> {
        let registry = ToolRegistry::default_registry();
        let mut tools = Vec::new();
        for (name, desc) in registry.list_descriptions() {
            let td = ToolDefinition::new(name.to_string(), desc.to_string())
                .with_parameters(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": format!("Input for {}", name)
                        }
                    },
                    "required": ["query"]
                }));
            tools.push(td);
        }
        tools
    }

    /// Build a context string from completed results (current session context)
    fn build_context(ctx: &str, rag: &str) -> String {
        let mut parts = Vec::new();
        if !ctx.is_empty() {
            parts.push(format!("前置已完成的任务结果：\n{}", ctx));
        }
        if !rag.is_empty() {
            parts.push(format!("知识库检索结果：\n{}", rag));
        }
        parts.join("\n\n")
    }

    /// Execute a single tool and return (output, tokens)
    async fn execute_tool(
        config: &Config,
        tool_name: &str,
        args: &serde_json::Value,
        ctx: &str,
        rag: &str,
        cancel: Arc<AtomicBool>,
        progress: Option<broadcast::Sender<String>>,
    ) -> (String, usize) {
        let registry = ToolRegistry::default_registry();
        let tool_ctx = ToolContext {
            config: config.clone(),
            task: String::new(),
            description: format!("执行工具: {}", tool_name),
            ctx: Self::build_context(ctx, rag),
            rag: String::new(),
            input_template: args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            vector_store: None,
            cancel,
            progress,
        };
        match registry.get(tool_name) {
            Some(tool) => {
                match tool.execute(&tool_ctx).await {
                    Ok((output, tokens)) => (output, tokens),
                    Err(e) => (format!("工具执行失败: {}", e), 0),
                }
            }
            None => (format!("工具 '{}' 不存在", tool_name), 0),
        }
    }

    /// Chat with tool calling support. Returns an SSE stream.
    pub fn chat(
        config: Config,
        messages: Vec<ChatMessage>,
        rag_context: String,
    ) -> impl Stream<Item = String> {
        use futures_util::stream::{self, StreamExt};

        let stream = async_stream::stream! {
            let llm = OpenAIChat::new(
                config.to_langchain_openai_config()
                    .with_max_tokens(4096)
            );

            // Convert messages to langchainrust Message format
            let mut lc_messages: Vec<Message> = Vec::new();
            for msg in &messages {
                let m = match msg.role.as_str() {
                    "system" => Message::system(&msg.content),
                    "user" | "human" => Message::human(&msg.content),
                    "assistant" | "ai" => Message::ai(&msg.content),
                    _ => Message::human(&msg.content),
                };
                lc_messages.push(m);
            }

            // If there's RAG context, inject it as a system message
            if !rag_context.is_empty() {
                lc_messages.insert(0, Message::system(
                    &format!("以下是知识库检索到的相关信息，请基于这些信息回答：\n\n{}", rag_context)
                ));
            }

            let tools = Self::build_tool_definitions();
            let max_rounds = 5;

            for round in 0..max_rounds {
                let response = if round == 0 && !tools.is_empty() {
                    let mut cfg = config.to_langchain_openai_config().with_max_tokens(4096);
                    cfg.tools = Some(tools.clone());
                    let llm_with_tools = OpenAIChat::new(cfg);
                    llm_with_tools.invoke(lc_messages.clone(), None).await
                } else {
                    llm.invoke(lc_messages.clone(), None).await
                };

                match response {
                    Ok(resp) => {
                        let tool_calls = resp.tool_calls.clone().unwrap_or_default();
                        let content = resp.content.clone();

                        if tool_calls.is_empty() {
                            yield sse_text(&content);
                            yield sse_done();
                            return;
                        }

                        if !content.is_empty() {
                            yield sse_text(&content);
                        }

                        for tc in &tool_calls {
                            let tool_name = tc.name().to_string();
                            let args_str = tc.arguments();
                            let args: serde_json::Value = serde_json::from_str(args_str)
                                .unwrap_or(serde_json::json!({"query": args_str}));

                            yield sse_tool_call(&tc.id, &tool_name, &args);

                            let start = Instant::now();
                            let (output, tokens) = Self::execute_tool(
                                &config, &tool_name, &args,
                                "", "", Arc::new(AtomicBool::new(false)), None,
                            ).await;
                            let duration = start.elapsed().as_millis() as u64;

                            yield sse_tool_result(&tc.id, &output, duration, tokens);

                            lc_messages.push(Message::ai_with_tool_calls(&content, vec![tc.clone()]));
                            lc_messages.push(Message::tool(&tc.id, &output));
                        }
                    }
                    Err(e) => {
                        yield sse_text(&format!("❌ LLM 调用失败: {}", e));
                        yield sse_done();
                        return;
                    }
                }
            }
            yield sse_text("⚠️ 已达到最大工具调用轮数");
            yield sse_done();
        };
        stream
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}
