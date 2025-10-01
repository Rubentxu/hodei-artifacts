use crate::{app_state::AppState, error::{AppError, Result}};
use axum::{extract::State, response::Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct AuthorizationRequest {
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub context: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct AuthorizationResponse {
    pub decision: String,
    pub reasons: Vec<String>,
    pub request_id: String,
    pub timestamp: String,
}

pub async fn authorize(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AuthorizationRequest>,
) -> Result<Json<AuthorizationResponse>> {
    let request_id = uuid::Uuid::new_v4().to_string();
    
    tracing::info!(
        request_id = %request_id,
        principal = %request.principal,
        action = %request.action,
        resource = %request.resource,
        "Processing authorization request"
    );
    
    // Validate request
    if request.principal.is_empty() {
        return Err(AppError::BadRequest("Principal cannot be empty".to_string()));
    }
    
    if request.action.is_empty() {
        return Err(AppError::BadRequest("Action cannot be empty".to_string()));
    }
    
    if request.resource.is_empty() {
        return Err(AppError::BadRequest("Resource cannot be empty".to_string()));
    }
    
    // Process authorization using the policy engine
    let decision = match process_authorization(&state, &request).await {
        Ok(result) => {
            state.metrics.record_authorization(result.decision == "Allow");
            result
        },
        Err(e) => {
            state.metrics.record_authorization(false);
            tracing::error!(
                request_id = %request_id,
                error = %e,
                "Authorization processing failed"
            );
            return Err(e);
        }
    };
    
    tracing::info!(
        request_id = %request_id,
        decision = %decision.decision,
        "Authorization request completed"
    );
    
    Ok(Json(AuthorizationResponse {
        decision: decision.decision,
        reasons: decision.reasons,
        request_id,
        timestamp: chrono::Utc::now().to_rfc3339(),
    }))
}

async fn process_authorization(
    state: &Arc<AppState>,
    request: &AuthorizationRequest,
) -> Result<AuthorizationResult> {
    // This is where you'd integrate with your actual policy engine
    // For now, this is a placeholder implementation
    
    // Example: Check if this is an admin user
    if request.principal == "admin" {
        return Ok(AuthorizationResult {
            decision: "Allow".to_string(),
            reasons: vec!["Admin user has full access".to_string()],
        });
    }
    
    // Example: Deny by default for this demo
    Ok(AuthorizationResult {
        decision: "Deny".to_string(),
        reasons: vec!["Default deny policy applied".to_string()],
    })
}

#[derive(Debug)]
struct AuthorizationResult {
    decision: String,
    reasons: Vec<String>,
}
