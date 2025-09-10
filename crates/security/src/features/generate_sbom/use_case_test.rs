#[cfg(test)]
mod tests {
    use crate::features::generate_sbom::use_case::GenerateSbomUseCase;
    use crate::features::generate_sbom::ports::{ISbomGenerator, ISbomRepository, SbomGenerationError, SbomRepositoryError};
    use crate::domain::sbom::{Sbom, SbomFormat, SbomMetadata};
    use std::sync::Arc;
    use mockall::mock;
    use async_trait::async_trait;
    use shared::models::ArtifactReference;
    use shared::enums::ArtifactType;
    use shared::hrn::PhysicalArtifactId;
    use uuid::Uuid;
    use chrono::Utc;

    mock! {
        pub SbomGenerator {
            async fn generate(&self, artifact: &artifact::domain::physical_artifact::PhysicalArtifact, format: SbomFormat) -> Result<Sbom, SbomGenerationError> {}
        }
    }

    #[async_trait]
    impl ISbomGenerator for MockSbomGenerator {
        async fn generate(&self, artifact: &artifact::domain::physical_artifact::PhysicalArtifact, format: SbomFormat) -> Result<Sbom, SbomGenerationError> {
            self.generate(artifact, format).await
        }
    }

    mock! {
        pub SbomRepository {
            async fn save(&self, sbom: &Sbom) -> Result<(), SbomRepositoryError> {}
            async fn get_by_artifact_id(&self, artifact_id: &str) -> Result<Option<Sbom>, SbomRepositoryError> {}
        }
    }

    #[async_trait]
    impl ISbomRepository for MockSbomRepository {
        async fn save(&self, sbom: &Sbom) -> Result<(), SbomRepositoryError> {
            self.save(sbom).await
        }
        
        async fn get_by_artifact_id(&self, artifact_id: &str) -> Result<Option<Sbom>, SbomRepositoryError> {
            self.get_by_artifact_id(artifact_id).await
        }
    }

    #[tokio::test]
    async fn test_execute_success() {
        // Arrange
        let mut generator = MockSbomGenerator::new();
        let mut repository = MockSbomRepository::new();
        
        let artifact_ref = ArtifactReference {
            artifact_hrn: PhysicalArtifactId::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234").unwrap(),
            artifact_type: ArtifactType::Primary,
            role: None,
        };
        
        let expected_sbom = Sbom {
            id: Uuid::new_v4().to_string(),
            artifact_id: "hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234".to_string(),
            format: SbomFormat::CycloneDX,
            spec_version: "1.4".to_string(),
            content: "{}".to_string(),
            created_at: Utc::now(),
            metadata: SbomMetadata {
                generator: "syft".to_string(),
                generator_version: "0.78.0".to_string(),
                component_count: 1,
            },
        };
        
        generator.expect_generate()
            .returning(move |_, _| Ok(expected_sbom.clone()));
            
        repository.expect_save()
            .returning(|_| Ok(()));
            
        let use_case = GenerateSbomUseCase::new(
            Arc::new(generator),
            Arc::new(repository),
        );

        // Act
        let result = use_case.execute(&artifact_ref).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_generator_failure() {
        // Arrange
        let mut generator = MockSbomGenerator::new();
        let repository = MockSbomRepository::new();
        
        let artifact_ref = ArtifactReference {
            artifact_hrn: PhysicalArtifactId::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234").unwrap(),
            artifact_type: ArtifactType::Primary,
            role: None,
        };
        
        generator.expect_generate()
            .returning(|_, _| Err(SbomGenerationError::GenerationFailed("Failed to generate SBOM".to_string())));
            
        let use_case = GenerateSbomUseCase::new(
            Arc::new(generator),
            Arc::new(repository),
        );

        // Act
        let result = use_case.execute(&artifact_ref).await;

        // Assert
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SbomGenerationError::GenerationFailed(_)));
    }

    #[tokio::test]
    async fn test_execute_repository_failure() {
        // Arrange
        let mut generator = MockSbomGenerator::new();
        let mut repository = MockSbomRepository::new();
        
        let artifact_ref = ArtifactReference {
            artifact_hrn: PhysicalArtifactId::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234").unwrap(),
            artifact_type: ArtifactType::Primary,
            role: None,
        };
        
        let sbom = Sbom {
            id: Uuid::new_v4().to_string(),
            artifact_id: "hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234".to_string(),
            format: SbomFormat::CycloneDX,
            spec_version: "1.4".to_string(),
            content: "{}".to_string(),
            created_at: Utc::now(),
            metadata: SbomMetadata {
                generator: "syft".to_string(),
                generator_version: "0.78.0".to_string(),
                component_count: 1,
            },
        };
        
        generator.expect_generate()
            .returning(move |_, _| Ok(sbom.clone()));
            
        repository.expect_save()
            .returning(|_| Err(SbomRepositoryError::SaveError("Failed to save SBOM".to_string())));
            
        let use_case = GenerateSbomUseCase::new(
            Arc::new(generator),
            Arc::new(repository),
        );

        // Act
        let result = use_case.execute(&artifact_ref).await;

        // Assert
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SbomGenerationError::GenerationFailed(_)));
    }
}