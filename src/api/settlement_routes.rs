use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;

use crate::services::settlement::{
    self, CompletenessBody, EvaluateBody, FromAllocationBody, PlanBody, SettlementConfigBody,
};
use crate::services::ServiceError;
use crate::settlement::{CompletenessReport, SettlementResult};

use super::{ApiError, ApiState};

pub fn routes() -> Router<Arc<ApiState>> {
    Router::new()
        .route(
            "/api/settlement/config",
            get(get_settlement_config).put(put_settlement_config),
        )
        .route("/api/settlement/evaluate", post(evaluate_order))
        .route("/api/settlement/completeness", post(completeness_check))
        .route("/api/settlement/plan", post(plan_order))
        .route("/api/settlement/from-allocation", post(from_allocation))
}

async fn get_settlement_config(State(state): State<Arc<ApiState>>) -> Json<Value> {
    Json(settlement::get_settlement_config(&state).await)
}

async fn put_settlement_config(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<SettlementConfigBody>,
) -> Result<Json<Value>, ApiError> {
    let value = settlement::put_settlement_config(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

async fn evaluate_order(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<EvaluateBody>,
) -> Result<Json<SettlementResult>, ApiError> {
    let result = settlement::evaluate_order(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(result))
}

async fn completeness_check(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<CompletenessBody>,
) -> Result<Json<CompletenessReport>, ApiError> {
    let report = settlement::completeness_check(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(report))
}

async fn plan_order(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<PlanBody>,
) -> Result<Json<Value>, ApiError> {
    let value = settlement::plan_order(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}

async fn from_allocation(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<FromAllocationBody>,
) -> Result<Json<Value>, ApiError> {
    let value = settlement::from_allocation(&state, body)
        .await
        .map_err(ServiceError::into_api)?;
    Ok(Json(value))
}
