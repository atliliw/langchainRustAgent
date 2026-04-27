//! 测试处理函数

use crate::handlers::{AppState, ApiErrorResponse};
use crate::models::*;
use crate::utils::SearchTester;
use axum::{
    extract::{State, Query},
    Json,
};
use std::sync::Arc;

pub async fn run_precision_test(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PrecisionTestQuery>,
    custom_cases_opt: Option<Json<Vec<TestCase>>>,
) -> Result<Json<PrecisionReport>, ApiErrorResponse> {
    let test_cases = if query.custom_cases {
        custom_cases_opt.map(|j| j.0).unwrap_or_default()
    } else {
        SearchTester::get_default_test_cases()
    };
    
    let tester = SearchTester::new(state.api.vector_store.clone(), state.config.clone());
    let report = tester.run_precision_test(test_cases).await?;
    
    Ok(Json(report))
}

pub async fn get_test_cases() -> Json<Vec<TestCase>> {
    Json(SearchTester::get_default_test_cases())
}