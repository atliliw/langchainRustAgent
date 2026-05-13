use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

pub struct McpClient;

impl McpClient {
    pub async fn list_tools(config: &McpServerConfig) -> Result<Vec<McpTool>, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build().map_err(|e| e.to_string())?;

        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: 1,
            method: "tools/list".into(),
            params: None,
        };

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        if let Some(ref key) = config.api_key {
            headers.insert("Authorization", format!("Bearer {}", key).parse().unwrap());
        }

        let resp = client.post(&config.url)
            .headers(headers)
            .json(&req)
            .send().await
            .map_err(|e| format!("MCP 请求失败: {}", e))?;

        let body: JsonRpcResponse = resp.json().await
            .map_err(|e| format!("MCP 响应解析失败: {}", e))?;

        if let Some(err) = body.error {
            return Err(format!("MCP 错误: {} (code {})", err.message, err.code));
        }

        let tools = match body.result {
            Some(r) => {
                let arr = r.get("tools").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                arr.into_iter().filter_map(|v| {
                    Some(McpTool {
                        name: v.get("name")?.as_str()?.to_string(),
                        description: v.get("description")?.as_str()?.to_string(),
                        parameters: v.get("inputSchema").or_else(|| v.get("parameters"))?.clone(),
                    })
                }).collect::<Vec<_>>()
            }
            None => vec![],
        };

        Ok(tools)
    }

    pub async fn call_tool(
        config: &McpServerConfig,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<String, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build().map_err(|e| e.to_string())?;

        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: 1,
            method: "tools/call".into(),
            params: Some(serde_json::json!({
                "name": tool_name,
                "arguments": args,
            })),
        };

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        if let Some(ref key) = config.api_key {
            headers.insert("Authorization", format!("Bearer {}", key).parse().unwrap());
        }

        let resp = client.post(&config.url)
            .headers(headers)
            .json(&req)
            .send().await
            .map_err(|e| format!("MCP 工具调用失败: {}", e))?;

        let body: JsonRpcResponse = resp.json().await
            .map_err(|e| format!("MCP 响应解析失败: {}", e))?;

        if let Some(err) = body.error {
            return Err(format!("MCP 工具错误: {} (code {})", err.message, err.code));
        }

        let content = match body.result {
            Some(r) => {
                r.get("content")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|item| item.get("text"))
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "工具执行完成".to_string())
            }
            None => "工具执行完成".to_string(),
        };

        Ok(content)
    }
}

pub type McpRegistry = HashMap<String, McpServerConfig>;
