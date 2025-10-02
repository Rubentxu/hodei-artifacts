use crate::{app_state::AppState, error::Result};
use axum::{extract::State, response::Json};
use serde_json::{Value, json};
use std::sync::Arc;

pub async fn health(State(state): State<Arc<AppState>>) -> Result<Json<Value>> {
    let health = state.health.read().await;

    let response = json!({
        "status": if health.is_healthy() { "healthy" } else { "unhealthy" },
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "uptime_seconds": (chrono::Utc::now() - health.startup_time).num_seconds(),
        "components": {
            "database": component_health_to_json(&health.database),
            "policy_engine": component_health_to_json(&health.policy_engine)
        },
        "version": env!("CARGO_PKG_VERSION"),
    });

    Ok(Json(response))
}

pub async fn readiness(State(state): State<Arc<AppState>>) -> Result<Json<Value>> {
    let health = state.health.read().await;

    let response = json!({
        "status": if health.is_healthy() { "ready" } else { "not_ready" },
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "checks": {
            "database": health.database,
            "policy_engine": health.policy_engine,
        }
    });

    Ok(Json(response))
}

fn component_health_to_json(health: &crate::app_state::ComponentHealth) -> Value {
    match health {
        crate::app_state::ComponentHealth::Healthy => json!({
            "status": "healthy"
        }),
        crate::app_state::ComponentHealth::Degraded { reason } => json!({
            "status": "degraded",
            "reason": reason
        }),
        crate::app_state::ComponentHealth::Unhealthy { reason } => json!({
            "status": "unhealthy",
            "reason": reason
        }),
    }
}
