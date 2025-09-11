use crate::features::upload_artifact::test_adapter::MockVersionValidator;
use crate::features::upload_artifact::ports::{VersionValidator, ParsedVersion};

#[tokio::test]
async fn test_mock_version_validator_success() {
    let validator = MockVersionValidator::new();
    
    // Test successful validation
    let result = validator.validate_version("1.0.0").await;
    assert!(result.is_ok(), "Mock validator should succeed by default");
    
    // Test successful parsing
    let result = validator.parse_version("1.0.0").await;
    assert!(result.is_ok(), "Mock validator should parse successfully by default");
    
    let parsed = result.unwrap();
    assert_eq!(parsed.original, "1.0.0");
    assert_eq!(parsed.major, 1);
    assert_eq!(parsed.minor, 0);
    assert_eq!(parsed.patch, 0);
    assert_eq!(parsed.prerelease, None);
    assert_eq!(parsed.build_metadata, None);
    assert_eq!(parsed.is_snapshot, false);
}

#[tokio::test]
async fn test_mock_version_validator_failure() {
    let validator = MockVersionValidator::new().with_failure("Mock validation failed");
    
    // Test failed validation
    let result = validator.validate_version("1.0.0").await;
    assert!(result.is_err(), "Mock validator should fail when configured to fail");
    assert_eq!(result.err().unwrap(), "Mock validation failed");
    
    // Test failed parsing
    let result = validator.parse_version("1.0.0").await;
    assert!(result.is_err(), "Mock validator should fail parsing when configured to fail");
    assert_eq!(result.err().unwrap(), "Mock validation failed");
}

#[tokio::test]
async fn test_mock_version_validator_empty_version() {
    let validator = MockVersionValidator::new();
    
    // Test empty version validation
    let result = validator.validate_version("").await;
    assert!(result.is_err(), "Empty version should fail validation");
    
    // Test empty version parsing
    let result = validator.parse_version("").await;
    assert!(result.is_err(), "Empty version should fail parsing");
}

#[tokio::test]
async fn test_mock_version_validator_snapshot_detection() {
    let validator = MockVersionValidator::new();
    
    // Test SNAPSHOT version detection
    let result = validator.parse_version("1.0.0-SNAPSHOT").await;
    assert!(result.is_ok(), "SNAPSHOT version should parse successfully");
    
    let parsed = result.unwrap();
    assert!(parsed.is_snapshot, "SNAPSHOT version should be detected as snapshot");
    assert_eq!(parsed.original, "1.0.0-SNAPSHOT");
}

#[tokio::test]
async fn test_mock_version_validator_multipart_version() {
    let validator = MockVersionValidator::new();
    
    // Test multi-part version parsing
    let result = validator.parse_version("1.2.3").await;
    assert!(result.is_ok(), "Multi-part version should parse successfully");
    
    let parsed = result.unwrap();
    assert_eq!(parsed.major, 1);
    assert_eq!(parsed.minor, 2);
    assert_eq!(parsed.patch, 3);
}

#[tokio::test]
async fn test_mock_version_validator_insufficient_parts() {
    let validator = MockVersionValidator::new();
    
    // Test version with insufficient parts
    let result = validator.parse_version("1.2").await;
    assert!(result.is_err(), "Version with insufficient parts should fail parsing");
    
    let result = validator.parse_version("1").await;
    assert!(result.is_err(), "Single part version should fail parsing");
}