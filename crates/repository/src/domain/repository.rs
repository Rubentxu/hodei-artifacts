// crates/repository/src/domain/repository.rs

use shared::hrn::{Hrn, OrganizationId, RepositoryId};
use shared::lifecycle::Lifecycle;
use shared::enums::Ecosystem;
use serde::{Serialize, Deserialize};
use url::Url;
use std::str::FromStr;

/// Representa un contenedor para artefactos que define políticas de acceso y almacenamiento.
/// Es el Agregado Raíz principal de este Bounded Context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    /// El HRN único y global del repositorio.
    /// Formato: `hrn:hodei:repository:<region>:<org_id>:repository/<repo_name>`
    pub hrn: RepositoryId,

    /// La organización a la que pertenece este repositorio.
    pub organization_hrn: OrganizationId,
    
    /// El nombre del repositorio, único dentro de la organización.
    pub name: String,

    /// La región geográfica donde reside primariamente este repositorio.
    pub region: String,
    
    /// El tipo de comportamiento del repositorio (Hosted, Proxy, Virtual).
    pub repo_type: RepositoryType,
    
    /// El ecosistema de paquetes que gestiona este repositorio (Maven, Npm, etc.).
    pub format: Ecosystem,

    /// Configuración detallada y específica según el `repo_type`.
    pub config: RepositoryConfig,
    
    /// HRN del backend de almacenamiento donde se guardarán los binarios.
    pub storage_backend_hrn: Hrn,

    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}

/// Configuración específica según el tipo de repositorio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepositoryConfig {
    Hosted(HostedConfig),
    Proxy(ProxyConfig),
    Virtual(VirtualConfig),
}

impl RepositoryConfig {
    pub fn get_type(&self) -> RepositoryType {
        match self {
            RepositoryConfig::Hosted(_) => RepositoryType::Hosted,
            RepositoryConfig::Proxy(_) => RepositoryType::Proxy,
            RepositoryConfig::Virtual(_) => RepositoryType::Virtual,
        }
    }
}


/// Configuración para un repositorio de tipo `Hosted`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedConfig {
    /// Define si se permiten artefactos de tipo SNAPSHOT, re-despliegues, etc.
    pub deployment_policy: DeploymentPolicy,
}

/// Configuración para un repositorio de tipo `Proxy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// La URL del repositorio remoto que se está "proxiando".
    pub remote_url: Url,
    /// Configuración del caché para los artefactos y metadatos.
    pub cache_settings: CacheSettings,
    /// Credenciales para autenticarse contra el repositorio remoto.
    pub remote_authentication: Option<ProxyAuth>,
}

/// Configuración para un repositorio de tipo `Virtual`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualConfig {
    /// Lista ordenada de HRNs de repositorios (Hosted o Proxy) que se agregan.
    pub aggregated_repositories: Vec<RepositoryId>,
    /// Estrategia para resolver artefactos cuando existen en múltiples repositorios agregados.
    pub resolution_order: ResolutionOrder,
}

/// Configuración de caché para repositorios Proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSettings {
    /// Tiempo de vida (en segundos) para los metadatos cacheados.
    pub metadata_ttl_seconds: u32,
    /// Tiempo de vida (en segundos) para los artefactos binarios cacheados.
    pub artifact_ttl_seconds: u32,
}

/// Credenciales seguras para un repositorio Proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAuth {
    pub username: String,
    /// HRN a un secreto en un gestor de secretos externo (ej. Vault).
    /// El valor del secreto nunca se almacena en este modelo.
    pub password_secret_hrn: Hrn,
}

/// El tipo de comportamiento del repositorio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepositoryType { Hosted, Proxy, Virtual }

/// Las reglas de despliegue para un repositorio Hosted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentPolicy { AllowSnapshots, BlockSnapshots, AllowRedeploy, BlockRedeploy }

impl FromStr for DeploymentPolicy {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AllowSnapshots" => Ok(DeploymentPolicy::AllowSnapshots),
            "BlockSnapshots" => Ok(DeploymentPolicy::BlockSnapshots),
            "AllowRedeploy" => Ok(DeploymentPolicy::AllowRedeploy),
            "BlockRedeploy" => Ok(DeploymentPolicy::BlockRedeploy),
            _ => Err(()),
        }
    }
}

/// La estrategia de resolución para un repositorio Virtual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionOrder { FirstFound }

impl FromStr for ResolutionOrder {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "FirstFound" => Ok(ResolutionOrder::FirstFound),
            _ => Err(()),
        }
    }
}