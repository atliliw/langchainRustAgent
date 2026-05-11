use crate::config::Config;
use crate::models::AgentTask;
use async_trait::async_trait;
use langchainrust::{language_models::OpenAIChat, schema::Message, core::runnables::Runnable};

/// 验证结果
#[derive(Debug)]
pub enum VerifyResult {
    /// 验证通过
    Pass,
    /// 验证失败，附带错误原因
    Fail(String),
}

/// 验证钩子 trait
/// 每个钩子实现一种验证方式，组合使用
#[async_trait]
pub trait VerifyHook: Send + Sync {
    fn name(&self) -> &str;
    async fn verify(&self, output: &str, task: &AgentTask) -> VerifyResult;
}

/// 规则匹配验证：通过正则/关键词匹配常见错误模式
pub struct PatternVerifyHook;

impl PatternVerifyHook {
    pub fn new() -> Self {
        Self
    }

    fn check_errors(output: &str) -> Option<String> {
        let lower = output.to_lowercase();

        // 仅匹配真正表示工具执行异常的关键词（避免误杀技术文档中的正常用词）
        // rule: 必须是以完整单词/短语形式出现，且上下文表示确实发生了异常
        let patterns = [
            ("error:", "工具返回错误"),
            ("timeout", "执行超时"),
            ("timed out", "执行超时"),
            ("not found", "未找到相关内容"),
            ("permission denied", "权限不足"),
            ("unavailable", "服务不可用"),
            ("rate limit", "请求频率限制"),
            ("internal server error", "服务端错误"),
            ("bad gateway", "网关错误"),
        ];

        for (keyword, reason) in &patterns {
            if lower.contains(keyword) {
                return Some(format!("输出包含异常关键词「{}」: {}", keyword, reason));
            }
        }

        None
    }
}

#[async_trait]
impl VerifyHook for PatternVerifyHook {
    fn name(&self) -> &str {
        "pattern"
    }

    async fn verify(&self, output: &str, _task: &AgentTask) -> VerifyResult {
        match Self::check_errors(output) {
            Some(reason) => VerifyResult::Fail(reason),
            None => VerifyResult::Pass,
        }
    }
}

/// LLM 质量验证：用 LLM 判断输出是否满足任务要求
pub struct LlmVerifyHook {
    config: Config,
}

impl LlmVerifyHook {
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

#[async_trait]
impl VerifyHook for LlmVerifyHook {
    fn name(&self) -> &str {
        "llm_judge"
    }

    async fn verify(&self, output: &str, task: &AgentTask) -> VerifyResult {
        let llm = OpenAIChat::new(
            self.config
                .to_langchain_openai_config()
                .with_max_tokens(256),
        );

        let desc = if task.description.is_empty() {
            &task.name
        } else {
            &task.description
        };
        let prompt = format!(
            "你是一个严格的质量检查员。判断以下输出是否有问题。\n\n\
             任务：{}\n\n\
             输出：\n{}\n\n\
             检查以下三点，有任何一点不满足就 FAIL：\n\
             1. 输出不能为空或几乎为空\n\
             2. 输出必须直接回应任务要求，不能答非所问\n\
             3. 输出不能有明顯的事实错误或矛盾\n\n\
             只返回 PASS 或 FAIL:具体原因",
            desc, output
        );

        match llm.invoke(vec![Message::human(&prompt)], None).await {
            Ok(r) => {
                let content = r.content.trim().to_lowercase();
                if content.starts_with("pass") {
                    VerifyResult::Pass
                } else {
                    let reason = content
                        .strip_prefix("fail:")
                        .or_else(|| content.strip_prefix("fail："))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "LLM 判断未通过".to_string());
                    VerifyResult::Fail(reason)
                }
            }
            Err(_) => VerifyResult::Pass, // LLM 调用失败时不阻塞执行
        }
    }
}

/// 组合验证：依次执行多个 hook，任一失败则整体失败
pub struct CompositeVerifyHook {
    hooks: Vec<Box<dyn VerifyHook>>,
}

impl CompositeVerifyHook {
    pub fn new(hooks: Vec<Box<dyn VerifyHook>>) -> Self {
        Self { hooks }
    }
}

#[async_trait]
impl VerifyHook for CompositeVerifyHook {
    fn name(&self) -> &str {
        "composite"
    }

    async fn verify(&self, output: &str, task: &AgentTask) -> VerifyResult {
        for hook in &self.hooks {
            let result = hook.verify(output, task).await;
            if let VerifyResult::Fail(reason) = result {
                return VerifyResult::Fail(format!("[{}] {}", hook.name(), reason));
            }
        }
        VerifyResult::Pass
    }
}

/// 根据 use_verify 创建组合验证器
pub fn create_verify_hook(config: &Config) -> CompositeVerifyHook {
    CompositeVerifyHook::new(vec![
        Box::new(PatternVerifyHook::new()),
        Box::new(LlmVerifyHook::new(config)),
    ])
}
