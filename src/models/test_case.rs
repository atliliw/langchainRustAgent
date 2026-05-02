//! 检索精准度测试用例模型

use serde::{Deserialize, Serialize};

/// 一条测试用例（search_test.rs 中使用）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestCase {
    pub document: String,           // 要插入的测试文档
    pub query: String,              // 搜索词
    pub expected_in_top_k: usize,   // 期望在前几名内找到
    pub description: String,        // 用例描述
}

/// 一条测试结果
#[derive(Debug, Serialize, Deserialize)]
pub struct TestResult {
    pub test_case: TestCase,
    pub found: bool,               // 是否找到
    pub position: Option<usize>,    // 在第几位找到
    pub score: Option<f32>,         // 相似度分数
    pub passed: bool,               // 是否通过
}

/// 精准度测试报告
#[derive(Debug, Serialize, Deserialize)]
pub struct PrecisionReport {
    pub total_tests: usize,
    pub passed_tests: usize,
    pub precision_score: f32,
    pub average_position: f32,
    pub results: Vec<TestResult>,
}

/// 精准度测试查询参数
#[derive(Debug, Deserialize)]
pub struct PrecisionTestQuery {
    #[serde(default)]
    pub custom_cases: bool,  // 是否使用自定义用例
}
