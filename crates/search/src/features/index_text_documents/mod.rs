//! Index Text Documents Feature
//!
//! This feature provides comprehensive document indexing capabilities following
//! Vertical Slice Architecture (VSA) principles with segregated interfaces.

pub mod dto;
pub mod ports;
pub mod error;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Re-export commonly used types and structures
pub use dto::*;
pub use error::*;
pub use api::*;
pub use di::*;

// Re-export use cases
pub use use_case::IndexDocumentUseCase;

// Re-export DI container and configuration
pub use di::{IndexTextDocumentsDIContainer, IndexTextDocumentsDIContainerBuilder, IndexTextDocumentsConfig};

// Re-export ports
pub use ports::*;

// Feature initialization function
pub fn initialize_feature() -> Result<IndexTextDocumentsDIContainer, IndexDocumentError> {
    tracing::info!("Initializing Index Text Documents feature");
    
    // Use in-memory index by default
    let container = IndexTextDocumentsDIContainer::for_production_with_memory_index()?;
    
    tracing::info!("Index Text Documents feature initialized successfully");
    Ok(container)
}

// Feature initialization with configuration
pub fn initialize_feature_with_config(
    config: IndexTextDocumentsConfig,
) -> Result<IndexTextDocumentsDIContainer, IndexDocumentError> {
    tracing::info!("Initializing Index Text Documents feature with configuration");
    
    let container = config.create_container()?;
    
    tracing::info!("Index Text Documents feature initialized successfully");
    Ok(container)
}

// Health check for the feature
pub async fn health_check(
    container: &IndexTextDocumentsDIContainer,
) -> Result<IndexHealth, IndexDocumentError> {
    tracing::debug!("Performing health check for Index Text Documents feature");
    
    let health = container.health_monitor().check_index_health().await?;
    
    tracing::debug!("Health check completed: {:?}", health.status);
    Ok(health)
}

// Feature statistics
pub async fn get_feature_stats(
    container: &IndexTextDocumentsDIContainer,
) -> Result<IndexStats, IndexDocumentError> {
    tracing::debug!("Getting statistics for Index Text Documents feature");
    
    let stats = container.health_monitor().get_index_stats().await?;
    
    tracing::debug!("Statistics retrieved: {} documents", stats.total_documents);
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_initialize_feature() {
        let result = initialize_feature();
        assert!(result.is_ok());
        
        let container = result.unwrap();
        assert!(container.document_use_case().is_ready());
        assert!(container.batch_use_case().is_ready());
    }
    
    #[test]
    fn test_initialize_feature_with_config() {
        let config = IndexTextDocumentsConfig::testing();
        let result = initialize_feature_with_config(config);
        assert!(result.is_ok());
        
        let container = result.unwrap();
        assert!(container.document_use_case().is_ready());
        assert!(container.batch_use_case().is_ready());
    }
    
    #[tokio::test]
    async fn test_health_check() {
        let container = IndexTextDocumentsDIContainer::for_testing();
        let result = health_check(&container).await;
        assert!(result.is_ok());
    }
    
    #[tokio::test]
    async fn test_get_feature_stats() {
        let container = IndexTextDocumentsDIContainer::for_testing();
        let result = get_feature_stats(&container).await;
        assert!(result.is_ok());
    }
}