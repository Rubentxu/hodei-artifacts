use crate::config::Config;
use metrics::{Counter, Histogram};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Application state - solo contiene use cases de los crates especializados
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub metrics: AppMetrics,
    pub health: Arc<RwLock<HealthStatus>>,
    // Use cases from policies crate
    pub create_policy_uc: Arc<policies::features::create_policy::use_case::CreatePolicyUseCase>,
    pub get_policy_uc: Arc<policies::features::get_policy::use_case::GetPolicyUseCase>,
    pub list_policies_uc: Arc<policies::features::list_policies::use_case::ListPoliciesUseCase>,
    pub delete_policy_uc: Arc<policies::features::delete_policy::use_case::DeletePolicyUseCase>,
    pub update_policy_uc: Arc<policies::features::update_policy::use_case::UpdatePolicyUseCase>,
    pub validate_policy_uc: Arc<policies::features::validate_policy::use_case::ValidatePolicyUseCase>,
    pub policy_playground_uc: Arc<policies::features::policy_playground::use_case::PolicyPlaygroundUseCase>,
    pub analyze_policies_uc: Arc<policies::features::policy_analysis::use_case::AnalyzePoliciesUseCase>,
    pub batch_eval_uc: Arc<policies::features::batch_eval::use_case::BatchEvalUseCase>,
    // Authorization engine from policies crate
    #[allow(dead_code)]
    pub authorization_engine: Arc<policies::shared::AuthorizationEngine>,
}

impl Default for AppMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct AppMetrics {
    pub requests_total: Counter,
    pub authorization_requests: Counter,
    pub authorization_success: Counter,
    pub authorization_failures: Counter,
    pub policy_operations: Counter,
    pub errors_total: Counter,
    pub request_duration: Histogram,
}

impl AppMetrics {
    pub fn new() -> Self {
        Self {
            requests_total: metrics::counter!("http_requests_total"),
            authorization_requests: metrics::counter!("authorization_requests_total"),
            authorization_success: metrics::counter!("authorization_success_total"),
            authorization_failures: metrics::counter!("authorization_failures_total"),
            policy_operations: metrics::counter!("policy_operations_total"),
            errors_total: metrics::counter!("errors_total"),
            request_duration: metrics::histogram!("http_request_duration_seconds"),
        }
    }

    pub fn record_request(&self) {
        self.requests_total.increment(1);
    }

    pub fn record_authorization(&self, success: bool) {
        self.authorization_requests.increment(1);
        if success {
            self.authorization_success.increment(1);
        } else {
            self.authorization_failures.increment(1);
        }
    }

    pub fn record_policy_operation(&self) {
        self.policy_operations.increment(1);
    }

    pub fn record_error(&self, _error_type: &str) {
        self.errors_total.increment(1);
        // You could add labels for error types if your metrics backend supports it
    }

    pub fn record_request_duration(&self, duration: std::time::Duration) {
        self.request_duration.record(duration.as_secs_f64());
    }
}

#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub database: ComponentHealth,
    pub policy_engine: ComponentHealth,
    #[allow(dead_code)]
    pub last_health_check: chrono::DateTime<chrono::Utc>,
    pub startup_time: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[allow(dead_code)]
pub enum ComponentHealth {
    Healthy,
    Degraded { reason: String },
    Unhealthy { reason: String },
}

impl HealthStatus {
    pub fn new() -> Self {
        let now = chrono::Utc::now();
        Self {
            database: ComponentHealth::Healthy,
            policy_engine: ComponentHealth::Healthy,
            startup_time: now,
            last_health_check: now,
        }
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self.database, ComponentHealth::Healthy)
            && matches!(self.policy_engine, ComponentHealth::Healthy)
    }

    #[allow(dead_code)]
    pub fn update_database_health(&mut self, health: ComponentHealth) {
        self.database = health;
        self.last_health_check = chrono::Utc::now();
    }

    #[allow(dead_code)]
    pub fn update_policy_engine_health(&mut self, health: ComponentHealth) {
        self.policy_engine = health;
        self.last_health_check = chrono::Utc::now();
    }
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self::new()
    }
}
