// crates/supply-chain/src/domain/attestation.rs

use shared::hrn::{Hrn, OrganizationId, PackageVersionId, PublicKeyId};
use shared::lifecycle::Lifecycle;
use shared::security::HodeiResource;
use serde::{Serialize, Deserialize};
use cedar_policy::{EntityUid, Expr};
use std::collections::HashMap;

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
    pub subject_hrn: PackageVersionId,

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
    pub key_hrn: PublicKeyId,
    
    /// El algoritmo de firma utilizado (ej. "rsassa-pss-sha256").
    pub algorithm: String,
    
    /// El valor de la firma, codificado en base64.
    pub value: String,
}

/// Tipos de atestaciones soportados por el sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttestationType {
    SbomCycloneDxV1_5Json,
    SbomSpdxV2_3Json,
    SlsaProvenanceV1_0,
    CosignSignature,
    GenericSignature, // Para otros tipos de firma
}

/// Implementación para que las atestaciones puedan ser recursos en políticas Cedar.
impl HodeiResource<EntityUid, Expr> for Attestation {
    fn resource_id(&self) -> EntityUid {
        EntityUid::from_str(&self.hrn.as_str()).unwrap()
    }

    fn resource_attributes(&self) -> HashMap<String, Expr> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), Expr::val("attestation"));
        attrs.insert("predicate_type".to_string(), Expr::val(self.predicate_type.as_ref()));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de una atestación es el artefacto que describe.
        // Esto permite políticas como "El artefacto X debe tener una atestación de tipo Y".
        vec![EntityUid::from_str(self.subject_hrn.as_str()).unwrap()]
    }
}