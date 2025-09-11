// crates/supply-chain/src/domain/events.rs

use shared::hrn::Hrn;
use serde::{Serialize, Deserialize};

/// Eventos de dominio publicados por el contexto `supply-chain`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupplyChainEvent {
    /// Se ha generado una nueva atestación para un artefacto.
    AttestationGenerated {
        attestation_hrn: Hrn,
        subject_hrn: Hrn,
        attestation_type: String,
    },
    
    /// Se ha verificado una atestación existente.
    AttestationVerified {
        attestation_hrn: Hrn,
        subject_hrn: Hrn,
        is_valid: bool,
        verification_time: chrono::DateTime<chrono::Utc>,
    },
    
    /// Se ha detectado una vulnerabilidad en un componente de una SBOM.
    SbomVulnerabilityDetected {
        sbom_hrn: Hrn,
        component_purl: String,
        vulnerability_id: String,
        severity: String,
    },
}