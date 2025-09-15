use crate::domain::package_version::PackageCoordinates;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use shared::hrn::Hrn;

/// Command to trigger artifact validation
pub struct ValidateArtifactCommand {
    pub package_version_hrn: Hrn,
    pub artifact_storage_path: String,
    pub artifact_type: String, // "maven", "npm", etc.
    pub coordinates: PackageCoordinates,
    pub content_length: u64,
}

/// Result of artifact validation
pub struct ValidationResult {
    pub package_version_hrn: Hrn,
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Validation rule definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub priority: u32,
    pub artifact_types: Vec<String>, // "maven", "npm", "*" for all
    pub rule_type: ValidationRuleType,
}

/// Type of validation rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationRuleType {
    /// Check that JAR files are signed
    JarSignatureRequired,

    /// Check that package.json has a license field
    NpmLicenseRequired,

    /// Check file size limits
    SizeLimit { max_size_bytes: u64 },

    /// Custom validation rule
    Custom {
        script_path: String,
        parameters: std::collections::HashMap<String, String>,
    },
}

/// Validation context for rule execution
pub struct ValidationContext {
    pub artifact_content: Bytes,
    pub artifact_storage_path: String,
    pub artifact_type: String,
    pub coordinates: PackageCoordinates,
    pub content_length: u64,
}

/// Outcome of a validation rule execution
#[derive(Debug)]
pub struct RuleValidationOutcome {
    pub rule_id: String,
    pub passed: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}
