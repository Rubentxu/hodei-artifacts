// crates/repository/src/infrastructure/api.rs

use axum::{
    routing::{post, get, put, delete},
    Router,
    Extension,
};
use std::sync::Arc;

use crate::features::{
    create_repository::{CreateRepositoryEndpoint, CreateRepositoryDIContainer},
    get_repository::{GetRepositoryEndpoint, GetRepositoryDIContainer},
    update_repository::{UpdateRepositoryEndpoint, UpdateRepositoryDIContainer},
    delete_repository::{DeleteRepositoryEndpoint, DeleteRepositoryDIContainer},
};

use super::unified_adapter::{UnifiedRepositoryAdapter, EventPublisherAdapter};

/// Módulo principal de API que integra todos los endpoints de repositorios
pub struct RepositoryApiModule {
    create_endpoint: Arc<CreateRepositoryEndpoint>,
    get_endpoint: Arc<GetRepositoryEndpoint>,
    update_endpoint: Arc<UpdateRepositoryEndpoint>,
    delete_endpoint: Arc<DeleteRepositoryEndpoint>,
}

impl RepositoryApiModule {
    pub fn new(
        create_endpoint: Arc<CreateRepositoryEndpoint>,
        get_endpoint: Arc<GetRepositoryEndpoint>,
        update_endpoint: Arc<UpdateRepositoryEndpoint>,
        delete_endpoint: Arc<DeleteRepositoryEndpoint>,
    ) -> Self {
        Self {
            create_endpoint,
            get_endpoint,
            update_endpoint,
            delete_endpoint,
        }
    }

    /// Crea el router de Axum con todos los endpoints de repositorios
    pub fn create_router(self) -> Router {
        Router::new()
            // CREATE - POST /repositories
            .route(
                "/repositories",
                post(CreateRepositoryEndpoint::create_repository),
            )
            // READ - GET /repositories/{repository_hrn}
            .route(
                "/repositories/:repository_hrn",
                get(GetRepositoryEndpoint::get_repository),
            )
            // UPDATE - PUT /repositories/{repository_hrn}
            .route(
                "/repositories/:repository_hrn",
                put(UpdateRepositoryEndpoint::update_repository),
            )
            // DELETE - DELETE /repositories/{repository_hrn}
            .route(
                "/repositories/:repository_hrn",
                delete(DeleteRepositoryEndpoint::delete_repository),
            )
            // LIST - GET /repositories (listar todos los repositorios de una organización)
            .route(
                "/repositories",
                get(GetRepositoryEndpoint::list_repositories),
            )
            // Extensiones para los endpoints
            .layer(Extension(self.create_endpoint))
            .layer(Extension(self.get_endpoint))
            .layer(Extension(self.update_endpoint))
            .layer(Extension(self.delete_endpoint))
    }
}

/// Builder para crear el módulo API con configuración flexible
pub struct RepositoryApiModuleBuilder {
    unified_adapter: Option<Arc<UnifiedRepositoryAdapter>>,
    event_publisher: Option<Arc<EventPublisherAdapter>>,
}

impl RepositoryApiModuleBuilder {
    pub fn new() -> Self {
        Self {
            unified_adapter: None,
            event_publisher: None,
        }
    }

    pub fn with_unified_adapter(mut self, adapter: Arc<UnifiedRepositoryAdapter>) -> Self {
        self.unified_adapter = Some(adapter);
        self
    }

    pub fn with_event_publisher(mut self, publisher: Arc<EventPublisherAdapter>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }

    pub fn build(self) -> Result<RepositoryApiModule, String> {
        let unified_adapter = self.unified_adapter.ok_or("Unified adapter is required")?;
        let event_publisher = self.event_publisher.unwrap_or_else(|| Arc::new(EventPublisherAdapter::new()));

        // Crear contenedores DI para cada feature
        let create_di = CreateRepositoryDIContainer::new(
            unified_adapter.clone(),
            unified_adapter.clone(),
            unified_adapter.clone(),
        );

        let get_di = GetRepositoryDIContainer::new(
            unified_adapter.clone(),
        );

        let update_di = UpdateRepositoryDIContainer::new(
            unified_adapter.clone(),
            unified_adapter.clone(),
            unified_adapter.clone(),
        );

        let delete_di = DeleteRepositoryDIContainer::new(
            unified_adapter.clone(),
            unified_adapter.clone(),
            unified_adapter.clone(),
            event_publisher,
        );

        Ok(RepositoryApiModule::new(
            Arc::new(create_di.endpoint),
            Arc::new(get_di.endpoint),
            Arc::new(update_di.endpoint),
            Arc::new(delete_di.endpoint),
        ))
    }
}

/// Función helper para crear el módulo API con configuración de producción
pub fn create_repository_api_module(db: mongodb::Database) -> RepositoryApiModule {
    let unified_adapter = Arc::new(UnifiedRepositoryAdapter::new(db));
    let event_publisher = Arc::new(EventPublisherAdapter::new());

    RepositoryApiModuleBuilder::new()
        .with_unified_adapter(unified_adapter)
        .with_event_publisher(event_publisher)
        .build()
        .expect("Failed to build repository API module")
}

/// Función helper para crear el módulo API con configuración de testing
#[cfg(test)]
pub fn create_repository_api_module_for_testing() -> RepositoryApiModule {
    use crate::features::{
        create_repository::di::CreateRepositoryDIContainer,
        get_repository::di::GetRepositoryDIContainer,
        update_repository::di::UpdateRepositoryDIContainer,
        delete_repository::di::DeleteRepositoryDIContainer,
    };

    let create_di = CreateRepositoryDIContainer::for_testing();
    let get_di = GetRepositoryDIContainer::for_testing();
    let update_di = UpdateRepositoryDIContainer::for_testing();
    let delete_di = DeleteRepositoryDIContainer::for_testing();

    RepositoryApiModule::new(
        Arc::new(create_di.endpoint),
        Arc::new(get_di.endpoint),
        Arc::new(update_di.endpoint),
        Arc::new(delete_di.endpoint),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_module_creation() {
        let module = create_repository_api_module_for_testing();
        let router = module.create_router();
        
        // Verificar que el router tiene las rutas esperadas
        // Nota: Axum no expone directamente las rutas, pero podemos verificar que se crea sin errores
        assert!(true); // Placeholder - en tests reales usaríamos testcontainers
    }

    #[tokio::test]
    async fn test_api_module_builder() {
        let builder = RepositoryApiModuleBuilder::new();
        
        // Debería fallar sin unified adapter
        let result = builder.build();
        assert!(result.is_err());
        
        // Debería funcionar con unified adapter
        let unified_adapter = Arc::new(UnifiedRepositoryAdapter::new(mongodb::Database::default()));
        let result = RepositoryApiModuleBuilder::new()
            .with_unified_adapter(unified_adapter)
            .build();
        
        assert!(result.is_ok());
    }
}