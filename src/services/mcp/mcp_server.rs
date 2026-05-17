//! MCP Server - 将项目的内置工具暴露为标准 MCP 服务
//!
//! 实现了 MCP 协议的 tools/list 和 tools/call 方法，
//! 让外部 AI Agent（如 Claude Desktop、Cursor 等）可以通过标准 MCP 协议
//! 调用此项目的检索/搜索/LLM 等能力。
//!
//! 协议：JSON-RPC 2.0 over HTTP POST
//! 端点：POST /api/v2/mcp/server

use crate::config::Config;
use crate::services::tools::*;
use crate::stores::QdrantStore;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

// ──── JSON-RPC 2.0 消息类型 ────

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcErrorResponse>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcErrorResponse {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }

    pub fn error(id: serde_json::Value, code: i64, message: String) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: None, error: Some(JsonRpcErrorResponse { code, message, data: None }) }
    }

    pub fn method_not_found(id: serde_json::Value, method: &str) -> Self {
        Self::error(id, -32601, format!("Method not found: {}", method))
    }

    pub fn invalid_params(id: serde_json::Value, msg: String) -> Self {
        Self::error(id, -32602, msg)
    }

    pub fn internal_error(id: serde_json::Value, msg: String) -> Self {
        Self::error(id, -32603, msg)
    }
}

// ──── MCP 工具定义 ────

fn tool_schemas() -> Vec<(&'static str, &'static str, serde_json::Value)> {
    vec![
        (
            "rag_search",
            "检索知识库（RAG）获取与问题相关的文档内容。基于向量相似度搜索，返回相关性 ≥ 0.3 的文档片段",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索关键词，尽量完整描述你想找的内容"}
                },
                "required": ["query"]
            }),
        ),
        (
            "web_search",
            "搜索网络获取实时信息。支持 SearXNG 和 Bing 搜索引擎，返回搜索结果摘要",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索关键词"}
                },
                "required": ["query"]
            }),
        ),
        (
            "llm_query",
            "直接用大语言模型回答用户的问题。适用于知识问答、推理、写作等通用任务",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "发给 LLM 的提示词"}
                },
                "required": ["prompt"]
            }),
        ),
        (
            "weather",
            "查询指定城市的当前天气情况。返回天气状况、温度、湿度和风速信息",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string", "description": "城市名（中文或英文）"}
                },
                "required": ["city"]
            }),
        ),
        (
            "code_execute",
            "让 LLM 编写并执行代码来解决指定的编程任务。适用于算法实现、数据处理等场景",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "编程任务描述"}
                },
                "required": ["task"]
            }),
        ),
        (
            "read_file",
            "让 LLM 读取文件内容并进行分析。适用于代码审查、文本分析等场景",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "文件路径或文件描述"}
                },
                "required": ["path"]
            }),
        ),
        (
            "summarize",
            "对给定的文本内容进行总结概括。适用于长文本压缩、要点提取等场景",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "需要总结的文本内容"}
                },
                "required": ["text"]
            }),
        ),
    ]
}

// ──── MCP Server 服务 ────

pub struct McpServerService {
    config: Config,
    vector_store: Option<Arc<QdrantStore>>,
}

impl McpServerService {
    pub fn new(config: Config, vector_store: Option<Arc<QdrantStore>>) -> Self {
        Self { config, vector_store }
    }

    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method.as_str() {
            "tools/list" => self.handle_list_tools(request.id),
            "tools/call" => {
                let params = match request.params {
                    Some(p) => p,
                    None => return JsonRpcResponse::invalid_params(request.id, "Missing params".into()),
                };
                self.handle_call_tool(request.id, params).await
            }
            _ => JsonRpcResponse::method_not_found(request.id, &request.method),
        }
    }

    fn handle_list_tools(&self, id: serde_json::Value) -> JsonRpcResponse {
        let tools: Vec<serde_json::Value> = tool_schemas().iter().map(|(name, description, input_schema)| {
            serde_json::json!({
                "name": name,
                "description": description,
                "inputSchema": input_schema
            })
        }).collect();

        JsonRpcResponse::success(id, serde_json::json!({"tools": tools}))
    }

    async fn handle_call_tool(&self, id: serde_json::Value, params: serde_json::Value) -> JsonRpcResponse {
        let name = match params["name"].as_str() {
            Some(n) => n.to_string(),
            None => return JsonRpcResponse::invalid_params(id, "Missing 'name' in params".into()),
        };

        let arguments = params.get("arguments")
            .map(|a| a.clone())
            .unwrap_or(serde_json::json!({}));

        let registry = ToolRegistry::default_registry();
        let tool = match registry.get(&name) {
            Some(t) => t,
            None => return JsonRpcResponse::error(id, -32602, format!("Unknown tool: {}", name)),
        };

        // 构造 ToolContext
        let query = arguments.get("query")
            .or_else(|| arguments.get("prompt"))
            .or_else(|| arguments.get("text"))
            .or_else(|| arguments.get("task"))
            .or_else(|| arguments.get("path"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let city = arguments.get("city")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // weather 工具特殊处理：把 city 放 description 里让 LLM 提取
        let description = if name == "weather" && !city.is_empty() {
            format!("查询 {} 的天气", city)
        } else {
            query.clone()
        };

        let ctx = ToolContext {
            config: self.config.clone(),
            task: query.clone(),
            description,
            ctx: String::new(),
            rag: String::new(),
            input_template: query,
            vector_store: self.vector_store.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
            progress: None,
        };

        match tool.execute(&ctx).await {
            Ok((result, _tokens)) => {
                JsonRpcResponse::success(id, serde_json::json!({
                    "content": [{"type": "text", "text": result}]
                }))
            }
            Err(e) => {
                JsonRpcResponse::internal_error(id, e)
            }
        }
    }
}
