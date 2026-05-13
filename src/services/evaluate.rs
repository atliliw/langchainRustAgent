use crate::config::Config;
use langchainrust::language_models::OpenAIChat;
use langchainrust::schema::Message;
use langchainrust::core::runnables::Runnable;

pub struct EvaluateEngine;

impl EvaluateEngine {
    pub async fn faithfulness(question: &str, answer: &str, context: &str, config: &Config) -> f64 {
        let prompt = format!(
            r#"你是一个评估专家。请判断回答是否基于给定的上下文，而不是凭空编造。

上下文：
{context}

问题：{question}
回答：{answer}

请给出 Faithfulness 评分（0-1 之间，1 表示完全基于上下文）：
只返回一个 0-1 之间的数字。"#,
            context = context, question = question, answer = answer
        );
        Self::score(prompt, config).await
    }

    pub async fn answer_relevancy(question: &str, answer: &str, config: &Config) -> f64 {
        let prompt = format!(
            r#"你是一个评估专家。请判断回答是否与问题相关。

问题：{question}
回答：{answer}

请给出 Answer Relevancy 评分（0-1 之间，1 表示完全相关）：
只返回一个 0-1 之间的数字。"#,
            question = question, answer = answer
        );
        Self::score(prompt, config).await
    }

    pub async fn context_precision(question: &str, context: &str, config: &Config) -> f64 {
        let prompt = format!(
            r#"你是一个评估专家。请判断给定的上下文是否包含回答问题的必要信息。

问题：{question}
上下文：{context}

请给出 Context Precision 评分（0-1 之间，1 表示上下文完全足够）：
只返回一个 0-1 之间的数字。"#,
            question = question, context = context
        );
        Self::score(prompt, config).await
    }

    pub async fn hallucination(question: &str, answer: &str, context: &str, config: &Config) -> f64 {
        let prompt = format!(
            r#"你是一个评估专家。请判断回答中是否存在幻觉（即包含上下文没有的信息）。

上下文：
{context}

问题：{question}
回答：{answer}

请给出 Hallucination 评分（0-1 之间，1 表示完全没有幻觉）：
只返回一个 0-1 之间的数字。"#,
            context = context, question = question, answer = answer
        );
        Self::score(prompt, config).await
    }

    async fn score(prompt: String, config: &Config) -> f64 {
        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(50));
        match llm.invoke(vec![Message::human(&prompt)], None).await {
            Ok(resp) => {
                let cleaned = resp.content.trim();
                cleaned.parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0)
            }
            Err(_) => 0.5,
        }
    }

    pub async fn full_evaluation(
        question: &str,
        answer: &str,
        context: &str,
        config: &Config,
    ) -> serde_json::Value {
        let (f, ar, cp, h) = tokio::join!(
            Self::faithfulness(question, answer, context, config),
            Self::answer_relevancy(question, answer, config),
            Self::context_precision(question, context, config),
            Self::hallucination(question, answer, context, config),
        );
        let avg = (f + ar + cp + h) / 4.0;
        serde_json::json!({
            "faithfulness": (f * 100.0).round() / 100.0,
            "answer_relevancy": (ar * 100.0).round() / 100.0,
            "context_precision": (cp * 100.0).round() / 100.0,
            "hallucination_score": (h * 100.0).round() / 100.0,
            "average": (avg * 100.0).round() / 100.0,
        })
    }

    pub async fn compare_evaluation(
        base_answer: &str,
        new_answer: &str,
        question: &str,
        config: &Config,
    ) -> serde_json::Value {
        let prompt = format!(
            r#"你是一个评估专家。比较以下两个回答，判断哪个更好。

问题：{question}

回答 A（基准）：{base}
回答 B（新）：{new}

请从以下维度给两个回答分别打分（0-10）：
- 准确性：信息是否正确
- 完整性：是否全面
- 清晰度：是否易懂

返回 JSON：{{"a": {{"accuracy": N, "completeness": N, "clarity": N}}, "b": {{...}}}}"#,
            question = question, base = base_answer, new = new_answer
        );

        let llm = OpenAIChat::new(config.to_langchain_openai_config().with_max_tokens(256));
        match llm.invoke(vec![Message::human(&prompt)], None).await {
            Ok(resp) => {
                let cleaned = resp.content.trim_start_matches("```json")
                    .trim_start_matches("```").trim_end_matches("```").trim();
                serde_json::from_str(cleaned).unwrap_or(serde_json::json!({"error": "parse failed"}))
            }
            Err(_) => serde_json::json!({"error": "eval failed"}),
        }
    }
}
