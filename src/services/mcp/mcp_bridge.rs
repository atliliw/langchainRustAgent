use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use crate::services::tools::{AgentTool, ToolContext};
use crate::services::mcp::mcp_client::{McpClient, McpServerConfig, McpTool, McpRegistry};

pub struct McpBridge {
    servers: Arc<Mutex<McpRegistry>>,
    tools_cache: Arc<Mutex<HashMap<String, Vec<McpTool>>>>,
}

impl McpBridge {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(Mutex::new(McpRegistry::new())),
            tools_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register_server(&self, name: String, config: McpServerConfig) {
        let mut reg = self.servers.lock().unwrap();
        reg.insert(name.clone(), config);
        // 清除该服务器的缓存，下次重新发现
        let mut cache = self.tools_cache.lock().unwrap();
        cache.remove(&name);
    }

    pub fn unregister_server(&self, name: &str) {
        let mut reg = self.servers.lock().unwrap();
        reg.remove(name);
        let mut cache = self.tools_cache.lock().unwrap();
        cache.remove(name);
    }

    pub fn list_servers(&self) -> Vec<(String, McpServerConfig)> {
        let reg = self.servers.lock().unwrap();
        reg.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn get_server(&self, name: &str) -> Option<McpServerConfig> {
        let reg = self.servers.lock().unwrap();
        reg.get(name).cloned()
    }

    /// 发现远端工具并缓存
    pub async fn discover_and_cache(&self, server_name: &str) -> Result<Vec<McpTool>, String> {
        let config = self.get_server(server_name)
            .ok_or_else(|| format!("MCP 服务器 '{}' 未注册", server_name))?;
        let tools = McpClient::list_tools(&config).await?;
        let mut cache = self.tools_cache.lock().unwrap();
        cache.insert(server_name.to_string(), tools.clone());
        Ok(tools)
    }

    /// 获取缓存的工具列表（不发送网络请求）
    pub fn get_cached_tools(&self, server_name: &str) -> Vec<McpTool> {
        let cache = self.tools_cache.lock().unwrap();
        cache.get(server_name).cloned().unwrap_or_default()
    }

    /// 从所有已注册且已缓存的服务器生成 AgentTool 适配器列表
    /// 每个适配器可以注册到 ToolRegistry 中，供 LLM 调用
    pub fn get_adapter_boxes(&self) -> Vec<Box<dyn AgentTool>> {
        let reg = self.servers.lock().unwrap();
        let cache = self.tools_cache.lock().unwrap();
        let mut adapters: Vec<Box<dyn AgentTool>> = Vec::new();
        for (server_name, _config) in reg.iter() {
            if let Some(tools) = cache.get(server_name) {
                for tool in tools {
                    adapters.push(Box::new(McpToolAdapter {
                        server_name: server_name.clone(),
                        tool_name: tool.name.clone(),
                        tool_description: tool.description.clone(),
                        bridge: self.servers.clone(),
                    }));
                }
            }
        }
        adapters
    }
}

pub struct McpToolAdapter {
    server_name: String,
    tool_name: String,
    tool_description: String,
    bridge: Arc<Mutex<McpRegistry>>,
}

#[async_trait]
impl AgentTool for McpToolAdapter {
    fn name(&self) -> &str { &self.tool_name }

    fn description(&self) -> &str { &self.tool_description }

    async fn execute(&self, ctx: &ToolContext) -> Result<(String, usize), String> {
        let config = {
            let reg = self.bridge.lock().unwrap();
            reg.get(&self.server_name).cloned()
                .ok_or_else(|| format!("MCP 服务器 '{}' 已断开", self.server_name))?
        };

        let args = serde_json::json!({
            "query": if ctx.input_template.is_empty() { ctx.description.clone() } else { ctx.input_template.clone() }
        });

        let result = McpClient::call_tool(&config, &self.tool_name, args).await?;
        let tokens = result.len() / 2;
        Ok((result, tokens))
    }
}
