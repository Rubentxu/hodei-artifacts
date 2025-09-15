// crates/artifact/src/features/versioning/validator_test.rs

#[cfg(test)]
mod tests {
    use super::super::dto::VersioningConfig;
    use super::super::error::VersioningError;
    use super::super::policy::VersioningPolicy;
    use super::super::validator::VersionValidator;

    #[test]
    fn test_valid_semver_versions() {
        let validator = VersionValidator::default();

        // Versiones válidas según SemVer
        let valid_versions = vec![
            "1.0.0",
            "1.2.3",
            "10.20.30",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-0.3.7",
            "1.0.0-x.7.z.92",
            "1.0.0-alpha+001",
            "1.0.0+20130313144700",
            "1.0.0-beta+exp.sha.5114f85",
        ];

        for version in valid_versions {
            assert!(
                validator.validate_version(version).is_ok(),
                "Version {} should be valid",
                version
            );
        }
    }

    #[test]
    fn test_invalid_semver_versions() {
        let validator = VersionValidator::default();

        // Versiones inválidas según SemVer
        let invalid_versions = vec![
            "1", "1.0", "1.0.0.0", "a.b.c", "1.0.0-",  // Tag pre-release vacío
            "1.0.0-!", // Carácter no válido en tag pre-release
        ];

        for version in invalid_versions {
            assert!(
                validator.validate_version(version).is_err(),
                "Version {} should be invalid",
                version
            );
        }
    }

    #[test]
    fn test_snapshot_versions() {
        let validator = VersionValidator::default();

        // Versiones SNAPSHOT válidas
        let snapshot_versions = vec!["1.0.0-SNAPSHOT", "2.1.3-SNAPSHOT", "1.0.0-alpha.1-SNAPSHOT"];

        for version in snapshot_versions {
            let parsed = validator
                .parse_version(version)
                .expect(&format!("Version {} should be parseable", version));
            assert!(
                parsed.is_snapshot,
                "Version {} should be identified as SNAPSHOT",
                version
            );
        }
    }

    #[test]
    fn test_strict_semver_enforcement() {
        let mut config = VersioningConfig::default();
        config.strict_semver = true;
        let policy = VersioningPolicy::new(config);
        let validator = VersionValidator::new(policy);

        // Versiones que siguen SemVer estricto
        let strict_versions = vec!["1.0.0", "1.2.3", "1.0.0-alpha.1"];

        for version in strict_versions {
            assert!(
                validator.validate_version(version).is_ok(),
                "Version {} should be valid under strict SemVer",
                version
            );
        }
    }

    #[test]
    fn test_prerelease_tag_restrictions() {
        let mut config = VersioningConfig::default();
        config.allowed_prerelease_tags = vec!["alpha".to_string(), "beta".to_string()];
        let policy = VersioningPolicy::new(config);
        let validator = VersionValidator::new(policy);

        // Versiones con tags permitidos
        let allowed_versions = vec!["1.0.0-alpha", "1.0.0-beta.1", "2.0.0-alpha.beta.gamma"];

        for version in allowed_versions {
            assert!(
                validator.validate_version(version).is_ok(),
                "Version {} should be allowed",
                version
            );
        }

        // Versiones con tags no permitidos
        let forbidden_versions = vec!["1.0.0-rc.1", "1.0.0-nightly"];

        for version in forbidden_versions {
            match validator.validate_version(version) {
                Err(VersioningError::PrereleaseTagNotAllowed(_)) => {
                    // Correcto, se esperaba este error
                }
                _ => {
                    panic!(
                        "Version {} should be rejected for forbidden prerelease tag",
                        version
                    );
                }
            }
        }
    }

    #[test]
    fn test_build_metadata_rejection() {
        let mut config = VersioningConfig::default();
        config.reject_build_metadata = true;
        let policy = VersioningPolicy::new(config);
        let validator = VersionValidator::new(policy);

        // Versiones con metadata de build que deben ser rechazadas
        let versions_with_build = vec!["1.0.0+build.1", "1.0.0-alpha.1+build.123"];

        for version in versions_with_build {
            match validator.validate_version(version) {
                Err(VersioningError::BuildMetadataNotAllowed(_)) => {
                    // Correcto, se esperaba este error
                }
                _ => {
                    panic!("Version {} should be rejected for build metadata", version);
                }
            }
        }

        // Versiones sin metadata de build que deben ser aceptadas
        let versions_without_build = vec!["1.0.0", "1.0.0-alpha.1"];

        for version in versions_without_build {
            assert!(
                validator.validate_version(version).is_ok(),
                "Version {} should be allowed",
                version
            );
        }
    }

    #[test]
    fn test_version_parsing_details() {
        let validator = VersionValidator::default();
        let parsed = validator.parse_version("1.2.3-alpha.1+build.123").unwrap();

        assert_eq!(parsed.original, "1.2.3-alpha.1+build.123");
        assert_eq!(parsed.major, 1);
        assert_eq!(parsed.minor, 2);
        assert_eq!(parsed.patch, 3);
        assert_eq!(parsed.prerelease, Some("alpha.1".to_string()));
        assert_eq!(parsed.build_metadata, Some("build.123".to_string()));
        assert_eq!(parsed.is_snapshot, false);
    }
}
