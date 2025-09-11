use serde::{Deserialize, Serialize};
use shared::hrn::Hrn;
use chrono::{DateTime, Utc};

/// Representa una firma digital de un artefacto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    /// ID único de la firma en formato HRN.
    pub id: Hrn,
    
    /// ID del artefacto al que pertenece esta firma.
    pub artifact_id: Hrn,
    
    /// Algoritmo de hash utilizado para calcular el digest.
    pub hash_algorithm: HashAlgorithm,
    
    /// Digest del artefacto (hash criptográfico).
    pub digest: Vec<u8>,
    
    /// Algoritmo de firma utilizado.
    pub signature_algorithm: SignatureAlgorithm,
    
    /// Valor de la firma digital.
    pub signature_value: Vec<u8>,
    
    /// ID de la clave pública utilizada para la firma.
    pub key_id: String,
    
    /// Fecha y hora de creación de la firma.
    pub created_at: DateTime<Utc>,
}

/// Algoritmos de hash soportados.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Sha256,
    Sha512,
}

/// Algoritmos de firma soportados.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureAlgorithm {
    Ed25519,
    RsaPkcs1v15,
}