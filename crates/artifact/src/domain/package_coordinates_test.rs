use crate::domain::model::{PackageCoordinates, Ecosystem, CoordinatesError};
use std::collections::BTreeMap;

#[cfg(test)]
mod package_coordinates_tests {
    use super::*;

    mod build_valid_coordinates {
        use super::*;

        #[test]
        fn should_build_maven_coordinates_with_namespace() {
            let result = PackageCoordinates::build(
                Ecosystem::Maven,
                Some("com.example".to_string()),
                "test-artifact",
                "1.0.0",
                Some("1.0.0".to_string()),
                BTreeMap::new(),
            );

            assert!(result.is_ok());
            let coords = result.unwrap();
            assert_eq!(coords.ecosystem, Ecosystem::Maven);
            assert_eq!(coords.namespace, Some("com.example".to_string()));
            assert_eq!(coords.name, "test-artifact");
            assert_eq!(coords.version.original(), "1.0.0");
            assert_eq!(coords.version.normalized(), Some("1.0.0"));
            assert_eq!(coords.canonical, "maven:com.example:test-artifact:1.0.0");
        }

        #[test]
        fn should_build_npm_coordinates_without_namespace() {
            let result = PackageCoordinates::build(
                Ecosystem::Npm,
                None,
                "react",
                "18.2.0",
                Some("18.2.0".to_string()),
                BTreeMap::new(),
            );

            assert!(result.is_ok());
            let coords = result.unwrap();
            assert_eq!(coords.ecosystem, Ecosystem::Npm);
            assert_eq!(coords.namespace, None);
            assert_eq!(coords.name, "react");
            assert_eq!(coords.canonical, "npm:-:react:18.2.0");
        }

        #[test]
        fn should_build_pypi_coordinates() {
            let result = PackageCoordinates::build(
                Ecosystem::Pypi,
                None,
                "django",
                "4.2.1",
                None,
                BTreeMap::new(),
            );

            assert!(result.is_ok());
            let coords = result.unwrap();
            assert_eq!(coords.ecosystem, Ecosystem::Pypi);
            assert_eq!(coords.version.normalized(), None);
            assert_eq!(coords.canonical, "pypi:-:django:4.2.1");
        }

        #[test]
        fn should_build_generic_coordinates() {
            let result = PackageCoordinates::build(
                Ecosystem::Generic,
                Some("custom".to_string()),
                "binary",
                "1.0.0-beta",
                None,
                BTreeMap::new(),
            );

            assert!(result.is_ok());
            let coords = result.unwrap();
            assert_eq!(coords.ecosystem, Ecosystem::Generic);
            assert_eq!(coords.canonical, "generic:custom:binary:1.0.0-beta");
        }

        #[test]
        fn should_include_qualifiers_in_coordinates() {
            let mut qualifiers = BTreeMap::new();
            qualifiers.insert("classifier".to_string(), "sources".to_string());
            qualifiers.insert("type".to_string(), "jar".to_string());

            let result = PackageCoordinates::build(
                Ecosystem::Maven,
                Some("com.example".to_string()),
                "test",
                "1.0.0",
                None,
                qualifiers.clone(),
            );

            assert!(result.is_ok());
            let coords = result.unwrap();
            assert_eq!(coords.qualifiers, qualifiers);
        }
    }

    mod validation_errors {
        use super::*;

        #[test]
        fn should_reject_empty_name() {
            let result = PackageCoordinates::build(
                Ecosystem::Maven,
                Some("com.example".to_string()),
                "",
                "1.0.0",
                None,
                BTreeMap::new(),
            );

            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), CoordinatesError::EmptyName));
        }

        #[test]
        fn should_reject_whitespace_only_name() {
            let result = PackageCoordinates::build(
                Ecosystem::Maven,
                Some("com.example".to_string()),
                "   ",
                "1.0.0",
                None,
                BTreeMap::new(),
            );

            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), CoordinatesError::EmptyName));
        }

        #[test]
        fn should_reject_empty_version() {
            let result = PackageCoordinates::build(
                Ecosystem::Maven,
                Some("com.example".to_string()),
                "test-artifact",
                "",
                None,
                BTreeMap::new(),
            );

            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), CoordinatesError::EmptyVersion));
        }

        #[test]
        fn should_reject_maven_with_empty_namespace() {
            let result = PackageCoordinates::build(
                Ecosystem::Maven,
                Some("".to_string()),
                "test-artifact",
                "1.0.0",
                None,
                BTreeMap::new(),
            );

            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), CoordinatesError::InvalidNamespace));
        }
    }

    mod canonical_format {
        use super::*;

        #[test]
        fn should_use_dash_for_missing_namespace() {
            let result = PackageCoordinates::build(
                Ecosystem::Npm,
                None,
                "package-name",
                "1.0.0",
                None,
                BTreeMap::new(),
            );

            assert!(result.is_ok());
            let coords = result.unwrap();
            assert_eq!(coords.canonical, "npm:-:package-name:1.0.0");
        }

        #[test]
        fn should_format_all_ecosystems_correctly() {
            let test_cases = vec![
                (Ecosystem::Maven, "maven"),
                (Ecosystem::Npm, "npm"),
                (Ecosystem::Pypi, "pypi"),
                (Ecosystem::Generic, "generic"),
            ];

            for (ecosystem, expected_prefix) in test_cases {
                let result = PackageCoordinates::build(
                    ecosystem,
                    Some("ns".to_string()),
                    "name",
                    "1.0.0",
                    None,
                    BTreeMap::new(),
                );

                assert!(result.is_ok());
                let coords = result.unwrap();
                assert!(coords.canonical.starts_with(expected_prefix));
                assert_eq!(coords.canonical, format!("{}:ns:name:1.0.0", expected_prefix));
            }
        }
    }
}

#[cfg(test)]
mod version_tests {
    use crate::domain::model::Version;

    #[test]
    fn should_store_original_and_normalized_versions() {
        let version = Version::new("1.0.0-SNAPSHOT", Some("1.0.0".to_string()));
        
        assert_eq!(version.original(), "1.0.0-SNAPSHOT");
        assert_eq!(version.normalized(), Some("1.0.0"));
    }

    #[test]
    fn should_handle_none_normalized_version() {
        let version = Version::new("custom-version", None);
        
        assert_eq!(version.original(), "custom-version");
        assert_eq!(version.normalized(), None);
    }
}

#[cfg(test)]
mod ecosystem_tests {
    use crate::domain::model::Ecosystem;
    use serde_json;

    #[test]
    fn should_serialize_to_lowercase() {
        assert_eq!(serde_json::to_string(&Ecosystem::Maven).unwrap(), "\"maven\"");
        assert_eq!(serde_json::to_string(&Ecosystem::Npm).unwrap(), "\"npm\"");
        assert_eq!(serde_json::to_string(&Ecosystem::Pypi).unwrap(), "\"pypi\"");
        assert_eq!(serde_json::to_string(&Ecosystem::Generic).unwrap(), "\"generic\"");
    }

    #[test]
    fn should_deserialize_from_lowercase() {
        assert_eq!(serde_json::from_str::<Ecosystem>("\"maven\"").unwrap(), Ecosystem::Maven);
        assert_eq!(serde_json::from_str::<Ecosystem>("\"npm\"").unwrap(), Ecosystem::Npm);
        assert_eq!(serde_json::from_str::<Ecosystem>("\"pypi\"").unwrap(), Ecosystem::Pypi);
        assert_eq!(serde_json::from_str::<Ecosystem>("\"generic\"").unwrap(), Ecosystem::Generic);
    }

    #[test]
    fn should_default_to_generic() {
        assert_eq!(Ecosystem::default(), Ecosystem::Generic);
    }
}
