// crates/distribution/src/application/format_registry.rs

use std::collections::HashMap;
use shared::enums::Ecosystem;
use crate::domain::{FormatHandler, FormatRequest, FormatResponse, DistributionResult, DistributionError};
use crate::domain::{MavenFormatHandler, NpmFormatHandler, DockerFormatHandler};

/// Registro de manejadores de formato
pub struct FormatHandlerRegistry {
    handlers: HashMap<Ecosystem, Box<dyn FormatHandler>>,
}

impl FormatHandlerRegistry {
    pub fn new() -> Self {
        let mut handlers = HashMap::new();
        
        // Registrar manejadores disponibles
        handlers.insert(Ecosystem::Maven, Box::new(MavenFormatHandler::new()));
        handlers.insert(Ecosystem::Npm, Box::new(NpmFormatHandler::new()));
        handlers.insert(Ecosystem::Docker, Box::new(DockerFormatHandler::new()));
        
        Self { handlers }
    }

    /// Procesa una petición de formato
    pub async fn handle_request(&self, request: FormatRequest) -> DistributionResult<FormatResponse> {
        // Encontrar el manejador apropiado
        let handler = self.handlers.get(&request.ecosystem)
            .ok_or_else(|| DistributionError::FormatNotSupported(format!("No handler for ecosystem: {:?}", request.ecosystem)))?;

        // Verificar que el manejador puede procesar este path
        if !handler.can_handle(&request.path, &request.ecosystem) {
            return Err(DistributionError::InvalidRequest(format!(
                "Handler for {:?} cannot process path: {}", 
                request.ecosystem, 
                request.path
            )));
        }

        // Procesar la petición
        handler.handle_request(request).await
    }

    /// Obtiene un manejador específico
    pub fn get_handler(&self, ecosystem: &Ecosystem) -> Option<&dyn FormatHandler> {
        self.handlers.get(ecosystem).map(|h| h.as_ref())
    }

    /// Lista todos los ecosistemas soportados
    pub fn supported_ecosystems(&self) -> Vec<Ecosystem> {
        self.handlers.keys().cloned().collect()
    }

    /// Verifica si un ecosistema está soportado
    pub fn is_supported(&self, ecosystem: &Ecosystem) -> bool {
        self.handlers.contains_key(ecosystem)
    }
}

impl Default for FormatHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::{RepositoryId, UserId};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = FormatHandlerRegistry::new();
        
        assert!(registry.is_supported(&Ecosystem::Maven));
        assert!(registry.is_supported(&Ecosystem::Npm));
        assert!(registry.is_supported(&Ecosystem::Docker));
    }

    #[tokio::test]
    async fn test_supported_ecosystems() {
        let registry = FormatHandlerRegistry::new();
        
        let ecosystems = registry.supported_ecosystems();
        assert!(ecosystems.contains(&Ecosystem::Maven));
        assert!(ecosystems.contains(&Ecosystem::Npm));
    }

    #[tokio::test]
    async fn test_get_handler() {
        let registry = FormatHandlerRegistry::new();
        
        let maven_handler = registry.get_handler(&Ecosystem::Maven);
        assert!(maven_handler.is_some());
        assert_eq!(maven_handler.unwrap().supported_ecosystem(), Ecosystem::Maven);
        
        let npm_handler = registry.get_handler(&Ecosystem::Npm);
        assert!(npm_handler.is_some());
        assert_eq!(npm_handler.unwrap().supported_ecosystem(), Ecosystem::Npm);
        
        let docker_handler = registry.get_handler(&Ecosystem::Docker);
        assert!(docker_handler.is_some());
        assert_eq!(docker_handler.unwrap().supported_ecosystem(), Ecosystem::Docker);
    }

    #[tokio::test]
    async fn test_handle_maven_request() {
        let registry = FormatHandlerRegistry::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Maven,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "maven-repo"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/com/example/my-app/1.0.0/my-app-1.0.0.jar".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let response = registry.handle_request(request).await.unwrap();
        assert_eq!(response.status_code, 200);
        assert!(response.body.is_some());
    }

    #[tokio::test]
    async fn test_handle_npm_request() {
        let registry = FormatHandlerRegistry::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Npm,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "npm-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/my-package".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let response = registry.handle_request(request).await.unwrap();
        assert_eq!(response.status_code, 200);
        assert!(response.body.is_some());
    }

    #[tokio::test]
    async fn test_handle_docker_request() {
        let registry = FormatHandlerRegistry::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Docker,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "docker-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/v2/library/nginx/manifests/latest".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let response = registry.handle_request(request).await.unwrap();
        assert_eq!(response.status_code, 200);
        assert!(response.body.is_some());
        
        let json = String::from_utf8(response.body.unwrap()).unwrap();
        assert!(json.contains("\"schemaVersion\": 2"));
    }

    #[tokio::test]
    async fn test_unsupported_ecosystem() {
        let registry = FormatHandlerRegistry::new();
        
        let request = FormatRequest {
            ecosystem: Ecosystem::Generic, // No implementado
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "generic-registry"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/some-package".to_string(),
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let result = registry.handle_request(request).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DistributionError::FormatNotSupported(_)));
    }

    #[tokio::test]
    async fn test_handler_cannot_process_path() {
        let registry = FormatHandlerRegistry::new();
        
        // Maven handler no puede procesar un path npm
        let request = FormatRequest {
            ecosystem: Ecosystem::Maven,
            repository_hrn: RepositoryId::new(
                &shared::hrn::OrganizationId::new("test-org").unwrap(),
                "maven-repo"
            ).unwrap(),
            user_id: UserId::new_system_user(),
            path: "/my-package".to_string(), // Path npm
            method: "GET".to_string(),
            headers: HashMap::new(),
            body: None,
            query_params: HashMap::new(),
        };

        let result = registry.handle_request(request).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DistributionError::InvalidRequest(_)));
    }
}