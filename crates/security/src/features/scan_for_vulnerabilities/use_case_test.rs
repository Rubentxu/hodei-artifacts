#[cfg(test)]
mod tests {
    use crate::features::scan_for_vulnerabilities::use_case::ScanForVulnerabilitiesUseCase;
    use artifact::domain::physical_artifact::PhysicalArtifact;
    use crate::features::scan_for_vulnerabilities::ports::{IScanner, IScannerProvider, IVulnerabilityRepository, IPhysicalArtifactRepository, ScanError, RepositoryError};
    use std::sync::Arc;
    use shared::hrn::{Hrn, OrganizationId, PhysicalArtifactId};
    use shared::lifecycle::Lifecycle;
    use shared::models::{ContentHash, ArtifactReference};
    use shared::enums::{HashAlgorithm, ArtifactType};
    use std::collections::HashMap;
    use crate::domain::vulnerability::{Vulnerability, Severity, VulnerabilityReport};
    use mockall::mock;
    use async_trait::async_trait;

    mock! {
        pub Scanner {
            async fn scan(&self, artifact: &PhysicalArtifact) -> Result<Vec<Vulnerability>, ScanError> {}
            fn name(&self) -> &'static str {}
        }
    }

    impl Clone for MockScanner {
        fn clone(&self) -> Self {
            let mut new = MockScanner::new();
            new.expect_scan().returning(|_| Ok(vec![]));
            new.expect_name().returning(|| "mock");
            new
        }
    }

    #[async_trait]
    impl IScanner for MockScanner {
        async fn scan(&self, artifact: &PhysicalArtifact) -> Result<Vec<Vulnerability>, ScanError> {
            self.scan(artifact).await
        }
        fn name(&self) -> &'static str {
            self.name()
        }
    }

    mock! {
        pub ScannerProvider {
            fn scanners_for(&self, artifact: &PhysicalArtifact) -> Vec<Arc<dyn IScanner>> {}
        }
    }

    #[async_trait]
    impl IScannerProvider for MockScannerProvider {
        fn scanners_for(&self, artifact: &PhysicalArtifact) -> Vec<Arc<dyn IScanner>> {
            self.scanners_for(artifact)
        }
    }

    mock! {
        pub VulnerabilityRepository {
            async fn save_report(&self, report: &VulnerabilityReport) -> Result<(), RepositoryError> {}
        }
    }

    #[async_trait]
    impl IVulnerabilityRepository for MockVulnerabilityRepository {
        async fn save_report(&self, report: &VulnerabilityReport) -> Result<(), RepositoryError> {
            self.save_report(report).await
        }
    }

    mock! {
        pub PhysicalArtifactRepository {
            async fn get_by_hrn(&self, hrn: &PhysicalArtifactId) -> Result<PhysicalArtifact, RepositoryError> {}
        }
    }

    #[async_trait]
    impl IPhysicalArtifactRepository for MockPhysicalArtifactRepository {
        async fn get_by_hrn(&self, hrn: &PhysicalArtifactId) -> Result<PhysicalArtifact, RepositoryError> {
            self.get_by_hrn(hrn).await
        }
    }

    #[tokio::test]
    async fn test_execute_success() {
        // Arrange
        let mut scanner_provider = MockScannerProvider::new();
        let mut repository = MockVulnerabilityRepository::new();
        let mut artifact_repository = MockPhysicalArtifactRepository::new();
        let mut scanner = MockScanner::new();

        let creator_hrn = Hrn::new("hrn:hodei:iam:us-east-1:123456789012:user/test-user").unwrap();
        let artifact = PhysicalArtifact {
            hrn: Hrn::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-123").unwrap(),
            organization_hrn: OrganizationId::new("123456789012").unwrap(),
            content_hash: ContentHash { algorithm: HashAlgorithm::Sha256, value: "123".to_string() },
            size_in_bytes: 100,
            checksums: HashMap::new(),
            storage_location: "s3://bucket/key.oci".to_string(),
            lifecycle: Lifecycle::new(creator_hrn),
        };

        let artifact_ref = ArtifactReference {
            artifact_hrn: PhysicalArtifactId::new("hrn:hodei:artifact:us-east-1:123456789012:physical-artifact/sha256-123").unwrap(),
            artifact_type: ArtifactType::ContainerImage,
            role: None,
        };

        artifact_repository.expect_get_by_hrn()
            .returning(move |_| Ok(artifact.clone()));

        scanner.expect_scan()
            .returning(|_| Ok(vec![Vulnerability { id: "CVE-123".to_string(), package_name: "test".to_string(), package_version: "1.0".to_string(), severity: Severity::High, description: "test".to_string(), source: "mock".to_string() }]));
        scanner.expect_name().returning(|| "mock");

        scanner_provider.expect_scanners_for()
            .returning(move |_| vec![Arc::new(scanner.clone())]);

        repository.expect_save_report()
            .times(1)
            .returning(|_| Ok(()));

        let use_case = ScanForVulnerabilitiesUseCase::new(
            Arc::new(scanner_provider),
            Arc::new(repository),
            Arc::new(artifact_repository),
        );

        // Act
        let result = use_case.execute(&artifact_ref).await;

        // Assert
        assert!(result.is_ok());
    }
}
