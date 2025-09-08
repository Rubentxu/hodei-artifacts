use shared::hrn::Hrn;
use crate::domain::package_version::{PackageMetadata, ArtifactDependency};

/// Command to trigger metadata extraction for a package version
pub struct ExtractMetadataCommand {
    pub package_version_hrn: Hrn,
    pub artifact_storage_path: String,
    pub artifact_type: String, // "maven", "npm", etc.
}

/// Result of metadata extraction process
pub struct MetadataExtractionResult {
    pub package_version_hrn: Hrn,
    pub extracted_metadata: PackageMetadata,
    pub extracted_dependencies: Vec<ArtifactDependency>,
}

/// Parsed Maven metadata from pom.xml
#[derive(Debug, Clone)]
pub struct ParsedMavenMetadata {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub description: Option<String>,
    pub licenses: Vec<String>,
    pub dependencies: Vec<MavenDependency>,
}

/// Parsed NPM metadata from package.json
#[derive(Debug, Clone)]
pub struct ParsedNpmMetadata {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub licenses: Vec<String>,
    pub dependencies: Vec<NpmDependency>,
}

/// Maven dependency representation
#[derive(Debug, Clone)]
pub struct MavenDependency {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub scope: String,
}

/// NPM dependency representation
#[derive(Debug, Clone)]
pub struct NpmDependency {
    pub name: String,
    pub version: String,
    pub is_dev_dependency: bool,
}