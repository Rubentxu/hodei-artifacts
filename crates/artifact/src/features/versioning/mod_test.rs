// crates/artifact/src/features/versioning/mod_test.rs

#[cfg(test)]
mod tests {
    use super::super::versioning::error::VersioningError;
    use super::super::versioning::{
        ParsedVersion, VersionValidator, VersioningConfig, VersioningPolicy,
    };

    #[test]
    fn test_integration_version_validation() {
        // Crear una configuración completa
        let mut config = VersioningConfig::default();
        config.strict_semver = true;
        config.allowed_prerelease_tags = vec!["alpha".to_string(), "beta".to_string()];
        config.reject_build_metadata = true;

        // Crear política y validador
        let policy = VersioningPolicy::new(config);
        let validator = VersionValidator::new(policy);

        // Probar una versión que cumple con todas las políticas
        let valid_version = "1.0.0-alpha.1";
        assert!(validator.validate_version(valid_version).is_ok());

        // Probar una versión que viola la política de tags pre-release
        let invalid_prerelease = "1.0.0-rc.1";
        match validator.validate_version(invalid_prerelease) {
            Err(VersioningError::PrereleaseTagNotAllowed(_)) => {
                // Correcto
            }
            _ => {
                panic!("Should have rejected prerelease tag");
            }
        }

        // Probar una versión que viola la política de metadata de build
        let version_with_build = "1.0.0+build.123";
        match validator.validate_version(version_with_build) {
            Err(VersioningError::BuildMetadataNotAllowed(_)) => {
                // Correcto
            }
            _ => {
                panic!("Should have rejected build metadata");
            }
        }
    }

    #[test]
    fn test_snapshot_version_integration() {
        let validator = VersionValidator::default();

        // Parsear una versión SNAPSHOT
        let snapshot_version = "1.0.0-SNAPSHOT";
        let parsed = validator.parse_version(snapshot_version).unwrap();

        assert_eq!(parsed.original, snapshot_version);
        assert_eq!(parsed.major, 1);
        assert_eq!(parsed.minor, 0);
        assert_eq!(parsed.patch, 0);
        assert_eq!(parsed.prerelease, None);
        assert_eq!(parsed.build_metadata, None);
        assert!(parsed.is_snapshot);
    }

    #[test]
    fn test_complex_version_parsing() {
        let validator = VersionValidator::default();

        // Parsear una versión compleja
        let complex_version = "2.1.3-alpha.1.beta.2+build.123.sha.a1b2c3d";
        let parsed = validator.parse_version(complex_version).unwrap();

        assert_eq!(parsed.original, complex_version);
        assert_eq!(parsed.major, 2);
        assert_eq!(parsed.minor, 1);
        assert_eq!(parsed.patch, 3);
        assert_eq!(parsed.prerelease, Some("alpha.1.beta.2".to_string()));
        assert_eq!(
            parsed.build_metadata,
            Some("build.123.sha.a1b2c3d".to_string())
        );
        assert!(!parsed.is_snapshot);
    }
}
