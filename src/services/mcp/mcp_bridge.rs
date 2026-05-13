use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex;
use crate::config::Config;
use crate::services::tools::{AgentTool, ToolContext};
use crate::services::mcp::mcp_client::{McpClient, McpServerConfig, McpTool, McpRegistry};

pub struct McpBridge {
    servers: Arc<Mutex<McpRegistry>>,
}

impl McpBridge {
    pub fn new() -> Self {
        Self { servers: Arc::new(Mutex::new(McpRegistry::new())) }
    }

    pub fn register_server(&self, name: String, config: McpServerConfig) {
        let mut reg = self.servers.lock().unwrap();
        reg.insert(name, config);
    }

    pub fn unregister_server(&self, name: &str) {
        let mut reg = self.servers.lock().unwrap();
        reg.remove(name);
    }

    pub fn list_servers(&self) -> Vec<(String, McpServerConfig)> {
        let reg = self.servers.lock().unwrap();
        reg.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn get_server(&self, name: &str) -> Option<McpServerConfig> {
        let reg = self.servers.lock().unwrap();
        reg.get(name).cloned()
    }

    pub async fn discover_tools(&self, server_name: &str) -> Result<Vec<McpTool>, String> {
        let config = self.get_server(server_name)
            .ok_or_else(|| format!("MCP 服务器 '{}' 未注册", server_name))?;
        McpClient::list_tools(&config).await
    }

    pub fn create_tool_adapter(
        server_name: &str,
        tool: &McpTool,
        bridge: Arc<Mutex<McpRegistry>>,
    ) -> Arc<dyn AgentTool> {
        let server_name = server_name.to_string();
        let tool_name = tool.name.clone();
        let _description = tool.description.clone();
        let bridge = bridge.clone();

        Arc::new(McpToolAdapter {
            server_name,
            tool_name,
            bridge,
        })
    }
}

pub struct McpToolAdapter {
    server_name: String,
    tool_name: String,
    bridge: Arc<Mutex<McpRegistry>>,
}

#[async_trait]
impl AgentTool for McpToolAdapter {
    fn name(&self) -> &str { &self.tool_name }
    fn description(&self) -> &str { &self.tool_name }

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
