use crate::domain::signature::{Signature, HashAlgorithm, SignatureAlgorithm};
use crate::features::sign_artifact::ports::{IArtifactHasher, IKeyProvider, ISignatureRepository, ArtifactHashingError, KeyProviderError};
use std::sync::Arc;
use shared::models::ArtifactReference;
use shared::hrn::Hrn;
use ed25519_dalek::Signer;
use uuid::Uuid;

pub struct SignArtifactUseCase {
    hasher: Arc<dyn IArtifactHasher>,
    key_provider: Arc<dyn IKeyProvider>,
    repository: Arc<dyn ISignatureRepository>,
}

impl SignArtifactUseCase {
    pub fn new(
        hasher: Arc<dyn IArtifactHasher>,
        key_provider: Arc<dyn IKeyProvider>,
        repository: Arc<dyn ISignatureRepository>,
    ) -> Self {
        Self { hasher, key_provider, repository }
    }

    pub async fn execute(&self, artifact_ref: &ArtifactReference) -> Result<Signature, SigningError> {
        // En una implementación real, aquí se obtendría el artefacto físico
        // Por ahora, simulamos el proceso de firma
        
        // 1. Calcular el digest del artefacto
        // En una implementación real, se pasaría el artefacto físico al hasher
        // TODO: Actually get the physical artifact and pass it to the hasher
        // For now, we simulate the digest calculation
        let digest = vec![0u8; 32]; // 32 bytes para SHA-256 (simulated)
        
        // 2. Obtener la clave de firma
        let signing_key = self.key_provider.get_signing_key().await
            .map_err(|e| SigningError::KeyProviderError(e))?;
        
        // 3. Firmar el digest
        let ed_signature = signing_key.sign(&digest);
        let signature_bytes = ed_signature.to_bytes().to_vec();
        
        // 4. Crear el objeto Signature
        let signature = Signature {
            id: Hrn::new(&format!("hrn:hodei:security:us-east-1:123456789012:signature/{}", Uuid::new_v4())).unwrap(),
            artifact_id: artifact_ref.artifact_hrn.0.clone(), // Extract the Hrn from PhysicalArtifactId
            hash_algorithm: HashAlgorithm::Sha256,
            digest,
            signature_algorithm: SignatureAlgorithm::Ed25519,
            signature_value: signature_bytes,
            key_id: self.key_provider.get_key_id(),
            created_at: chrono::Utc::now(),
        };
        
        // 5. Guardar la firma
        self.repository.save(&signature).await
            .map_err(|e| SigningError::RepositoryError(e))?;
        
        Ok(signature)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("Artifact hashing failed: {0}")]
    HashingError(#[from] ArtifactHashingError),
    
    #[error("Key provider error: {0}")]
    KeyProviderError(#[from] KeyProviderError),
    
    #[error("Signature repository error: {0}")]
    RepositoryError(crate::features::sign_artifact::ports::SignatureRepositoryError),
    
    #[error("Signature generation failed: {0}")]
    GenerationFailed(String),
}

impl From<crate::features::sign_artifact::ports::SignatureRepositoryError> for SigningError {
    fn from(error: crate::features::sign_artifact::ports::SignatureRepositoryError) -> Self {
        SigningError::RepositoryError(error)
    }
}