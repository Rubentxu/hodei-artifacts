use std::sync::Arc;
use crate::features::generate_sbom::{
    use_case::GenerateSbomUseCase,
    ports::{ISbomGenerator, ISbomRepository, IArtifactRetriever},
    adapter::{SyftSbomGenerator, S3SbomRepository, MongoArtifactRetriever},
};

/// Configures and provides dependencies for the Generate SBOM feature.
/// This follows the Dependency Injection pattern to allow for easy testing and substitution.
pub struct GenerateSbomDI;

impl GenerateSbomDI {
    /// Creates a production-ready instance of the GenerateSbomUseCase
    pub fn create_use_case() -> GenerateSbomUseCase {
        let generator = Arc::new(SyftSbomGenerator::new()) as Arc<dyn ISbomGenerator>;
        let repository = Arc::new(S3SbomRepository::new()) as Arc<dyn ISbomRepository>;
        let artifact_retriever = Arc::new(MongoArtifactRetriever::new()) as Arc<dyn IArtifactRetriever>;
        
        GenerateSbomUseCase::new(generator, repository, artifact_retriever)
    }
    
    /// Creates a test-friendly instance of the GenerateSbomUseCase with mockable dependencies
    #[cfg(test)]
    pub fn create_use_case_with_deps(
        generator: Arc<dyn ISbomGenerator>,
        repository: Arc<dyn ISbomRepository>,
        artifact_retriever: Arc<dyn IArtifactRetriever>,
    ) -> GenerateSbomUseCase {
        GenerateSbomUseCase::new(generator, repository, artifact_retriever)
    }
}