// crates/iam/tests/hrn_validation_integration_test.rs

use iam::features::create_policy::adapter::CedarPolicyValidatorAdapter;
use iam::features::create_policy::ports::PolicyValidator;
use iam::infrastructure::validation::cedar_validator::CedarPolicyValidator;
use std::sync::Arc;

#[tokio::test]
async fn test_create_policy_with_valid_hrns() {
    let cedar_validator = Arc::new(CedarPolicyValidator::new());
    let adapter = CedarPolicyValidatorAdapter::new(cedar_validator);

    // Simple policy with valid HRNs that should pass HRN validation
    let valid_policy = r#"
        permit(
            principal == User::"hrn:hodei:iam:global:system:user/alice",
            action == ReadRepository,
            resource == Repository::"hrn:hodei:artifact:global:system:repository/myorg/myrepo"
        );
    "#;

    // Test HRN validation specifically
    let semantic_result = adapter.validate_semantics(valid_policy).await;
    
    // The policy should pass HRN validation (even if it fails Cedar schema validation)
    // We're testing that the HRNs are properly formatted and use supported resource types
    match semantic_result {
        Ok(_) => {
            // Great! HRNs are valid and policy passed all validation
        }
        Err(e) => {
            let error_msg = e.to_string();
            // If it fails, it should NOT be due to HRN validation errors
            assert!(
                !error_msg.contains("Unsupported service") && 
                !error_msg.contains("Unsupported resource type") &&
                !error_msg.contains("Invalid resource type"),
                "Policy should not fail HRN validation, but got: {}", error_msg
            );
        }
    }
}

#[tokio::test]
async fn test_create_policy_with_invalid_service_hrn() {
    let cedar_validator = Arc::new(CedarPolicyValidator::new());
    let adapter = CedarPolicyValidatorAdapter::new(cedar_validator);

    let invalid_policy = r#"
        permit(
            principal == User::"hrn:hodei:unknown-service:global:system:user/alice",
            action == Action::"ReadRepository",
            resource == Repository::"hrn:hodei:artifact:global:system:repository/myorg/myrepo"
        );
    "#;

    let result = adapter.validate_semantics(invalid_policy).await;
    assert!(result.is_err(), "Policy with invalid service HRN should fail semantic validation");

    let error = result.unwrap_err();
    let error_message = error.to_string();
    assert!(error_message.contains("Unsupported service"), "Error should mention unsupported service: {}", error_message);
}

#[tokio::test]
async fn test_create_policy_with_invalid_resource_type_hrn() {
    let cedar_validator = Arc::new(CedarPolicyValidator::new());
    let adapter = CedarPolicyValidatorAdapter::new(cedar_validator);

    let invalid_policy = r#"
        permit(
            principal == User::"hrn:hodei:iam:global:system:user/alice",
            action == Action::"ReadRepository",
            resource == Repository::"hrn:hodei:artifact:global:system:unknown-resource/test"
        );
    "#;

    let result = adapter.validate_semantics(invalid_policy).await;
    assert!(result.is_err(), "Policy with invalid resource type HRN should fail semantic validation");

    let error = result.unwrap_err();
    let error_message = error.to_string();
    assert!(error_message.contains("Unsupported resource type"), "Error should mention unsupported resource type: {}", error_message);
}

#[tokio::test]
async fn test_create_policy_with_incompatible_service_resource() {
    let cedar_validator = Arc::new(CedarPolicyValidator::new());
    let adapter = CedarPolicyValidatorAdapter::new(cedar_validator);

    let invalid_policy = r#"
        permit(
            principal == User::"hrn:hodei:iam:global:system:user/alice",
            action == Action::"ReadRepository",
            resource == Repository::"hrn:hodei:iam:global:system:repository/test"
        );
    "#;

    let result = adapter.validate_semantics(invalid_policy).await;
    assert!(result.is_err(), "Policy with incompatible service-resource combination should fail semantic validation");

    let error = result.unwrap_err();
    let error_message = error.to_string();
    assert!(error_message.contains("Invalid resource type"), "Error should mention invalid resource type for service: {}", error_message);
}

#[tokio::test]
async fn test_create_policy_with_multiple_valid_hrns() {
    let cedar_validator = Arc::new(CedarPolicyValidator::new());
    let adapter = CedarPolicyValidatorAdapter::new(cedar_validator);

    let valid_policy = r#"
        permit(
            principal == User::"hrn:hodei:iam:global:system:user/alice",
            action == Action::"ReadArtifact",
            resource == PhysicalArtifact::"hrn:hodei:artifact:global:system:physical-artifact/sha256-abc123"
        ) when {
            resource.owner == User::"hrn:hodei:iam:global:system:user/bob" &&
            context.organization == Organization::"hrn:hodei:organization:global:system:organization/myorg"
        };
    "#;

    let result = adapter.validate_semantics(valid_policy).await;
    assert!(result.is_ok(), "Policy with multiple valid HRNs should pass semantic validation: {:?}", result);
}

#[tokio::test]
async fn test_create_policy_with_supply_chain_hrns() {
    let cedar_validator = Arc::new(CedarPolicyValidator::new());
    let adapter = CedarPolicyValidatorAdapter::new(cedar_validator);

    let valid_policy = r#"
        permit(
            principal == ServiceAccount::"hrn:hodei:iam:global:system:service-account/scanner",
            action == Action::"CreateAttestation",
            resource == PhysicalArtifact::"hrn:hodei:artifact:global:system:physical-artifact/sha256-def456"
        ) when {
            context.scan_result == ScanResult::"hrn:hodei:supply-chain:global:system:scan-result/scan-123" &&
            context.public_key == PublicKey::"hrn:hodei:supply-chain:global:system:public-key/key-456"
        };
    "#;

    let result = adapter.validate_semantics(valid_policy).await;
    assert!(result.is_ok(), "Policy with supply chain HRNs should pass semantic validation: {:?}", result);
}

#[tokio::test]
async fn test_create_policy_with_analytics_hrns() {
    let cedar_validator = Arc::new(CedarPolicyValidator::new());
    let adapter = CedarPolicyValidatorAdapter::new(cedar_validator);

    let valid_policy = r#"
        permit(
            principal == User::"hrn:hodei:iam:global:system:user/analyst",
            action == Action::"ReadMetrics",
            resource == Dashboard::"hrn:hodei:analytics:global:system:dashboard/security-dashboard"
        ) when {
            principal.role == "analyst" &&
            context.organization == Organization::"hrn:hodei:organization:global:system:organization/myorg"
        };
    "#;

    let result = adapter.validate_semantics(valid_policy).await;
    assert!(result.is_ok(), "Policy with analytics HRNs should pass semantic validation: {:?}", result);
}

#[tokio::test]
async fn test_create_policy_with_configuration_hrns() {
    let cedar_validator = Arc::new(CedarPolicyValidator::new());
    let adapter = CedarPolicyValidatorAdapter::new(cedar_validator);

    let valid_policy = r#"
        permit(
            principal == User::"hrn:hodei:iam:global:system:user/devops",
            action == Action::"WriteConfiguration",
            resource == Configuration::"hrn:hodei:config:global:system:configuration/app-config"
        ) when {
            principal.role == "devops" &&
            resource.organization == Organization::"hrn:hodei:organization:global:system:organization/myorg"
        };
    "#;

    let result = adapter.validate_semantics(valid_policy).await;
    assert!(result.is_ok(), "Policy with configuration HRNs should pass semantic validation: {:?}", result);
}

#[tokio::test]
async fn test_create_policy_with_monitoring_hrns() {
    let cedar_validator = Arc::new(CedarPolicyValidator::new());
    let adapter = CedarPolicyValidatorAdapter::new(cedar_validator);

    let valid_policy = r#"
        permit(
            principal == ServiceAccount::"hrn:hodei:iam:global:system:service-account/monitoring",
            action == Action::"ReadMonitor",
            resource == HealthCheck::"hrn:hodei:monitoring:global:system:health-check/api-health"
        ) when {
            context.alert_level == "critical"
        };
    "#;

    let result = adapter.validate_semantics(valid_policy).await;
    assert!(result.is_ok(), "Policy with monitoring HRNs should pass semantic validation: {:?}", result);
}