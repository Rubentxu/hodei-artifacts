// crates/repository/src/features/create_repository/dto.rs

use serde::{Deserialize, Serialize};
use shared::enums::Ecosystem;
use crate::domain::repository::{RepositoryType, HostedConfig, ProxyConfig, VirtualConfig};
use url::Url;

/// Comando para crear un nuevo repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepositoryCommand {
    /// Nombre del repositorio (único dentro de la organización)
    pub name: String,
    
    /// Descripción del repositorio
    pub description: Option<String>,
    
    /// Tipo de repositorio (Hosted, Proxy, Virtual)
    pub repo_type: RepositoryType,
    
    /// Ecosistema de paquetes (Maven, NPM, Docker, etc.)
    pub format: Ecosystem,
    
    /// Configuración específica según el tipo
    pub config: RepositoryConfigDto,
    
    /// HRN del backend de almacenamiento (solo para Hosted)
    pub storage_backend_hrn: Option<String>,
    
    /// Indica si el repositorio es público
    pub is_public: bool,
    
    /// Metadatos personalizados adicionales
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

/// DTO para la configuración del repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "config")]
pub enum RepositoryConfigDto {
    Hosted(HostedConfigDto),
    Proxy(ProxyConfigDto),
    Virtual(VirtualConfigDto),
}

/// Configuración para repositorio Hosted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedConfigDto {
    /// Política de despliegue
    pub deployment_policy: DeploymentPolicyDto,
}

/// Configuración para repositorio Proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfigDto {
    /// URL del repositorio remoto
    pub remote_url: String,
    
    /// Configuración de caché
    pub cache_settings: CacheSettingsDto,
    
    /// Autenticación para el repositorio remoto (opcional)
    pub remote_authentication: Option<ProxyAuthDto>,
}

/// Configuración para repositorio Virtual
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualConfigDto {
    /// Lista de HRNs de repositorios agregados
    pub aggregated_repositories: Vec<String>,
    
    /// Orden de resolución
    pub resolution_order: ResolutionOrderDto,
}

/// Política de despliegue
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DeploymentPolicyDto {
    AllowSnapshots,
    BlockSnapshots,
    AllowRedeploy,
    BlockRedeploy,
}

/// Configuración de caché
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSettingsDto {
    /// Tiempo de vida para metadatos en segundos
    pub metadata_ttl_seconds: u32,
    
    /// Tiempo de vida para artefactos en segundos
    pub artifact_ttl_seconds: u32,
}

/// Autenticación para proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAuthDto {
    /// Nombre de usuario
    pub username: String,
    
    /// HRN del secreto de contraseña
    pub password_secret_hrn: String,
}

/// Orden de resolución
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResolutionOrderDto {
    FirstFound,
}

/// Respuesta de creación de repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepositoryResponse {
    /// HRN del repositorio creado
    pub hrn: String,
    
    /// Nombre del repositorio
    pub name: String,
    
    /// Tipo de repositorio
    pub repo_type: RepositoryType,
    
    /// Ecosistema
    pub format: Ecosystem,
    
    /// Fecha de creación
    pub created_at: time::OffsetDateTime,
}

// Conversiones entre DTOs y modelos de dominio
impl From<DeploymentPolicyDto> for crate::domain::repository::DeploymentPolicy {
    fn from(dto: DeploymentPolicyDto) -> Self {
        match dto {
            DeploymentPolicyDto::AllowSnapshots => crate::domain::repository::DeploymentPolicy::AllowSnapshots,
            DeploymentPolicyDto::BlockSnapshots => crate::domain::repository::DeploymentPolicy::BlockSnapshots,
            DeploymentPolicyDto::AllowRedeploy => crate::domain::repository::DeploymentPolicy::AllowRedeploy,
            DeploymentPolicyDto::BlockRedeploy => crate::domain::repository::DeploymentPolicy::BlockRedeploy,
        }
    }
}

impl From<CacheSettingsDto> for crate::domain::repository::CacheSettings {
    fn from(dto: CacheSettingsDto) -> Self {
        crate::domain::repository::CacheSettings {
            metadata_ttl_seconds: dto.metadata_ttl_seconds,
            artifact_ttl_seconds: dto.artifact_ttl_seconds,
        }
    }
}

impl From<ProxyAuthDto> for crate::domain::repository::ProxyAuth {
    fn from(dto: ProxyAuthDto) -> Self {
        crate::domain::repository::ProxyAuth {
            username: dto.username,
            password_secret_hrn: shared::hrn::Hrn::new(&dto.password_secret_hrn).unwrap(),
        }
    }
}

impl From<ResolutionOrderDto> for crate::domain::repository::ResolutionOrder {
    fn from(dto: ResolutionOrderDto) -> Self {
        match dto {
            ResolutionOrderDto::FirstFound => crate::domain::repository::ResolutionOrder::FirstFound,
        }
    }
}

impl From<RepositoryConfigDto> for crate::domain::repository::RepositoryConfig {
    fn from(dto: RepositoryConfigDto) -> Self {
        match dto {
            RepositoryConfigDto::Hosted(config) => {
                crate::domain::repository::RepositoryConfig::Hosted(
                    crate::domain::repository::HostedConfig {
                        deployment_policy: config.deployment_policy.into(),
                    }
                )
            },
            RepositoryConfigDto::Proxy(config) => {
                let remote_url = Url::parse(&config.remote_url).unwrap();
                
                crate::domain::repository::RepositoryConfig::Proxy(
                    crate::domain::repository::ProxyConfig {
                        remote_url,
                        cache_settings: config.cache_settings.into(),
                        remote_authentication: config.remote_authentication.map(|auth| auth.into()),
                    }
                )
            },
            RepositoryConfigDto::Virtual(config) => {
                let mut aggregated_repositories = Vec::new();
                for repo_hrn in config.aggregated_repositories {
                    let repo_id = shared::hrn::RepositoryId::new(
                        &shared::hrn::OrganizationId::new("system").unwrap().to_string(), // This will be replaced with actual org
                        &repo_hrn
                    ).unwrap();
                    aggregated_repositories.push(repo_id);
                }
                
                crate::domain::repository::RepositoryConfig::Virtual(
                    crate::domain::repository::VirtualConfig {
                        aggregated_repositories,
                        resolution_order: config.resolution_order.into(),
                    }
                )
            },
        }
    }
}