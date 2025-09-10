use std::sync::Arc;
use async_trait::async_trait;
use crate::domain::vulnerability::{Vulnerability, VulnerabilityReport, Severity};
use crate::features::scan_for_vulnerabilities::ports::{
    IScanner, IScannerProvider, IVulnerabilityRepository, ScanError, RepositoryError
};
use artifact::domain::physical_artifact::PhysicalArtifact;
use mongodb::Collection;

// --- Adaptador para Trivy ---
#[derive(Clone)]
pub struct TrivyScannerAdapter;

#[async_trait]
impl IScanner for TrivyScannerAdapter {
    async fn scan(&self, _artifact: &PhysicalArtifact) -> Result<Vec<Vulnerability>, ScanError> {
        // Lógica para llamar a Trivy y normalizar la salida a Vec<Vulnerability>
        Ok(vec![
            Vulnerability {
                id: "CVE-2021-44228".to_string(),
                package_name: "log4j".to_string(),
                package_version: "2.14.1".to_string(),
                severity: Severity::Critical,
                description: "Log4Shell vulnerability".to_string(),
                source: "Trivy".to_string(),
            }
        ])
    }
    fn name(&self) -> &'static str { "Trivy" }
}

// --- Adaptador para Cargo Audit ---
#[derive(Clone)]
pub struct CargoAuditScannerAdapter;

#[async_trait]
impl IScanner for CargoAuditScannerAdapter {
    async fn scan(&self, _artifact: &PhysicalArtifact) -> Result<Vec<Vulnerability>, ScanError> {
        Ok(vec![]) // No vulnerabilities found for simplicity
    }
    fn name(&self) -> &'static str { "Cargo-Audit" }
}

// --- Implementación del Proveedor de Escáneres ---
pub struct DefaultScannerProvider {
    scanners: Vec<Arc<dyn IScanner>>,
}

impl DefaultScannerProvider {
    pub fn new(scanners: Vec<Arc<dyn IScanner>>) -> Self {
        Self { scanners }
    }
}

impl IScannerProvider for DefaultScannerProvider {
    fn scanners_for(&self, artifact: &PhysicalArtifact) -> Vec<Arc<dyn IScanner>> {
        self.scanners.iter()
            .filter(|scanner| {
                match scanner.name() {
                    "Trivy" => artifact.storage_location.ends_with(".oci"),
                    "Cargo-Audit" => artifact.storage_location.ends_with(".crate"),
                    _ => false
                }
            })
            .cloned()
            .collect()
    }
}

// --- Adaptador para el Repositorio en MongoDB ---
pub struct MongoVulnerabilityRepository {
    collection: Collection<VulnerabilityReport>,
}

impl MongoVulnerabilityRepository {
    pub fn new(collection: Collection<VulnerabilityReport>) -> Self {
        Self { collection }
    }
}

#[async_trait]
impl IVulnerabilityRepository for MongoVulnerabilityRepository {
    async fn save_report(&self, report: &VulnerabilityReport) -> Result<(), RepositoryError> {
        self.collection.insert_one(report).await
            .map_err(|e| RepositoryError::SaveError(e.to_string()))?;
        Ok(())
    }
}
