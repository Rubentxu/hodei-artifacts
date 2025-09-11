use std::sync::Arc;
use crate::features::sign_artifact::{
    use_case::SignArtifactUseCase,
    ports::{IArtifactHasher, IKeyProvider, ISignatureRepository},
    adapter::{Sha256ArtifactHasher, FileKeyProvider, MongoSignatureRepository},
};

/// Configures and provides dependencies for the Sign Artifact feature.
/// This follows the Dependency Injection pattern to allow for easy testing and substitution.
pub struct SignArtifactDI;

impl SignArtifactDI {
    /// Creates a production-ready instance of the SignArtifactUseCase
    pub fn create_use_case() -> SignArtifactUseCase {
        let hasher = Arc::new(Sha256ArtifactHasher::new()) as Arc<dyn IArtifactHasher>;
        let key_provider = Arc::new(FileKeyProvider::new(
            "/etc/hodei/keys/signing.key".to_string(),
            "default-key-1".to_string()
        )) as Arc<dyn IKeyProvider>;
        let repository = Arc::new(MongoSignatureRepository::new()) as Arc<dyn ISignatureRepository>;
        
        SignArtifactUseCase::new(hasher, key_provider, repository)
    }
    
    /// Creates a test-friendly instance of the SignArtifactUseCase with mockable dependencies
    #[cfg(test)]
    pub fn create_use_case_with_deps(
        hasher: Arc<dyn IArtifactHasher>,
        key_provider: Arc<dyn IKeyProvider>,
        repository: Arc<dyn ISignatureRepository>,
    ) -> SignArtifactUseCase {
        SignArtifactUseCase::new(hasher, key_provider, repository)
    }
}