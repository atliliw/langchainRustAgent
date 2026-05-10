//! Agent 工具系统
//!
//! 可扩展的工具注册机制，每个工具实现 `AgentTool` trait。
//! 新增工具只需实现 trait 并注册到 `ToolRegistry`。
//!
//! 内置工具：
//! - llm_query: 直接用 LLM 回答
//! - web_search: 搜索网络获取信息（当前降级为 LLM 回答）
//! - weather: 查询天气
//! - code_execute: 执行代码（LLM 生成）
//! - read_file: 读取文件（LLM 分析）
//! - summarize: 总结
//! - rag_search: 检索知识库（RAG）

use crate::config::Config;
use crate::stores::QdrantStore;
use async_trait::async_trait;
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::sleep;

/// 工具执行上下文
pub struct ToolContext {
    pub config: Config,
    pub task: String,
    pub description: String,
    /// 前置已完成任务的输出汇总
    pub ctx: String,
    /// RAG 知识库检索结果
    pub rag: String,
    pub input_template: String,
    pub vector_store: Option<Arc<QdrantStore>>,
    /// 取消信号（true=已取消）
    pub cancel: Arc<AtomicBool>,
    /// 进度推送 channel（SSE用）
    pub progress: Option<broadcast::Sender<String>>,
}

impl ToolContext {
    /// 检查是否已取消
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// 推送进度事件
    pub fn send_progress(&self, event: &str) {
        if let Some(ref tx) = self.progress {
            let _ = tx.send(event.to_string());
        }
    }
}

/// Agent 工具 trait
///
/// 所有 Agent 可使用的工具都必须实现此 trait。
/// - `name()`: 工具唯一标识符
/// - `description()`: 工具描述（用于 LLM 规划 prompt）
/// - `execute()`: 工具执行逻辑
#[async_trait]
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, ctx: &ToolContext) -> Result<(String, usize), String>;
}

// ──── 重试辅助 ────

/// 带指数退避的重试执行（不带超时）
/// 默认 3 次重试，间隔 [1s, 3s, 5s]
#[allow(dead_code)]
async fn with_retry<F, Fut>(max_retries: usize, f: F) -> Result<(String, usize), String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<(String, usize), String>>,
{
    let backoff = [1u64, 3, 5];
    let mut last_error = String::new();
    for attempt in 1..=max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = e;
                tracing::warn!("工具执行 attempt {}/{} 失败: {}", attempt, max_retries, last_error);
                if attempt < max_retries {
                    let delay = backoff.get(attempt - 1).copied().unwrap_or(5);
                    sleep(Duration::from_secs(delay)).await;
                }
            }
        }
    }
    Err(format!("执行失败(重试{}次后): {}", max_retries, last_error))
}

/// 带超时的重试执行
async fn with_timeout_retry<F, Fut>(
    timeout_secs: u64,
    max_retries: usize,
    tool_name: &str,
    cancel: Option<&Arc<AtomicBool>>,
    f: F,
) -> Result<(String, usize), String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<(String, usize), String>>,
{
    // 检查初始状态是否已取消
    if let Some(c) = cancel {
        if c.load(Ordering::Relaxed) {
            return Err("任务已取消".to_string());
        }
    }

    let backoff = [1u64, 3, 5];
    let mut last_error = String::new();
    for attempt in 1..=max_retries {
        // 每次重试前检查取消信号
        if attempt > 1 {
            if let Some(c) = cancel {
                if c.load(Ordering::Relaxed) {
                    return Err("任务已取消".to_string());
                }
            }
        }

        match tokio::time::timeout(Duration::from_secs(timeout_secs), f()).await {
            Ok(Ok(result)) => return Ok(result),
            Ok(Err(e)) => {
                last_error = e;
                tracing::warn!("{} attempt {}/{} failed: {}", tool_name, attempt, max_retries, last_error);
            }
            Err(_) => {
                last_error = "timeout".to_string();
                tracing::warn!("{} attempt {}/{} timeout", tool_name, attempt, max_retries);
            }
        }
        if attempt < max_retries {
            let delay = backoff.get(attempt - 1).copied().unwrap_or(5);
            sleep(Duration::from_secs(delay)).await;
        }
    }
    Err(format!("{} 执行失败(重试{}次后): {}", tool_name, max_retries, last_error))
}

/// 当 API 不返回 token_usage 时，用文本长度估算 token 数
fn estimate_token_usage(prompt: &str, response: &str) -> usize {
    let input_tokens = prompt.chars().count() / 2 + 1;
    let output_tokens = response.chars().count() / 2 + 1;
    input_tokens + output_tokens
}

// ──── LLM 调用辅助 ────

fn make_llm(config: &Config) -> OpenAIChat {
    OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(2048))
}

fn call_llm<'a>(llm: &'a OpenAIChat, prompt: &'a str) -> impl std::future::Future<Output = Result<(String, usize), String>> + 'a {
    async move {
        let r = llm.invoke(vec![Message::human(prompt)], None)
            .await
            .map_err(|e| e.to_string())?;
        let tokens = r.token_usage
            .as_ref()
            .map(|u| u.total_tokens)
            .unwrap_or_else(|| estimate_token_usage(prompt, &r.content));
        Ok((r.content.clone(), tokens))
    }
}

// ──── 各工具实现 ────

/// LLM 直接回答工具
pub struct LlmQueryTool;

#[async_trait]
impl AgentTool for LlmQueryTool {
    fn name(&self) -> &str { "llm_query" }
    fn description(&self) -> &str { "直接用 LLM 回答" }

    async fn execute(&self, ctx: &ToolContext) -> Result<(String, usize), String> {
        let prompt = if ctx.ctx.is_empty() {
            format!("任务：{}\n当前子任务：{}\n\n请执行当前子任务并输出结果。{}",
                ctx.task, ctx.description, ctx.rag)
        } else {
            format!("任务：{}\n当前子任务：{}\n\n前置完成的任务结果：\n{}\n\n请基于前置结果执行当前子任务并输出。{}",
                ctx.task, ctx.description, ctx.ctx, ctx.rag)
        };
        let llm = make_llm(&ctx.config);
        with_timeout_retry(120, 3, "llm_query", Some(&ctx.cancel), || call_llm(&llm, &prompt)).await
    }
}

/// 网络搜索工具
pub struct WebSearchTool;

/// 执行真实网络搜索（根据配置的搜索引擎）
async fn search_web(query: &str, config: &Config) -> Result<String, String> {
    let engine = &config.search_engine;
    if engine.provider.is_empty() || engine.base_url.is_empty() {
        return Err("搜索引擎未配置".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    match engine.provider.as_str() {
        "searxng" => {
            let url = format!("{}/search?q={}&format=json", engine.base_url, urlencode(query));
            let resp = client.get(&url)
                .header("User-Agent", "LangChainRust-Agent/1.0")
                .send().await
                .map_err(|e| format!("SearXNG 请求失败: {}", e))?;
            if !resp.status().is_success() {
                return Err(format!("SearXNG 返回: {}", resp.status()));
            }
            let body: serde_json::Value = resp.json().await
                .map_err(|e| format!("SearXNG 解析失败: {}", e))?;
            let results = body["results"].as_array().ok_or("SearXNG 返回格式异常")?;
            if results.is_empty() {
                return Err("未找到相关结果".to_string());
            }
            let mut output = Vec::new();
            for r in results.iter().take(5) {
                let title = r["title"].as_str().unwrap_or("");
                let url = r["url"].as_str().unwrap_or("");
                let content = r["content"].as_str().unwrap_or("");
                output.push(format!("【{}】\n{}", title, content));
                if !url.is_empty() {
                    output.push(format!("  来源: {}", url));
                }
            }
            Ok(output.join("\n\n"))
        }
        "bing" => {
            let url = format!("{}/v7.0/search?q={}&count=5&mkt=zh-CN", engine.base_url, urlencode(query));
            let resp = client.get(&url)
                .header("Ocp-Apim-Subscription-Key", &engine.api_key)
                .send().await
                .map_err(|e| format!("Bing 请求失败: {}", e))?;
            if !resp.status().is_success() {
                return Err(format!("Bing 返回: {}", resp.status()));
            }
            let body: serde_json::Value = resp.json().await
                .map_err(|e| format!("Bing 解析失败: {}", e))?;
            let results = body["webPages"]["value"].as_array().ok_or("Bing 返回格式异常")?;
            if results.is_empty() {
                return Err("未找到相关结果".to_string());
            }
            let mut output = Vec::new();
            for r in results.iter().take(5) {
                let name = r["name"].as_str().unwrap_or("");
                let url = r["url"].as_str().unwrap_or("");
                let snippet = r["snippet"].as_str().unwrap_or("");
                output.push(format!("【{}】\n{}", name, snippet));
                if !url.is_empty() {
                    output.push(format!("  来源: {}", url));
                }
            }
            Ok(output.join("\n\n"))
        }
        _ => Err(format!("不支持的搜索引擎: {}", engine.provider)),
    }
}

fn urlencode(s: &str) -> String {
    s.chars().map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
        _ => format!("%{:02X}", c as u8),
    }).collect()
}

#[async_trait]
impl AgentTool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    fn description(&self) -> &str { "搜索网络获取信息" }

    async fn execute(&self, ctx: &ToolContext) -> Result<(String, usize), String> {
        // 构造搜索关键词
        let query = if ctx.input_template.is_empty() {
            format!("{} {}", ctx.task, ctx.description)
        } else {
            ctx.input_template.clone()
        };

        // 尝试真实搜索
        match search_web(&query, &ctx.config).await {
            Ok(results) => {
                // 用 LLM 总结搜索结果
                let prompt = format!(
                    "任务：{}\n当前子任务：{}\n\n以下是网络搜索结果：\n{}\n\n请基于以上搜索结果，总结回答。",
                    ctx.task, ctx.description, results
                );
                let llm = make_llm(&ctx.config);
                with_timeout_retry(60, 3, "web_search_summarize", Some(&ctx.cancel), || call_llm(&llm, &prompt)).await
            }
            Err(_) => {
                // 搜索引擎不可用时降级为 LLM 知识回答
                tracing::info!("web_search 降级为 LLM 知识回答（未配置搜索引擎）");
                let prompt = format!("任务：{}\n当前子任务：{}\n\n请基于你的知识回答。",
                    ctx.task, ctx.description);
                let llm = make_llm(&ctx.config);
                with_timeout_retry(60, 3, "web_search", Some(&ctx.cancel), || call_llm(&llm, &prompt)).await
            }
        }
    }
}

/// 天气查询工具
pub struct WeatherTool;

impl WeatherTool {
    async fn query_weather(city: &str) -> Result<String, String> {
        let e: String = city.chars().map(|c| match c {
            'A'..='Z'|'a'..='z'|'0'..='9'|'-'|'_'|'.'|'~' => c.to_string(),
            _ => format!("%{:02X}", c as u8)
        }).collect();
        reqwest::get(&format!("https://wttr.in/{}?format=%C+|+%t+|+%h+|+%w&lang=zh", e)).await
            .map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())
    }
}

#[async_trait]
impl AgentTool for WeatherTool {
    fn name(&self) -> &str { "weather" }
    fn description(&self) -> &str { "查询天气" }

    async fn execute(&self, ctx: &ToolContext) -> Result<(String, usize), String> {
        let city_prompt = format!("任务：{}\n当前子任务：{}\n\n请输出要查询天气的城市名，不要多余内容。",
            ctx.task, ctx.description);
        let llm = make_llm(&ctx.config);

        let city = with_timeout_retry(15, 2, "weather_city", Some(&ctx.cancel), || call_llm(&llm, &city_prompt))
            .await
            .map(|(c, _)| c.trim().to_string())
            .unwrap_or_else(|_| ctx.description.clone());

        match Self::query_weather(&city).await {
            Ok(r) => Ok((format!("{}的天气：{}", city, r), 0)),
            Err(e) => Ok((format!("天气查询失败: {}", e), 0)),
        }
    }
}

/// 代码执行工具（LLM 生成代码）
pub struct CodeExecuteTool;

#[async_trait]
impl AgentTool for CodeExecuteTool {
    fn name(&self) -> &str { "code_execute" }
    fn description(&self) -> &str { "执行代码" }

    async fn execute(&self, ctx: &ToolContext) -> Result<(String, usize), String> {
        let prompt = if ctx.ctx.is_empty() {
            format!("任务：{}\n当前子任务：{}\n\n请编写并执行代码，输出结果。{}",
                ctx.task, ctx.description, ctx.rag)
        } else {
            format!("任务：{}\n当前子任务：{}\n\n前置结果：\n{}\n\n请基于前置结果编写并执行代码。{}",
                ctx.task, ctx.description, ctx.ctx, ctx.rag)
        };
        let llm = make_llm(&ctx.config);
        with_timeout_retry(120, 3, "code_execute", Some(&ctx.cancel), || call_llm(&llm, &prompt)).await
    }
}

/// 读取文件工具（LLM 分析文件内容）
pub struct ReadFileTool;

#[async_trait]
impl AgentTool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str { "读取文件" }

    async fn execute(&self, ctx: &ToolContext) -> Result<(String, usize), String> {
        let prompt = format!("任务：{}\n当前子任务：{}\n\n请读取并分析文件内容，输出结果。{}",
            ctx.task, ctx.description, ctx.rag);
        let llm = make_llm(&ctx.config);
        with_timeout_retry(120, 3, "read_file", Some(&ctx.cancel), || call_llm(&llm, &prompt)).await
    }
}

/// 总结工具
pub struct SummarizeTool;

#[async_trait]
impl AgentTool for SummarizeTool {
    fn name(&self) -> &str { "summarize" }
    fn description(&self) -> &str { "总结" }

    async fn execute(&self, ctx: &ToolContext) -> Result<(String, usize), String> {
        let prompt = if ctx.ctx.is_empty() {
            format!("任务：{}\n当前子任务：{}\n\n请对以下内容进行总结。{}",
                ctx.task, ctx.description, ctx.rag)
        } else {
            format!("任务：{}\n当前子任务：{}\n\n前置结果：\n{}\n\n请基于前置结果进行总结。{}",
                ctx.task, ctx.description, ctx.ctx, ctx.rag)
        };
        let llm = make_llm(&ctx.config);
        with_timeout_retry(120, 3, "summarize", Some(&ctx.cancel), || call_llm(&llm, &prompt)).await
    }
}

/// RAG 知识库检索工具
pub struct RagSearchTool;

#[async_trait]
impl AgentTool for RagSearchTool {
    fn name(&self) -> &str { "rag_search" }
    fn description(&self) -> &str { "检索知识库（RAG）获取与任务相关的文档内容" }

    async fn execute(&self, ctx: &ToolContext) -> Result<(String, usize), String> {
        let query = if ctx.input_template.is_empty() {
            ctx.task.clone()
        } else {
            ctx.input_template.clone()
        };

        match &ctx.vector_store {
            Some(store) => {
                match tokio::time::timeout(Duration::from_secs(30), store.search_rag(&query, 3)).await {
                    Ok(Ok(results)) => {
                        let filtered: Vec<_> = results.iter()
                            .filter(|r| r.score >= 0.3)
                            .collect();
                        if filtered.is_empty() {
                            Ok((format!("知识库中未找到相关文档（搜索词：{}）", query), 0))
                        } else {
                            let content: Vec<String> = filtered.iter().map(|r| {
                                format!("[相关性 {:.1}%]\n{}", r.score * 100.0, r.document.content)
                            }).collect();
                            let embed_tokens = query.chars().count().max(1);
                            Ok((format!("知识库检索到以下相关信息（搜索词：{}，Embedding 消耗 ~{} tokens）：\n\n{}",
                                query, embed_tokens, content.join("\n\n---\n\n")), embed_tokens))
                        }
                    }
                    Ok(Err(_)) => Ok((format!("知识库搜索失败（搜索词：{}）", query), 0)),
                    Err(_) => Ok((format!("知识库搜索超时（搜索词：{}）", query), 0)),
                }
            }
            None => {
                if ctx.rag.is_empty() {
                    Ok(("知识库中未找到相关文档".to_string(), 0))
                } else {
                    Ok((format!("知识库检索到以下相关信息：\n\n{}", ctx.rag), 0))
                }
            }
        }
    }
}

// ──── 工具注册器 ────

/// 工具注册器，管理所有可用工具
pub struct ToolRegistry {
    tools: Vec<Box<dyn AgentTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// 注册一个工具
    pub fn register(&mut self, tool: Box<dyn AgentTool>) {
        // 避免重复注册同名工具
        let name = tool.name().to_string();
        if !self.tools.iter().any(|t| t.name() == name) {
            self.tools.push(tool);
        }
    }

    /// 按名称获取工具
    pub fn get(&self, name: &str) -> Option<&dyn AgentTool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    /// 判断工具是否存在
    pub fn has(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.name() == name)
    }

    /// 列举所有工具（名称, 描述）
    pub fn list_descriptions(&self) -> Vec<(&str, &str)> {
        self.tools.iter().map(|t| (t.name(), t.description())).collect()
    }

    /// 工具数量
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// 创建默认工具集
    pub fn default_registry() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(LlmQueryTool));
        registry.register(Box::new(WebSearchTool));
        registry.register(Box::new(WeatherTool));
        registry.register(Box::new(CodeExecuteTool));
        registry.register(Box::new(ReadFileTool));
        registry.register(Box::new(SummarizeTool));
        registry.register(Box::new(RagSearchTool));
        registry
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::default_registry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_default_tools() {
        let registry = ToolRegistry::default_registry();
        assert_eq!(registry.len(), 7);
        assert!(registry.has("llm_query"));
        assert!(registry.has("web_search"));
        assert!(registry.has("weather"));
        assert!(registry.has("code_execute"));
        assert!(registry.has("read_file"));
        assert!(registry.has("summarize"));
        assert!(registry.has("rag_search"));
    }

    #[test]
    fn test_registry_no_duplicates() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(LlmQueryTool));
        registry.register(Box::new(LlmQueryTool)); // duplicate
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_registry_list_descriptions() {
        let registry = ToolRegistry::default_registry();
        let list = registry.list_descriptions();
        assert_eq!(list.len(), 7);
        let names: Vec<&str> = list.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"llm_query"));
        assert!(names.contains(&"rag_search"));
    }

    #[test]
    fn test_estimate_token_usage() {
        let tokens = estimate_token_usage("hello", "world");
        assert!(tokens > 0);
        assert!(tokens < 100);
    }
}
