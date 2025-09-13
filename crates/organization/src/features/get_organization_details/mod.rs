//! Get Organization Details Feature
//!
//! This module implements the functionality to retrieve organization details
//! following Vertical Slice Architecture principles with segregated interfaces.

pub mod dto;
pub mod ports;
pub mod error;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Re-export commonly used types for easier access
pub use dto::*;
pub use error::*;
pub use ports::*;
pub use api::*;
pub use di::*;

use std::sync::Arc;

/// Feature initialization and configuration
pub struct GetOrganizationDetailsFeature {
    pub di_container: Arc<GetOrganizationDetailsDIContainer>,
}

impl GetOrganizationDetailsFeature {
    /// Create a new feature with dependency injection container
    pub fn new(di_container: Arc<GetOrganizationDetailsDIContainer>) -> Self {
        Self { di_container }
    }
    
    /// Initialize the feature
    pub async fn initialize(&self) -> Result<(), OrganizationError> {
        tracing::info!("Initializing Get Organization Details feature");
        
        // Perform any necessary initialization
        // For now, this is a placeholder
        
        tracing::info!("Get Organization Details feature initialized successfully");
        Ok(())
    }
    
    /// Get the API handler
    pub fn api(&self) -> Arc<GetOrganizationDetailsApi> {
        self.di_container.api()
    }
    
    /// Get feature health status
    pub async fn health_check(&self) -> FeatureHealthStatus {
        let health = self.di_container.repository().health_check().await;
        
        FeatureHealthStatus {
            feature_name: "get_organization_details".to_string(),
            is_healthy: health.is_healthy(),
            components: vec![("repository".to_string(), health.status())],
            last_check: chrono::Utc::now(),
            message: health.message(),
        }
    }
}

/// Feature health status
#[derive(Debug, Clone)]
pub struct FeatureHealthStatus {
    pub feature_name: String,
    pub is_healthy: bool,
    pub components: Vec<(String, shared::lifecycle::HealthStatus)>,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub message: String,
}

/// Create a default feature for testing
#[cfg(test)]
pub fn create_test_feature() -> GetOrganizationDetailsFeature {
    let di_container = Arc::new(GetOrganizationDetailsDIContainer::for_testing());
    GetOrganizationDetailsFeature::new(di_container)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_feature_initialization() {
        let feature = create_test_feature();
        
        let result = feature.initialize().await;
        assert!(result.is_ok());
        
        let health = feature.health_check().await;
        assert!(health.is_healthy);
    }
    
    #[tokio::test]
    async fn test_feature_structure() {
        let feature = create_test_feature();
        
        // Test that we can access the API
        let api = feature.api();
        assert!(api.is_some());
        
        // Test feature name
        let health = feature.health_check().await;
        assert_eq!(health.feature_name, "get_organization_details");
    }
}