use async_trait::async_trait;
use langchainrust::{Tool, ToolError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WeatherInput {
    /// 城市名，如 "北京"、"London"
    pub city: String,
}

#[derive(Debug, Serialize)]
pub struct WeatherOutput {
    pub city: String,
    pub weather: String,
    pub raw: String,
}

pub struct WeatherTool;

#[async_trait]
impl Tool for WeatherTool {
    type Input = WeatherInput;
    type Output = WeatherOutput;

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output, ToolError> {
        let url = format!("https://wttr.in/{}?format=%C+|+%t+|+%h+|+%w&lang=zh", urlencoding(&input.city));
        let resp = reqwest::get(&url)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("请求失败: {}", e)))?;
        let text = resp.text().await
            .map_err(|e| ToolError::ExecutionFailed(format!("读取失败: {}", e)))?;
        Ok(WeatherOutput {
            city: input.city,
            weather: text.clone(),
            raw: text,
        })
    }
}

fn urlencoding(s: &str) -> String {
    s.chars().map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
        _ => format!("%{:02X}", c as u8),
    }).collect()
}
