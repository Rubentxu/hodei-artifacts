use crate::features::upload_artifact::adapter::DefaultVersionValidator;
use crate::features::upload_artifact::ports::{VersionValidator, ParsedVersion};

#[tokio::test]
async fn test_default_version_validator_valid_versions() {
    let validator = DefaultVersionValidator::new();
    
    // Test valid semantic versions
    let valid_versions = vec![
        "1.0.0",
        "2.1.3", 
        "0.1.0",
        "1.0.0-alpha",
        "1.0.0-beta.1",
        "1.0.0-rc.2",
        "1.0.0+build.1",
        "1.0.0-alpha+build.1",
        "1.0.0-SNAPSHOT",
        "2.3.4-snapshot",
    ];
    
    for version in valid_versions {
        let result = validator.validate_version(version).await;
        assert!(result.is_ok(), "Version {} should be valid: {:?}", version, result.err());
    }
}

#[tokio::test]
async fn test_default_version_validator_invalid_versions() {
    let validator = DefaultVersionValidator::new();
    
    // Test invalid semantic versions
    let invalid_versions = vec![
        "",
        "1.0",
        "1",
        "a.b.c",
        "1.0.0-",
        "1.0.0+",
        "1.0.0-+build",
    ];
    
    for version in invalid_versions {
        let result = validator.validate_version(version).await;
        assert!(result.is_err(), "Version {} should be invalid", version);
    }
}

#[tokio::test]
async fn test_default_version_validator_parse_valid_versions() {
    let validator = DefaultVersionValidator::new();
    
    // Test parsing valid versions
    let test_cases = vec![
        ("1.0.0", ParsedVersion {
            original: "1.0.0".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: None,
            build_metadata: None,
            is_snapshot: false,
        }),
        ("2.1.3", ParsedVersion {
            original: "2.1.3".to_string(),
            major: 2,
            minor: 1,
            patch: 3,
            prerelease: None,
            build_metadata: None,
            is_snapshot: false,
        }),
        ("1.0.0-alpha", ParsedVersion {
            original: "1.0.0-alpha".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: Some("alpha".to_string()),
            build_metadata: None,
            is_snapshot: false,
        }),
        ("1.0.0-beta.1", ParsedVersion {
            original: "1.0.0-beta.1".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: Some("beta.1".to_string()),
            build_metadata: None,
            is_snapshot: false,
        }),
        ("1.0.0+build.1", ParsedVersion {
            original: "1.0.0+build.1".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: None,
            build_metadata: Some("build.1".to_string()),
            is_snapshot: false,
        }),
        ("1.0.0-SNAPSHOT", ParsedVersion {
            original: "1.0.0-SNAPSHOT".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: None,
            build_metadata: None,
            is_snapshot: true,
        }),
    ];
    
    for (version_str, expected) in test_cases {
        let result = validator.parse_version(version_str).await;
        assert!(result.is_ok(), "Failed to parse version {}: {:?}", version_str, result.err());
        
        let parsed = result.unwrap();
        assert_eq!(parsed.original, expected.original);
        assert_eq!(parsed.major, expected.major);
        assert_eq!(parsed.minor, expected.minor);
        assert_eq!(parsed.patch, expected.patch);
        assert_eq!(parsed.prerelease, expected.prerelease);
        assert_eq!(parsed.build_metadata, expected.build_metadata);
        assert_eq!(parsed.is_snapshot, expected.is_snapshot);
    }
}

#[tokio::test]
async fn test_default_version_validator_parse_invalid_versions() {
    let validator = DefaultVersionValidator::new();
    
    // Test parsing invalid versions
    let invalid_versions = vec![
        "",
        "1.0",
        "1",
        "a.b.c",
    ];
    
    for version in invalid_versions {
        let result = validator.parse_version(version).await;
        assert!(result.is_err(), "Version {} should fail to parse", version);
    }
}

#[tokio::test]
async fn test_default_version_validator_case_insensitive_snapshot() {
    let validator = DefaultVersionValidator::new();
    
    // Test case insensitive SNAPSHOT detection
    let snapshot_versions = vec![
        "1.0.0-SNAPSHOT",
        "1.0.0-snapshot",
        "1.0.0-Snapshot",
        "2.3.4-SNAPSHOT",
    ];
    
    for version in snapshot_versions {
        let result = validator.parse_version(version).await;
        assert!(result.is_ok(), "Failed to parse version {}: {:?}", version, result.err());
        
        let parsed = result.unwrap();
        assert!(parsed.is_snapshot, "Version {} should be detected as snapshot", version);
        
        // The original version string should be preserved
        assert_eq!(parsed.original, version);
    }
}