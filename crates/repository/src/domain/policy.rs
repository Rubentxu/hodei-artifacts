// crates/repository/src/domain/policy.rs

use shared::hrn::{Hrn, RepositoryId};
use shared::lifecycle::Lifecycle;
use shared::enums::ArtifactStatus; // Necesario para algunas reglas
use serde::{Serialize, Deserialize};

/// Un regex validado para prevenir ataques de Denegación de Servicio (ReDoS).
/// El constructor debe implementar la lógica de validación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafeRegex(String);

/// Una política de retención de artefactos que se aplica a un repositorio.
/// Es un Agregado Raíz.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// HRN de la política.
    /// Formato: `hrn:hodei:repository:<region>:<org_id>:retention-policy/<policy_name>`
    pub hrn: Hrn,

    /// HRN del repositorio al que se aplica esta política.
    pub repository_hrn: RepositoryId,
    
    /// Nombre de la política.
    pub name: String,

    /// Lista de reglas que componen la política. Se ejecutan en orden.
    pub rules: Vec<RetentionRule>,

    /// Si la política está activa.
    pub is_enabled: bool,
    
    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}

/// Una regla específica dentro de una política de retención.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetentionRule {
    /// Aplica a artefactos que no han sido descargados en un número de días.
    ByAgeSinceLastDownload {
        max_age_days: u32,
        action: RetentionAction,
    },
    /// Mantiene solo las N versiones más recientes de un paquete.
    ByVersionCount {
        max_versions: u32,
        action: RetentionAction,
    },
    /// Aplica a artefactos que coinciden con un estado específico.
    ByStatus {
        status: ArtifactStatus,
        action: RetentionAction,
    },
    /// Aplica a artefactos cuyo nombre de versión coincide con un regex.
    /// Ideal para limpiar versiones SNAPSHOT.
    MatchesVersionRegex {
        regex: SafeRegex,
        action: RetentionAction,
    },
}

/// Acción a tomar cuando una regla de retención se cumple.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RetentionAction { Delete, Archive, Notify }