use crate::domain::vulnerability::VulnerabilityReport;
use crate::features::scan_for_vulnerabilities::ports::{IScannerProvider, IVulnerabilityRepository, ScanError};
use std::sync::Arc;
use futures::future::join_all;
use artifact::domain::physical_artifact::PhysicalArtifact;
use uuid::Uuid;

pub struct ScanForVulnerabilitiesUseCase {
    scanner_provider: Arc<dyn IScannerProvider>,
    repository: Arc<dyn IVulnerabilityRepository>,
}

impl ScanForVulnerabilitiesUseCase {
    pub fn new(scanner_provider: Arc<dyn IScannerProvider>, repository: Arc<dyn IVulnerabilityRepository>) -> Self {
        Self { scanner_provider, repository }
    }

    pub async fn execute(&self, artifact: PhysicalArtifact) -> Result<(), ScanError> {
        let scanners = self.scanner_provider.scanners_for(&artifact);
        
        // Ejecutar escaneos en paralelo
        let scan_futures = scanners.iter().map(|s| s.scan(&artifact));
        let results = join_all(scan_futures).await;

        // Procesar y consolidar resultados
        let mut all_vulnerabilities = vec![];
        for result in results {
            if let Ok(vulnerabilities) = result {
                all_vulnerabilities.push(vulnerabilities);
            } else {
                // Manejar error de un escáner individual
            }
        }
        
        let consolidated = VulnerabilityReport::consolidate(all_vulnerabilities);

        // Crear y guardar el informe final
        let report = VulnerabilityReport {
            artifact_id: artifact.hrn.to_string(),
            report_id: Uuid::new_v4().to_string(),
            status: crate::domain::vulnerability::ScanStatus::Completed,
            vulnerabilities: consolidated,
            created_at: chrono::Utc::now(),
        };
        use crate::domain::vulnerability::VulnerabilityReport;
use crate::features::scan_for_vulnerabilities::ports::{IScannerProvider, IVulnerabilityRepository, IPhysicalArtifactRepository, ScanError};
use std::sync::Arc;
use futures::future::join_all;
use shared::models::ArtifactReference;
use uuid::Uuid;

pub struct ScanForVulnerabilitiesUseCase {
    scanner_provider: Arc<dyn IScannerProvider>,
    repository: Arc<dyn IVulnerabilityRepository>,
    artifact_repository: Arc<dyn IPhysicalArtifactRepository>,
}

impl ScanForVulnerabilitiesUseCase {
    pub fn new(
        scanner_provider: Arc<dyn IScannerProvider>,
        repository: Arc<dyn IVulnerabilityRepository>,
        artifact_repository: Arc<dyn IPhysicalArtifactRepository>,
    ) -> Self {
        Self { scanner_provider, repository, artifact_repository }
    }

    pub async fn execute(&self, artifact_ref: &ArtifactReference) -> Result<(), ScanError> {
        let artifact = self.artifact_repository.get_by_hrn(&artifact_ref.artifact_hrn).await.map_err(|e| ScanError::ExecutionError(e.to_string()))?;
        let scanners = self.scanner_provider.scanners_for(&artifact);
        
        // Ejecutar escaneos en paralelo
        let scan_futures = scanners.iter().map(|s| s.scan(&artifact));
        let results = join_all(scan_futures).await;

        // Procesar y consolidar resultados
        let mut all_vulnerabilities = vec![];
        for result in results {
            if let Ok(vulnerabilities) = result {
                all_vulnerabilities.push(vulnerabilities);
            } else {
                // Manejar error de un escáner individual
            }
        }
        
        let consolidated = VulnerabilityReport::consolidate(all_vulnerabilities);

        // Crear y guardar el informe final
        let report = VulnerabilityReport {
            artifact_id: artifact.hrn.to_string(),
            report_id: Uuid::new_v4().to_string(),
            status: crate::domain::vulnerability::ScanStatus::Completed,
            vulnerabilities: consolidated,
            created_at: chrono::Utc::now(),
        };
        self.repository.save_report(&report).await.map_err(|e| ScanError::ExecutionError(e.to_string()))?;

        Ok(())
    }
}


        Ok(())
    }
}
