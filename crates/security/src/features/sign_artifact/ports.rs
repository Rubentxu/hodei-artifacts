use async_trait::async_trait;
use crate::domain::signature::{Signature, HashAlgorithm};
use artifact::domain::physical_artifact::PhysicalArtifact;
use shared::hrn::Hrn;
use ed25519_dalek::SigningKey;

// Puerto para calcular el hash de un artefacto.
#[async_trait]
pub trait IArtifactHasher: Send + Sync {
    async fn calculate_digest(&self, artifact: &PhysicalArtifact, algorithm: HashAlgorithm) -> Result<Vec<u8>, ArtifactHashingError>;
}

// Puerto para obtener una clave de firma.
#[async_trait]
pub trait IKeyProvider: Send + Sync {
    async fn get_signing_key(&self) -> Result<SigningKey, KeyProviderError>;
    fn get_key_id(&self) -> String;
}

// Puerto para persistir la firma.
#[async_trait]
pub trait ISignatureRepository: Send + Sync {
    async fn save(&self, signature: &Signature) -> Result<(), SignatureRepositoryError>;
    async fn get_by_artifact_id(&self, artifact_id: &Hrn) -> Result<Option<Signature>, SignatureRepositoryError>;
}

// Errores específicos de hashing de artefactos
#[derive(Debug, thiserror::Error)]
pub enum ArtifactHashingError {
    #[error("Failed to calculate digest: {0}")]
    HashingFailed(String),
    
    #[error("Unsupported hash algorithm: {0}")]
    UnsupportedAlgorithm(String),
    
    #[error("Artifact not found: {0}")]
    ArtifactNotFound(String),
}

// Errores específicos del proveedor de claves
#[derive(Debug, thiserror::Error)]
pub enum KeyProviderError {
    #[error("Failed to get signing key: {0}")]
    KeyRetrievalFailed(String),
    
    #[error("Invalid key format: {0}")]
    InvalidKeyFormat(String),
    
    #[error("Key not found: {0}")]
    KeyNotFound(String),
}

// Errores específicos del repositorio de firmas
#[derive(Debug, thiserror::Error)]
pub enum SignatureRepositoryError {
    #[error("Failed to save signature: {0}")]
    SaveError(String),
    
    #[error("Failed to retrieve signature: {0}")]
    RetrieveError(String),
    
    #[error("Signature not found for artifact: {0}")]
    NotFound(String),
}