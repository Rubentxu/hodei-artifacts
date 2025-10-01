use crate::{app_state::AppState, error::{AppError, Result}};
use axum::{extract::{Path, State}, response::Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub description: Option<String>,
    pub policy_content: String,
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PolicyResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub policy_content: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct PolicyListResponse {
    pub policies: Vec<PolicyResponse>,
    pub total: usize,
}

pub async fn create_policy(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreatePolicyRequest>,
) -> Result<Json<PolicyResponse>> {
    tracing::info!(
        policy_name = %request.name,
        "Creating new policy"
    );
    
    // Validate request
    if request.name.is_empty() {
        return Err(AppError::BadRequest("Policy name cannot be empty".to_string()));
    }
    
    if request.policy_content.is_empty() {
        return Err(AppError::BadRequest("Policy content cannot be empty".to_string()));
    }
    
    // Build command and validate via policies DTO
    let cmd = policies::features::create_policy::dto::CreatePolicyCommand::new(
        request.policy_content.clone(),
    );
    if let Err(e) = cmd.validate() {
        return Err(AppError::Validation(e.to_string()));
    }

    // Execute use case (via DI from AppState or fallback DI builder)
    if let Some(uc) = &state.create_policy_uc {
        uc.execute(&cmd).await.map_err(|e| AppError::BadRequest(e.to_string()))?;
    } else {
        let (uc, _engine) = policies::features::create_policy::di::make_use_case_mem()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        uc.execute(&cmd).await.map_err(|e| AppError::BadRequest(e.to_string()))?;
    }

    // Record metrics
    state.metrics.record_policy_operation();

    // Create response (ID generated here; persistence layer stores the policy text)
    let policy_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    
    let policy = PolicyResponse {
        id: policy_id.clone(),
        name: request.name.clone(),
        description: request.description,
        policy_content: request.policy_content,
        enabled: request.enabled.unwrap_or(true),
        created_at: now.clone(),
        updated_at: now,
    };
    
    tracing::info!(
        policy_id = %policy_id,
        policy_name = %request.name,
        "Policy created successfully"
    );
    
    Ok(Json(policy))
}

pub async fn list_policies(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PolicyListResponse>> {
    tracing::debug!("Listing all policies");
    
    // Placeholder implementation - in reality, you'd fetch from storage
    let policies = vec![
        PolicyResponse {
            id: "sample-policy-1".to_string(),
            name: "Sample Policy".to_string(),
            description: Some("A sample policy for demonstration".to_string()),
            policy_content: "permit(principal, action, resource);".to_string(),
            enabled: true,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        },
    ];
    
    let response = PolicyListResponse {
        total: policies.len(),
        policies,
    };
    
    Ok(Json(response))
}

pub async fn delete_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!(
        policy_id = %policy_id,
        "Deleting policy"
    );
    
    if policy_id.is_empty() {
        return Err(AppError::BadRequest("Policy ID cannot be empty".to_string()));
    }
    
    // Record metrics
    state.metrics.record_policy_operation();
    
    // Placeholder implementation - in reality, you'd delete from storage
    // and handle cases where the policy doesn't exist
    
    tracing::info!(
        policy_id = %policy_id,
        "Policy deleted successfully"
    );
    
    Ok(Json(serde_json::json!({
        "message": "Policy deleted successfully",
        "policy_id": policy_id,
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}
