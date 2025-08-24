use artifact::domain::model::{Artifact, ArtifactVersion, ArtifactChecksum};
use shared::{RepositoryId, UserId};

#[cfg(test)]
mod artifact_tests {
    use super::*;
    use crate::support::{Fixtures, artifact, maven_artifact};

    mod artifact_creation {
        use super::*;

        #[test]
        fn should_create_artifact_with_generated_id_and_timestamp() {
            let repository_id = Fixtures::repository_id();
            let user_id = Fixtures::user_id();
            let version = ArtifactVersion::new("1.0.0");
            let checksum = ArtifactChecksum::new(Fixtures::valid_checksum());

            let artifact = Artifact::new(
                repository_id,
                version.clone(),
                "test.jar".to_string(),
                1024,
                checksum.clone(),
                user_id,
            );

            assert_eq!(artifact.repository_id, repository_id);
            assert_eq!(artifact.version.0, version.0);
            assert_eq!(artifact.file_name, "test.jar");
            assert_eq!(artifact.size_bytes, 1024);
            assert_eq!(artifact.checksum.0, checksum.0);
            assert_eq!(artifact.created_by, user_id);
            assert!(artifact.coordinates.is_none());
            // ID y timestamp se generan automáticamente
            assert!(!artifact.id.to_string().is_empty());
        }

        #[test]
        fn should_create_artifact_with_coordinates() {
            let coords = maven_artifact("com.example", "test", "1.0.0")
                .build()
                .expect("Valid coordinates");

            let artifact = Artifact::new(
                Fixtures::repository_id(),
                ArtifactVersion::new("1.0.0"),
                "test-1.0.0.jar".to_string(),
                2048,
                ArtifactChecksum::new(Fixtures::valid_checksum()),
                Fixtures::user_id(),
            ).with_coordinates(coords.clone());

            assert!(artifact.coordinates.is_some());
            let artifact_coords = artifact.coordinates.unwrap();
            assert_eq!(artifact_coords.name, coords.name);
            assert_eq!(artifact_coords.canonical, coords.canonical);
        }
    }

    mod using_builders {
        use super::*;

        #[test]
        fn should_build_artifact_with_all_fields() {
            let artifact_id = Fixtures::artifact_id();
            let repository_id = Fixtures::repository_id();
            let user_id = Fixtures::user_id();

            let artifact = artifact()
                .with_id(artifact_id)
                .with_repository_id(repository_id)
                .with_version("2.1.0")
                .with_file_name("custom.jar")
                .with_size_bytes(4096)
                .with_checksum("custom-checksum")
                .with_created_by(user_id)
                .build();

            assert_eq!(artifact.id, artifact_id);
            assert_eq!(artifact.repository_id, repository_id);
            assert_eq!(artifact.version.0, "2.1.0");
            assert_eq!(artifact.file_name, "custom.jar");
            assert_eq!(artifact.size_bytes, 4096);
            assert_eq!(artifact.checksum.0, "custom-checksum");
            assert_eq!(artifact.created_by, user_id);
        }

        #[test]
        fn should_build_artifact_with_defaults() {
            let artifact = artifact().build();

            assert_eq!(artifact.version.0, "1.0.0");
            assert_eq!(artifact.file_name, "test-artifact.jar");
            assert_eq!(artifact.size_bytes, 1024);
            assert_eq!(artifact.checksum.0, "abc123");
            assert!(artifact.coordinates.is_none());
        }

        #[test]
        fn should_use_fixtures_for_common_scenarios() {
            let basic = Fixtures::basic_artifact();
            assert_eq!(basic.id, Fixtures::artifact_id());
            assert_eq!(basic.repository_id, Fixtures::repository_id());

            let maven = Fixtures::maven_artifact();
            assert!(maven.coordinates.is_some());
            assert_eq!(maven.file_name, "test-artifact-1.0.0.jar");

            let npm = Fixtures::npm_artifact();
            assert!(npm.coordinates.is_some());
            assert_eq!(npm.file_name, "test-package-1.0.0.tgz");
        }
    }
}

mod artifact_version_tests {
    use super::*;

    #[test]
    fn should_create_version_from_string() {
        let version = ArtifactVersion::new("1.2.3-SNAPSHOT");
        assert_eq!(version.0, "1.2.3-SNAPSHOT");
    }

    #[test]
    fn should_create_version_from_string_ref() {
        let version_str = "2.0.0";
        let version = ArtifactVersion::new(version_str);
        assert_eq!(version.0, "2.0.0");
    }

    #[test]
    fn should_create_version_from_owned_string() {
        let version_string = String::from("3.1.4");
        let version = ArtifactVersion::new(version_string);
        assert_eq!(version.0, "3.1.4");
    }
}

mod artifact_checksum_tests {
    use super::*;

    #[test]
    fn should_create_checksum_from_string() {
        let checksum = ArtifactChecksum::new("abcdef123456");
        assert_eq!(checksum.0, "abcdef123456");
    }

    #[test]
    fn should_accept_sha256_format() {
        let sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let checksum = ArtifactChecksum::new(sha256);
        assert_eq!(checksum.0, sha256);
    }

    #[test]
    fn should_accept_any_string_format() {
        // El checksum no valida formato específico en el dominio
        let custom_checksum = "custom-hash-123";
        let checksum = ArtifactChecksum::new(custom_checksum);
        assert_eq!(checksum.0, custom_checksum);
    }
}
