use crate::config::Config;
use langchainrust::language_models::OpenAIChat;
use langchainrust::schema::Message;
use langchainrust::core::runnables::Runnable;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct VisionRequest {
    pub image_url: String,
    pub question: String,
    pub model: Option<String>,
}

pub struct VisionService;

impl VisionService {
    pub async fn analyze(request: VisionRequest, config: &Config) -> Result<String, String> {
        let model = request.model.unwrap_or_else(|| "gpt-4o".to_string());

        let mut llm_config = config.to_langchain_openai_config()
            .with_max_tokens(2048);
        llm_config.model = model;

        let llm = OpenAIChat::new(llm_config);

        let prompt = format!(
            r#"请分析这张图片并回答以下问题：

问题：{question}

请给出详细的分析结果。"#,
            question = request.question
        );

        let messages = vec![
            Message::human(&prompt),
        ];

        match llm.invoke(messages, None).await {
            Ok(resp) => Ok(resp.content),
            Err(e) => Err(format!("Vision 分析失败: {}", e)),
        }
    }
}
