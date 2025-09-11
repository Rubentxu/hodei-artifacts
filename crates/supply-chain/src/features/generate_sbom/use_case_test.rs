#[cfg(test)]
mod tests {
    use crate::features::generate_sbom::use_case::GenerateSbomUseCase;
    use crate::features::generate_sbom::ports::{ISbomGenerator, ISbomRepository, IArtifactRetriever, SbomGenerationError, SbomRepositoryError};
    use crate::domain::sbom::{Sbom, SbomFormat, ToolInformation};
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
            async fn get_by_artifact_id(&self, artifact_id: &PhysicalArtifactId) -> Result<Option<Sbom>, SbomRepositoryError> {}
        }
    }

    #[async_trait]
    impl ISbomRepository for MockSbomRepository {
        async fn save(&self, sbom: &Sbom) -> Result<(), SbomRepositoryError> {
            self.save(sbom).await
        }
        
        async fn get_by_artifact_id(&self, artifact_id: &PhysicalArtifactId) -> Result<Option<Sbom>, SbomRepositoryError> {
            self.get_by_artifact_id(artifact_id).await
        }
    }

    mock! {
        pub ArtifactRetriever {
            async fn get_physical_artifact(&self, artifact_id: &PhysicalArtifactId) -> Result<artifact::domain::physical_artifact::PhysicalArtifact, SbomGenerationError> {}
        }
    }

    #[async_trait]
    impl IArtifactRetriever for MockArtifactRetriever {
        async fn get_physical_artifact(&self, artifact_id: &PhysicalArtifactId) -> Result<artifact::domain::physical_artifact::PhysicalArtifact, SbomGenerationError> {
            self.get_physical_artifact(artifact_id).await
        }
    }

    #[tokio::test]
    async fn test_execute_success() {
        // Arrange
        let mut generator = MockSbomGenerator::new();
        let mut repository = MockSbomRepository::new();
        let mut artifact_retriever = MockArtifactRetriever::new();
        
        let artifact_ref = ArtifactReference {
            artifact_hrn: PhysicalArtifactId::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234").unwrap(),
            artifact_type: ArtifactType::Primary,
            role: None,
        };
        
        let artifact = artifact::domain::physical_artifact::PhysicalArtifact {
            hrn: "hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234".parse().unwrap(),
            organization_hrn: shared::hrn::OrganizationId::new("123456789012").unwrap(),
            content_hash: shared::models::ContentHash {
                algorithm: shared::enums::HashAlgorithm::Sha256,
                value: "abcd1234".to_string(),
            },
            size_in_bytes: 1024,
            checksums: std::collections::HashMap::new(),
            storage_location: "s3://test-bucket/artifacts/abcd1234".to_string(),
            lifecycle: shared::lifecycle::Lifecycle::new(
                "hrn:hodei:iam:us-east-1:123456789012:user/test-user".parse().unwrap()
            ),
        };
        
        let expected_sbom = Sbom {
            id: shared::hrn::Hrn::new(&format!("hrn:hodei:supply-chain:us-east-1:123456789012:sbom/{}", Uuid::new_v4())).unwrap(),
            artifact_id: artifact.hrn.clone(),
            format: SbomFormat::CycloneDX,
            spec_version: "1.4".to_string(),
            content: "{}".to_string(),
            created_at: Utc::now(),
            tools: vec![
                ToolInformation {
                    vendor: "test".to_string(),
                    name: "test-generator".to_string(),
                    version: "1.0.0".to_string(),
                }
            ],
            authors: vec![],
            serial_number: "test-serial".to_string(),
            document_name: "test-document".to_string(),
            document_namespace: "test-namespace".to_string(),
            external_references: vec![],
            data_license: "CC0-1.0".to_string(),
        };
        
        artifact_retriever.expect_get_physical_artifact()
            .returning(move |_| Ok(artifact.clone()));
            
        generator.expect_generate()
            .returning(move |_, _| Ok(expected_sbom.clone()));
            
        repository.expect_save()
            .returning(|_| Ok(()));
            
        let use_case = GenerateSbomUseCase::new(
            Arc::new(generator),
            Arc::new(repository),
            Arc::new(artifact_retriever),
        );

        // Act
        let result = use_case.execute(&artifact_ref).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_artifact_retrieval_failure() {
        // Arrange
        let mut artifact_retriever = MockArtifactRetriever::new();
        let generator = MockSbomGenerator::new();
        let repository = MockSbomRepository::new();
        
        let artifact_ref = ArtifactReference {
            artifact_hrn: PhysicalArtifactId::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234").unwrap(),
            artifact_type: ArtifactType::Primary,
            role: None,
        };
        
        artifact_retriever.expect_get_physical_artifact()
            .returning(|_| Err(SbomGenerationError::ArtifactNotFound("Artifact not found".to_string())));
            
        let use_case = GenerateSbomUseCase::new(
            Arc::new(generator),
            Arc::new(repository),
            Arc::new(artifact_retriever),
        );

        // Act
        let result = use_case.execute(&artifact_ref).await;

        // Assert
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SbomGenerationError::ArtifactNotFound(_)));
    }

    #[tokio::test]
    async fn test_execute_generator_failure() {
        // Arrange
        let mut generator = MockSbomGenerator::new();
        let mut artifact_retriever = MockArtifactRetriever::new();
        let repository = MockSbomRepository::new();
        
        let artifact_ref = ArtifactReference {
            artifact_hrn: PhysicalArtifactId::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234").unwrap(),
            artifact_type: ArtifactType::Primary,
            role: None,
        };
        
        let artifact = artifact::domain::physical_artifact::PhysicalArtifact {
            hrn: "hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234".parse().unwrap(),
            organization_hrn: shared::hrn::OrganizationId::new("123456789012").unwrap(),
            content_hash: shared::models::ContentHash {
                algorithm: shared::enums::HashAlgorithm::Sha256,
                value: "abcd1234".to_string(),
            },
            size_in_bytes: 1024,
            checksums: std::collections::HashMap::new(),
            storage_location: "s3://test-bucket/artifacts/abcd1234".to_string(),
            lifecycle: shared::lifecycle::Lifecycle::new(
                "hrn:hodei:iam:us-east-1:123456789012:user/test-user".parse().unwrap()
            ),
        };
        
        artifact_retriever.expect_get_physical_artifact()
            .returning(move |_| Ok(artifact.clone()));
        
        generator.expect_generate()
            .returning(|_, _| Err(SbomGenerationError::GenerationFailed("Failed to generate SBOM".to_string())));
            
        let use_case = GenerateSbomUseCase::new(
            Arc::new(generator),
            Arc::new(repository),
            Arc::new(artifact_retriever),
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
        let mut artifact_retriever = MockArtifactRetriever::new();
        
        let artifact_ref = ArtifactReference {
            artifact_hrn: PhysicalArtifactId::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234").unwrap(),
            artifact_type: ArtifactType::Primary,
            role: None,
        };
        
        let artifact = artifact::domain::physical_artifact::PhysicalArtifact {
            hrn: "hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-abcd1234".parse().unwrap(),
            organization_hrn: shared::hrn::OrganizationId::new("123456789012").unwrap(),
            content_hash: shared::models::ContentHash {
                algorithm: shared::enums::HashAlgorithm::Sha256,
                value: "abcd1234".to_string(),
            },
            size_in_bytes: 1024,
            checksums: std::collections::HashMap::new(),
            storage_location: "s3://test-bucket/artifacts/abcd1234".to_string(),
            lifecycle: shared::lifecycle::Lifecycle::new(
                "hrn:hodei:iam:us-east-1:123456789012:user/test-user".parse().unwrap()
            ),
        };
        
        let sbom = Sbom {
            id: shared::hrn::Hrn::new(&format!("hrn:hodei:supply-chain:us-east-1:123456789012:sbom/{}", Uuid::new_v4())).unwrap(),
            artifact_id: artifact.hrn.clone(),
            format: SbomFormat::CycloneDX,
            spec_version: "1.4".to_string(),
            content: "{}".to_string(),
            created_at: Utc::now(),
            tools: vec![
                ToolInformation {
                    vendor: "test".to_string(),
                    name: "test-generator".to_string(),
                    version: "1.0.0".to_string(),
                }
            ],
            authors: vec![],
            serial_number: "test-serial".to_string(),
            document_name: "test-document".to_string(),
            document_namespace: "test-namespace".to_string(),
            external_references: vec![],
            data_license: "CC0-1.0".to_string(),
        };
        
        artifact_retriever.expect_get_physical_artifact()
            .returning(move |_| Ok(artifact.clone()));
            
        generator.expect_generate()
            .returning(move |_, _| Ok(sbom.clone()));
            
        repository.expect_save()
            .returning(|_| Err(SbomRepositoryError::SaveError("Failed to save SBOM".to_string())));
            
        let use_case = GenerateSbomUseCase::new(
            Arc::new(generator),
            Arc::new(repository),
            Arc::new(artifact_retriever),
        );

        // Act
        let result = use_case.execute(&artifact_ref).await;

        // Assert
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SbomGenerationError::RepositoryError(_)));
    }
}