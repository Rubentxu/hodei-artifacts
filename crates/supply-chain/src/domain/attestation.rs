// crates/supply-chain/src/domain/attestation.rs

use shared::hrn::{Hrn, OrganizationId};
use shared::lifecycle::Lifecycle;
use serde::{Serialize, Deserialize};

/// Una prueba criptográficamente verificable sobre un artefacto (`PackageVersion`).
/// Es el Agregado Raíz principal de este contexto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attestation {
    /// El HRN único de la atestación.
    /// Formato: `hrn:hodei:supply-chain:<region>:<org_id>:attestation/<attestation_id>`
    pub hrn: Hrn,
    
    /// La organización a la que pertenece esta atestación.
    pub organization_hrn: OrganizationId,

    /// El HRN del `PackageVersion` al que se refiere esta prueba.
    pub subject_hrn: Hrn,

    /// El tipo de prueba contenida en el predicado (SBOM, SLSA, etc.).
    pub predicate_type: AttestationType,

    /// El contenido de la prueba en formato JSON. Se deserializa a un struct específico
    /// (ej. `SbomPredicate`) en tiempo de ejecución, basado en `predicate_type`.
    pub predicate: serde_json::Value,

    /// Lista de firmas que validan la integridad de esta atestación.
    pub signatures: Vec<Signature>,

    /// Información de auditoría y ciclo de vida.
    pub lifecycle: Lifecycle,
}

/// Una firma digital sobre una atestación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    /// El HRN de la `PublicKey` utilizada para generar esta firma.
    pub key_hrn: Hrn,
    
    /// El algoritmo de firma utilizado (ej. "rsassa-pss-sha256").
    pub algorithm: String,
    
    /// El valor de la firma, codificado en base64.
    pub value: String,
}

/// Tipos de atestaciones soportados por el sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttestationType {
    /// Software Bill of Materials - Lista de ingredientes de software.
    Sbom,
    /// Supply Chain Levels for Software Artifacts - Niveles de seguridad de la cadena de suministro.
    Slsa,
    /// Otros tipos de atestaciones personalizadas.
    Custom,
}

