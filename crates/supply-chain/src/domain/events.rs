// crates/supply-chain/src/domain/events.rs

use shared::hrn::{Hrn, PackageVersionId};
use crate::domain::attestation::AttestationType;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;

/// Eventos de dominio publicados por el contexto `supply-chain`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupplyChainEvent {
    /// Se ha generado y almacenado una nueva atestación para un artefacto.
    AttestationGenerated(AttestationGenerated),

    /// Se ha verificado la firma de una atestación.
    SignatureVerified(SignatureVerified),
    
    /// Se ha añadido una nueva clave pública al sistema.
    PublicKeyAdded(PublicKeyAdded),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationGenerated {
    pub hrn: Hrn,
    pub subject_hrn: PackageVersionId,
    pub predicate_type: AttestationType,
    pub generated_by: Hrn,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureVerified {
    pub attestation_hrn: Hrn,
    pub key_hrn: Hrn,
    pub is_valid: bool,
    pub verified_by: Hrn,
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeyAdded {
    pub hrn: Hrn,
    pub source: String, // "ManualUpload", "CertificateAuthority"
    pub added_by: Hrn,
    pub at: OffsetDateTime,
}