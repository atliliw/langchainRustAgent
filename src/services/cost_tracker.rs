use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRecord {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub endpoint: String,
    pub timestamp: String,
}

pub struct CostTracker {
    pool: SqlitePool,
}

impl CostTracker {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn ensure_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS cost_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL NOT NULL DEFAULT 0.0,
                endpoint TEXT NOT NULL DEFAULT '',
                timestamp TEXT NOT NULL DEFAULT (datetime('now'))
            )"
        ).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn record(&self, model: &str, input_tokens: u64, output_tokens: u64, endpoint: &str) -> Result<(), sqlx::Error> {
        let total = input_tokens + output_tokens;
        let cost = Self::calculate_cost(model, input_tokens, output_tokens);
        sqlx::query(
            "INSERT INTO cost_records (model, input_tokens, output_tokens, total_tokens, cost_usd, endpoint) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(model)
        .bind(input_tokens as i64)
        .bind(output_tokens as i64)
        .bind(total as i64)
        .bind(cost)
        .bind(endpoint)
        .execute(&self.pool).await?;
        Ok(())
    }

    pub fn calculate_cost(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
        let (input_price, output_price) = match model {
            m if m.contains("gpt-4o") => (0.0025, 0.01),
            m if m.contains("gpt-4") => (0.03, 0.06),
            m if m.contains("gpt-3.5") => (0.0005, 0.0015),
            m if m.contains("claude-3-opus") => (0.015, 0.075),
            m if m.contains("claude-3-sonnet") => (0.003, 0.015),
            m if m.contains("claude-3-haiku") => (0.00025, 0.00125),
            m if m.contains("qwen") || m.contains("deepseek") => (0.0005, 0.002),
            _ => (0.001, 0.002),
        };
        (input_tokens as f64 / 1000.0 * input_price) + (output_tokens as f64 / 1000.0 * output_price)
    }

    pub async fn get_stats(&self) -> Result<serde_json::Value, sqlx::Error> {
        let total = sqlx::query_as::<_, (Option<i64>, Option<f64>)>(
            "SELECT SUM(total_tokens), SUM(cost_usd) FROM cost_records"
        ).fetch_one(&self.pool).await?;

        let today = sqlx::query_as::<_, (Option<i64>, Option<f64>)>(
            "SELECT SUM(total_tokens), SUM(cost_usd) FROM cost_records WHERE timestamp >= date('now')"
        ).fetch_one(&self.pool).await?;

        let by_model = sqlx::query_as::<_, (String, Option<i64>, Option<f64>)>(
            "SELECT model, SUM(total_tokens), SUM(cost_usd) FROM cost_records GROUP BY model ORDER BY SUM(cost_usd) DESC"
        ).fetch_all(&self.pool).await?;

        let by_endpoint = sqlx::query_as::<_, (String, Option<i64>)>(
            "SELECT endpoint, COUNT(*) as cnt FROM cost_records GROUP BY endpoint ORDER BY cnt DESC"
        ).fetch_all(&self.pool).await?;

        Ok(serde_json::json!({
            "total_tokens": total.0.unwrap_or(0),
            "total_cost": total.1.unwrap_or(0.0),
            "today_tokens": today.0.unwrap_or(0),
            "today_cost": today.1.unwrap_or(0.0),
            "by_model": by_model.iter().map(|(m, t, c)| serde_json::json!({
                "model": m, "tokens": t.unwrap_or(0), "cost": c.unwrap_or(0.0)
            })).collect::<Vec<_>>(),
            "by_endpoint": by_endpoint.iter().map(|(e, c)| serde_json::json!({
                "endpoint": e, "count": c.unwrap_or(0)
            })).collect::<Vec<_>>(),
        }))
    }
}
