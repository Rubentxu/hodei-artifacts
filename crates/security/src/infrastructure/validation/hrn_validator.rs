// crates/security/src/infrastructure/validation/hrn_validator.rs

use crate::infrastructure::errors::SecurityError;
use shared::hrn::Hrn;
use std::collections::HashSet;

/// Comprehensive HRN validator for all Hodei resource types
pub struct HrnValidator {
    /// Set of supported resource types across all domains
    supported_resource_types: HashSet<String>,
    /// Set of supported services
    supported_services: HashSet<String>,
}

impl HrnValidator {
    /// Create a new HRN validator with all supported resource types
    pub fn new() -> Self {
        let mut supported_resource_types = HashSet::new();
        let mut supported_services = HashSet::new();

        // IAM Domain Resources
        supported_resource_types.insert("user".to_string());
        supported_resource_types.insert("policy".to_string());
        supported_resource_types.insert("api-key".to_string());
        supported_resource_types.insert("service-account".to_string());
        supported_services.insert("iam".to_string());

        // Organization Domain Resources
        supported_resource_types.insert("organization".to_string());
        supported_resource_types.insert("team".to_string());
        supported_resource_types.insert("membership".to_string());
        supported_services.insert("organization".to_string());

        // Artifact Domain Resources
        supported_resource_types.insert("repository".to_string());
        supported_resource_types.insert("physical-artifact".to_string());
        supported_resource_types.insert("package-version".to_string());
        supported_resource_types.insert("artifact-metadata".to_string());
        supported_services.insert("artifact".to_string());

        // Configuration Domain Resources
        supported_resource_types.insert("configuration".to_string());
        supported_resource_types.insert("config-template".to_string());
        supported_resource_types.insert("config-version".to_string());
        supported_services.insert("config".to_string());

        // Events Domain Resources
        supported_resource_types.insert("event".to_string());
        supported_resource_types.insert("event-stream".to_string());
        supported_resource_types.insert("event-subscription".to_string());
        supported_services.insert("events".to_string());

        // Analytics Domain Resources
        supported_resource_types.insert("metric".to_string());
        supported_resource_types.insert("dashboard".to_string());
        supported_resource_types.insert("report".to_string());
        supported_resource_types.insert("alert".to_string());
        supported_services.insert("analytics".to_string());

        // Supply Chain Domain Resources
        supported_resource_types.insert("attestation".to_string());
        supported_resource_types.insert("public-key".to_string());
        supported_resource_types.insert("scan-result".to_string());
        supported_resource_types.insert("vulnerability-definition".to_string());
        supported_resource_types.insert("vulnerability-occurrence".to_string());
        supported_resource_types.insert("provenance-record".to_string());
        supported_services.insert("supply-chain".to_string());

        // Storage Domain Resources
        supported_resource_types.insert("storage-backend".to_string());
        supported_resource_types.insert("storage-bucket".to_string());
        supported_resource_types.insert("storage-policy".to_string());
        supported_services.insert("storage".to_string());

        // Monitoring Domain Resources
        supported_resource_types.insert("monitor".to_string());
        supported_resource_types.insert("health-check".to_string());
        supported_resource_types.insert("log-stream".to_string());
        supported_services.insert("monitoring".to_string());

        // System Resources
        supported_resource_types.insert("system".to_string());
        supported_services.insert("system".to_string());

        Self {
            supported_resource_types,
            supported_services,
        }
    }

    /// Validate that an HRN is properly formatted and references a supported resource type
    pub fn validate_hrn(&self, hrn_str: &str) -> Result<(), SecurityError> {
        // First validate basic HRN format
        let hrn = Hrn::new(hrn_str)
            .map_err(|e| SecurityError::ValidationError(format!("Invalid HRN format '{}': {}", hrn_str, e)))?;

        // Parse HRN components: hrn:hodei:service:region:account:resource-type/resource-id
        let parts: Vec<&str> = hrn.as_str().split(':').collect();
        if parts.len() < 6 {
            return Err(SecurityError::ValidationError(format!(
                "HRN '{}' does not have enough components. Expected format: hrn:hodei:service:region:account:resource-type/resource-id",
                hrn_str
            )));
        }

        let service = parts[2];
        let resource_part = parts[5];
        let resource_type = resource_part.split('/').next().unwrap_or("");

        // Validate service is supported
        if !self.supported_services.contains(service) {
            return Err(SecurityError::ValidationError(format!(
                "Unsupported service '{}' in HRN '{}'. Supported services: {}",
                service,
                hrn_str,
                self.supported_services.iter().cloned().collect::<Vec<_>>().join(", ")
            )));
        }

        // Validate resource type is supported
        if !self.supported_resource_types.contains(resource_type) {
            return Err(SecurityError::ValidationError(format!(
                "Unsupported resource type '{}' in HRN '{}'. Supported types: {}",
                resource_type,
                hrn_str,
                self.supported_resource_types.iter().cloned().collect::<Vec<_>>().join(", ")
            )));
        }

        // Validate service-resource type compatibility
        self.validate_service_resource_compatibility(service, resource_type, hrn_str)?;

        Ok(())
    }

    /// Validate that a service and resource type combination is valid
    fn validate_service_resource_compatibility(&self, service: &str, resource_type: &str, hrn_str: &str) -> Result<(), SecurityError> {
        let valid_combinations = match service {
            "iam" => vec!["user", "policy", "api-key", "service-account"],
            "organization" => vec!["organization", "team", "membership"],
            "artifact" => vec!["repository", "physical-artifact", "package-version", "artifact-metadata"],
            "config" => vec!["configuration", "config-template", "config-version"],
            "events" => vec!["event", "event-stream", "event-subscription"],
            "analytics" => vec!["metric", "dashboard", "report", "alert"],
            "supply-chain" => vec!["attestation", "public-key", "scan-result", "vulnerability-definition", "vulnerability-occurrence", "provenance-record"],
            "storage" => vec!["storage-backend", "storage-bucket", "storage-policy"],
            "monitoring" => vec!["monitor", "health-check", "log-stream"],
            "system" => vec!["system", "user", "organization"], // System can reference cross-domain resources
            _ => return Err(SecurityError::ValidationError(format!("Unknown service '{}'", service))),
        };

        if !valid_combinations.contains(&resource_type) {
            return Err(SecurityError::ValidationError(format!(
                "Invalid resource type '{}' for service '{}' in HRN '{}'. Valid types for {}: {}",
                resource_type,
                service,
                hrn_str,
                service,
                valid_combinations.join(", ")
            )));
        }

        Ok(())
    }

    /// Extract all HRNs from a Cedar policy content
    pub fn extract_hrns_from_policy(&self, policy_content: &str) -> Result<Vec<String>, SecurityError> {
        use regex::Regex;

        let hrn_regex = Regex::new(r#""(hrn:[^"]+)""#)
            .map_err(|e| SecurityError::ValidationError(format!("Regex error: {}", e)))?;

        let mut hrns = Vec::new();
        for captures in hrn_regex.captures_iter(policy_content) {
            if let Some(hrn_match) = captures.get(1) {
                hrns.push(hrn_match.as_str().to_string());
            }
        }

        Ok(hrns)
    }

    /// Validate all HRNs found in a Cedar policy
    pub fn validate_policy_hrns(&self, policy_content: &str) -> Result<(), SecurityError> {
        let hrns = self.extract_hrns_from_policy(policy_content)?;

        for hrn in hrns {
            self.validate_hrn(&hrn)?;
        }

        Ok(())
    }

    /// Get all supported resource types
    pub fn get_supported_resource_types(&self) -> Vec<String> {
        self.supported_resource_types.iter().cloned().collect()
    }

    /// Get all supported services
    pub fn get_supported_services(&self) -> Vec<String> {
        self.supported_services.iter().cloned().collect()
    }
}

impl Default for HrnValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_iam_hrns() {
        let validator = HrnValidator::new();

        let valid_hrns = vec![
            "hrn:hodei:iam:global:system:user/alice",
            "hrn:hodei:iam:global:system:policy/read-artifacts",
            "hrn:hodei:iam:global:system:api-key/key-123",
            "hrn:hodei:iam:global:system:service-account/ci-service",
        ];

        for hrn in valid_hrns {
            assert!(validator.validate_hrn(hrn).is_ok(), "HRN should be valid: {}", hrn);
        }
    }

    #[test]
    fn test_validate_valid_artifact_hrns() {
        let validator = HrnValidator::new();

        let valid_hrns = vec![
            "hrn:hodei:artifact:global:system:repository/myorg/myrepo",
            "hrn:hodei:artifact:global:system:physical-artifact/sha256-abc123",
            "hrn:hodei:artifact:global:system:package-version/myorg/myrepo/1.0.0",
        ];

        for hrn in valid_hrns {
            assert!(validator.validate_hrn(hrn).is_ok(), "HRN should be valid: {}", hrn);
        }
    }

    #[test]
    fn test_validate_invalid_service() {
        let validator = HrnValidator::new();
        let invalid_hrn = "hrn:hodei:unknown-service:global:system:user/alice";

        let result = validator.validate_hrn(invalid_hrn);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported service"));
    }

    #[test]
    fn test_validate_invalid_resource_type() {
        let validator = HrnValidator::new();
        let invalid_hrn = "hrn:hodei:iam:global:system:unknown-resource/test";

        let result = validator.validate_hrn(invalid_hrn);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported resource type"));
    }

    #[test]
    fn test_validate_incompatible_service_resource() {
        let validator = HrnValidator::new();
        let invalid_hrn = "hrn:hodei:iam:global:system:repository/test"; // repository should be in artifact service

        let result = validator.validate_hrn(invalid_hrn);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid resource type"));
    }

    #[test]
    fn test_extract_hrns_from_policy() {
        let validator = HrnValidator::new();
        let policy = r#"
            permit(
                principal == "hrn:hodei:iam:global:system:user/alice",
                action == ReadRepository,
                resource == "hrn:hodei:artifact:global:system:repository/myorg/myrepo"
            );
        "#;

        let hrns = validator.extract_hrns_from_policy(policy).unwrap();
        assert_eq!(hrns.len(), 2);
        assert!(hrns.contains(&"hrn:hodei:iam:global:system:user/alice".to_string()));
        assert!(hrns.contains(&"hrn:hodei:artifact:global:system:repository/myorg/myrepo".to_string()));
    }

    #[test]
    fn test_validate_policy_hrns() {
        let validator = HrnValidator::new();
        let valid_policy = r#"
            permit(
                principal == "hrn:hodei:iam:global:system:user/alice",
                action == ReadRepository,
                resource == "hrn:hodei:artifact:global:system:repository/myorg/myrepo"
            );
        "#;

        assert!(validator.validate_policy_hrns(valid_policy).is_ok());

        let invalid_policy = r#"
            permit(
                principal == "hrn:hodei:unknown:global:system:user/alice",
                action == ReadRepository,
                resource == "hrn:hodei:artifact:global:system:repository/myorg/myrepo"
            );
        "#;

        assert!(validator.validate_policy_hrns(invalid_policy).is_err());
    }
}