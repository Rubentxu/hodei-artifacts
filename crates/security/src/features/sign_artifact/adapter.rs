use async_trait::async_trait;
use crate::features::sign_artifact::ports::{IArtifactHasher, IKeyProvider, ISignatureRepository, ArtifactHashingError, KeyProviderError, SignatureRepositoryError};
use artifact::domain::physical_artifact::PhysicalArtifact;
use crate::domain::signature::HashAlgorithm;
use shared::hrn::Hrn;
use sha2::{Sha256, Digest};
use ed25519_dalek::SigningKey;
use tracing::info;

// --- Adaptador para calcular hash SHA-256 ---
pub struct Sha256ArtifactHasher;

impl Sha256ArtifactHasher {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl IArtifactHasher for Sha256ArtifactHasher {
    async fn calculate_digest(&self, _artifact: &PhysicalArtifact, algorithm: HashAlgorithm) -> Result<Vec<u8>, ArtifactHashingError> {
        match algorithm {
            HashAlgorithm::Sha256 => {
                // En una implementación real, aquí se leería el contenido del artefacto
                // y se calcularía el hash. Por ahora, simulamos el proceso.
                
                // Simulamos la lectura del contenido del artefacto
                let mut hasher = Sha256::new();
                // En una implementación real, se leería el contenido real del artefacto
                hasher.update(b"simulated artifact content");
                let result = hasher.finalize();
                
                Ok(result.to_vec())
            },
            _ => Err(ArtifactHashingError::UnsupportedAlgorithm(format!("Unsupported hash algorithm: {:?}", algorithm)))
        }
    }
}

// --- Adaptador para obtener clave de firma desde archivo ---
pub struct FileKeyProvider {
    key_path: String,
    key_id: String,
}

impl FileKeyProvider {
    pub fn new(key_path: String, key_id: String) -> Self {
        Self { key_path, key_id }
    }
}

#[async_trait]
impl IKeyProvider for FileKeyProvider {
    async fn get_signing_key(&self) -> Result<SigningKey, KeyProviderError> {
        // En una implementación real, aquí se leería la clave del archivo
        // y se crearía un SigningKey. Por ahora, simulamos el proceso.
        
        info!("Reading signing key from file: {}", self.key_path);
        
        // Simulamos la lectura de una clave
        // En una implementación real, se leería la clave del archivo y se crearía un SigningKey
        let secret_key_bytes = [0u8; 32]; // Simulamos una clave de 32 bytes
        
        // En ed25519-dalek v2.0, creamos directamente el SigningKey desde bytes
        let signing_key = SigningKey::from_bytes(&secret_key_bytes);
        Ok(signing_key)
    }
    
    fn get_key_id(&self) -> String {
        self.key_id.clone()
    }
}

// --- Adaptador para el Repositorio de Firmas en MongoDB ---
pub struct MongoSignatureRepository;

impl MongoSignatureRepository {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl ISignatureRepository for MongoSignatureRepository {
    async fn save(&self, signature: &crate::domain::signature::Signature) -> Result<(), SignatureRepositoryError> {
        // En una implementación real, aquí se guardaría la firma en MongoDB
        info!("Saving signature {} for artifact {} to MongoDB", signature.id, signature.artifact_id);
        
        // Por ahora, solo simulamos la operación
        Ok(())
    }
    
    async fn get_by_artifact_id(&self, artifact_id: &Hrn) -> Result<Option<crate::domain::signature::Signature>, SignatureRepositoryError> {
        // En una implementación real, aquí se recuperaría la firma de MongoDB
        info!("Retrieving signature for artifact {} from MongoDB", artifact_id);
        
        // Por ahora, solo simulamos que no se encuentra la firma
        Ok(None)
    }
}