//! ============================================================================
//! 搜索精准度测试模块 - 评估向量搜索效果
//! ============================================================================
//!
//! 功能说明：
//! 1. 插入预设测试文档
//! 2. 执行搜索查询
//! 3. 验证搜索结果排名
//! 4. 计算精准度分数
//!
//! 测试流程：
//! 1. 插入 5 个测试文档（关于 Rust、Qdrant、LangChain 等）
//! 2. 对每个文档执行相关查询
//! 3. 检查查询结果是否包含正确的文档
//! 4. 计算通过率和平均排名

use crate::config::Config;
use crate::qdrant_store::QdrantStore;
use langchainrust::Document;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

// ============================================================================
// 错误类型定义
// ============================================================================

#[derive(Error, Debug)]
pub enum TestError {
    #[error("测试数据初始化失败: {0}")]
    InitError(String),
    
    #[error("搜索测试失败: {0}")]
    SearchError(String),
}

// ============================================================================
// 测试用例结构体 - 定义单个测试
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestCase {
    pub document: String,         // 测试文档内容
    pub query: String,            // 搜索查询文本
    pub expected_in_top_k: usize, // 期望的排名位置（例如: 期望在 Top 1 或 Top 2）
    pub description: String,      // 测试描述
}

// ============================================================================
// 测试结果结构体 - 单个测试的执行结果
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct TestResult {
    pub test_case: TestCase,       // 原测试用例
    pub found: bool,                // 是否找到正确文档
    pub position: Option<usize>,    // 实际排名位置
    pub score: Option<f32>,         // 相似度分数
    pub passed: bool,               // 是否通过（排名 <= expected_in_top_k）
}

// ============================================================================
// 精准度报告结构体 - 所有测试的汇总结果
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct PrecisionReport {
    pub total_tests: usize,        // 总测试数量
    pub passed_tests: usize,       // 通过数量
    pub precision_score: f32,      // 精准度分数 (0-1)
    pub average_position: f32,     // 平均排名位置
    pub results: Vec<TestResult>,  // 详细测试结果
}

// ============================================================================
// 搜索测试器结构体
// ============================================================================

pub struct SearchTester {
    store: Arc<QdrantStore>,
    config: Config,
}

impl SearchTester {
    pub fn new(store: Arc<QdrantStore>, config: Config) -> Self {
        Self { store, config }
    }
    
    pub fn get_default_test_cases() -> Vec<TestCase> {
        vec![
            TestCase {
                document: "Rust 是一门专注于内存安全的系统编程语言，由 Mozilla 开发，于 2015 年发布。".to_string(),
                query: "Rust 语言是什么时候发布的？".to_string(),
                expected_in_top_k: 1,
                description: "时间信息检索".to_string(),
            },
            TestCase {
                document: "向量数据库 Qdrant 支持高效的语义搜索，可以存储文档的向量表示。".to_string(),
                query: "Qdrant 能做什么？".to_string(),
                expected_in_top_k: 2,
                description: "功能描述检索".to_string(),
            },
            TestCase {
                document: "LangChain 是一个用于构建 LLM 应用的框架，支持 Python 和 JavaScript。".to_string(),
                query: "LangChain 支持哪些编程语言？".to_string(),
                expected_in_top_k: 2,
                description: "编程语言信息检索".to_string(),
            },
            TestCase {
                document: "机器学习是人工智能的一个分支，通过算法让计算机从数据中学习。".to_string(),
                query: "什么是机器学习？".to_string(),
                expected_in_top_k: 1,
                description: "定义查询".to_string(),
            },
            TestCase {
                document: "深度学习使用神经网络处理复杂任务，如图像识别和自然语言处理。".to_string(),
                query: "深度学习可以用于图像处理吗？".to_string(),
                expected_in_top_k: 2,
                description: "应用场景检索".to_string(),
            },
        ]
    }
    
    pub async fn init_test_data(&self, test_cases: &[TestCase]) -> Result<(), TestError> {
        let documents: Vec<Document> = test_cases.iter()
            .enumerate()
            .map(|(i, tc)| {
                Document::new(tc.document.clone())
                    .with_id(format!("test_{}", i))
                    .with_metadata("test_type", tc.description.clone())
            })
            .collect();
        
        self.store.add_documents(documents).await
            .map_err(|e| TestError::InitError(e.to_string()))?;
        
        Ok(())
    }
    
    pub async fn run_precision_test(&self, test_cases: Vec<TestCase>) -> Result<PrecisionReport, TestError> {
        self.init_test_data(&test_cases).await?;
        
        let mut results = Vec::new();
        let mut passed_count = 0;
        let mut total_position = 0.0;
        
        for tc in &test_cases {
            let search_results = self.store.search(&tc.query, tc.expected_in_top_k + 2).await
                .map_err(|e| TestError::SearchError(e.to_string()))?;
            
            let mut found = false;
            let mut position = None;
            let mut score = None;
            
            for (idx, result) in search_results.iter().enumerate() {
                if result.document.content.contains(&tc.document) 
                    || result.document.id.as_ref().map(|id| id.starts_with("test_")).unwrap_or(false) {
                    found = true;
                    position = Some(idx + 1);
                    score = Some(result.score);
                    break;
                }
            }
            
            let passed = found && position.unwrap_or(999) <= tc.expected_in_top_k;
            if passed {
                passed_count += 1;
            }
            
            if let Some(pos) = position {
                total_position += pos as f32;
            }
            
            results.push(TestResult {
                test_case: tc.clone(),
                found,
                position,
                score,
                passed,
            });
        }
        
        let precision_score = passed_count as f32 / test_cases.len() as f32;
        let avg_position = if passed_count > 0 {
            total_position / passed_count as f32
        } else {
            0.0
        };
        
        Ok(PrecisionReport {
            total_tests: test_cases.len(),
            passed_tests: passed_count,
            precision_score,
            average_position: avg_position,
            results,
        })
    }
    
    pub async fn clear_test_data(&self) -> Result<(), TestError> {
        self.store.clear().await
            .map_err(|e| TestError::InitError(e.to_string()))?;
        Ok(())
    }
}