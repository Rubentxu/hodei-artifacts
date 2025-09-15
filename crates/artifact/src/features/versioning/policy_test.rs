// crates/artifact/src/features/versioning/policy_test.rs

#[cfg(test)]
mod tests {
    use super::super::dto::{ParsedVersion, VersioningConfig};
    use super::super::error::VersioningError;
    use super::super::policy::VersioningPolicy;

    #[test]
    fn test_default_policy() {
        let config = VersioningConfig::default();
        let policy = VersioningPolicy::new(config);

        assert!(!policy.is_strict_semver());

        // Con la política por defecto, todas las versiones válidas deben pasar
        let parsed_version = ParsedVersion {
            original: "1.0.0".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: None,
            build_metadata: None,
            is_snapshot: false,
        };

        assert!(policy.validate_version(&parsed_version).is_ok());
    }

    #[test]
    fn test_strict_semver_policy() {
        let mut config = VersioningConfig::default();
        config.strict_semver = true;
        let policy = VersioningPolicy::new(config);

        assert!(policy.is_strict_semver());
    }

    #[test]
    fn test_allowed_prerelease_tags_policy() {
        let mut config = VersioningConfig::default();
        config.allowed_prerelease_tags = vec!["alpha".to_string(), "beta".to_string()];
        let policy = VersioningPolicy::new(config);

        // Versión con tag permitido
        let parsed_alpha = ParsedVersion {
            original: "1.0.0-alpha".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: Some("alpha".to_string()),
            build_metadata: None,
            is_snapshot: false,
        };

        assert!(policy.validate_version(&parsed_alpha).is_ok());

        // Versión con tag no permitido
        let parsed_rc = ParsedVersion {
            original: "1.0.0-rc.1".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: Some("rc.1".to_string()),
            build_metadata: None,
            is_snapshot: false,
        };

        match policy.validate_version(&parsed_rc) {
            Err(VersioningError::PrereleaseTagNotAllowed(_)) => {
                // Correcto
            }
            _ => {
                panic!("Should have rejected prerelease tag");
            }
        }
    }

    #[test]
    fn test_build_metadata_rejection_policy() {
        let mut config = VersioningConfig::default();
        config.reject_build_metadata = true;
        let policy = VersioningPolicy::new(config);

        // Versión con metadata de build
        let parsed_version = ParsedVersion {
            original: "1.0.0+build.123".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: None,
            build_metadata: Some("build.123".to_string()),
            is_snapshot: false,
        };

        match policy.validate_version(&parsed_version) {
            Err(VersioningError::BuildMetadataNotAllowed(_)) => {
                // Correcto
            }
            _ => {
                panic!("Should have rejected build metadata");
            }
        }
    }

    #[test]
    fn test_no_prerelease_tag_restrictions() {
        let mut config = VersioningConfig::default();
        config.allowed_prerelease_tags = vec![]; // Lista vacía significa todos permitidos
        let policy = VersioningPolicy::new(config);

        let parsed_version = ParsedVersion {
            original: "1.0.0-rc.1".to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: Some("rc.1".to_string()),
            build_metadata: None,
            is_snapshot: false,
        };

        assert!(policy.validate_version(&parsed_version).is_ok());
    }
}
