
// crates/repository/src/features/create_repository/di.rs

use std::sync::Arc;
use mongodb::Database;

use crate::infrastructure::mongodb_adapter::MongoDbRepositoryAdapter;
use super::ports::{
    OrganizationExistsPort, RepositoryExistsPort, RepositoryCreatorPort,
    StorageBackendExistsPort, EventPublisherPort
};
use super::use_case::CreateRepositoryUseCase;
use super::api::CreateRepositoryEndpoint;

// EventPublisherAdapter will be defined elsewhere or mocked
pub struct EventPublisherAdapter;

impl EventPublisherAdapter {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl EventPublisherPort for EventPublisherAdapter {
    async fn publish_repository_created(&self, repository: &crate::domain::repository::Repository) -> crate::domain::RepositoryResult<()> {
        tracing::info!("Mock event published");
        Ok(())
    }
}


/// Contenedor de inyección de dependencias para la feature create_repository
pub struct CreateRepositoryDIContainer {
    pub endpoint: CreateRepositoryEndpoint,
}

impl CreateRepositoryDIContainer {
    /// Constructor flexible que acepta cualquier implementación de los ports
    pub fn new(
        db: Database,
        event_publisher_port: Arc<dyn EventPublisherPort>,
    ) -> Self {
        let mongo_adapter = Arc::new(MongoDbRepositoryAdapter::new(db));
        
        let use_case = Arc::new(CreateRepositoryUseCase::new(
            mongo_adapter.clone(),
            mongo_adapter.clone(),
            mongo_adapter.clone(),
            mongo_adapter,
            event_publisher_port,
        ));
        
        let endpoint = CreateRepositoryEndpoint::new(use_case);
        
        Self { endpoint }
    }

    /// Constructor para producción con implementaciones MongoDB
    pub fn for_production(db: Database) -> Self {
        let event_publisher_port: Arc<dyn EventPublisherPort> = 
            Arc::new(EventPublisherAdapter::new());

        Self::new(db, event_publisher_port)
    }

    /// Constructor para testing con mocks
    #[cfg(test)]
    pub async fn for_testing() -> (Self, Arc<test_adapter::MockEventPublisherPort>) {
        let db = crate::infrastructure::tests::setup_test_database().await.unwrap();
        let event_publisher_port = Arc::new(test_adapter::MockEventPublisherPort::new());

        let container = Self::new(db, event_publisher_port.clone());
        (container, event_publisher_port)
    }
}

/// Adaptadores de prueba para testing
#[cfg(test)]
pub mod test_adapter {
    use std::sync::Mutex;
    use async_trait::async_trait;
    use crate::domain::RepositoryResult;
    use crate::domain::events::RepositoryEvent;
    use super::super::ports::*;

    /// Mock para EventPublisherPort
    pub struct MockEventPublisherPort {
        published_events: Mutex<Vec<String>>,
    }

    impl MockEventPublisherPort {
        pub fn new() -> Self {
            Self {
                published_events: Mutex::new(Vec::new()),
            }
        }
        pub fn get_published_events(&self) -> Vec<String> {
            self.published_events.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl EventPublisherPort for MockEventPublisherPort {
        async fn publish_repository_created(&self, repository: &crate::domain::repository::Repository) -> RepositoryResult<()> {
            if let crate::domain::events::RepositoryEvent::RepositoryCreated(e) = repository {
                self.published_events.lock().unwrap().push(e.hrn.to_string());
            }
            Ok(())
        }
    }
}
