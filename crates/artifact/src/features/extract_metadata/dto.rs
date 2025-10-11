use crate::domain::package_version::{ArtifactDependency, PackageMetadata};
use shared::hrn::Hrn;
use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;

/// Command to trigger metadata extraction for a package version
pub struct ExtractMetadataCommand {
    pub package_version_hrn: Hrn,
    pub artifact_storage_path: String,
    pub artifact_type: String, // "maven", "npm", etc.
}

impl ActionTrait for ExtractMetadataCommand {
    fn name() -> &'static str {
        "ExtractMetadata"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("artifact").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Artifact::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Artifact::Package".to_string()
    }
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
