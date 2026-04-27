use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestCase {
    pub document: String,
    pub query: String,
    pub expected_in_top_k: usize,
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestResult {
    pub test_case: TestCase,
    pub found: bool,
    pub position: Option<usize>,
    pub score: Option<f32>,
    pub passed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PrecisionReport {
    pub total_tests: usize,
    pub passed_tests: usize,
    pub precision_score: f32,
    pub average_position: f32,
    pub results: Vec<TestResult>,
}

#[derive(Deserialize)]
pub struct PrecisionTestQuery {
    #[serde(default)]
    pub custom_cases: bool,
}
