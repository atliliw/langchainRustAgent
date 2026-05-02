//! 检索精准度测试
//!
//! 用来测试向量检索的效果好不好。
//! 做法：准备几条已知正确答案的问题，跑检索看能不能找到。

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use crate::utils::SearchTester;
use axum::{
    extract::{State, Query},
    Json,
};
use std::sync::Arc;

/// 运行精准度测试
/// POST /api/test/precision
/// 用预设的测试用例（或自定义用例）测试检索准确率
pub async fn run_precision_test(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PrecisionTestQuery>,
    custom_cases_opt: Option<Json<Vec<TestCase>>>,
) -> Result<Json<PrecisionReport>, ApiErrorResponse> {
    // 如果前端要求用自定义用例，就用传进来的；否则用系统默认的
    let test_cases = if query.custom_cases {
        custom_cases_opt.map(|j| j.0).unwrap_or_default()
    } else {
        SearchTester::get_default_test_cases()
    };
    
    let tester = SearchTester::new(state.api.vector_store.clone(), state.config.clone());
    let report = tester.run_precision_test(test_cases).await?;
    
    Ok(Json(report))
}

/// 获取默认测试用例列表
/// GET /api/test/cases
pub async fn get_test_cases() -> Json<Vec<TestCase>> {
    Json(SearchTester::get_default_test_cases())
}
