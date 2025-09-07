use artifact::domain::model::{Artifact, PackageCoordinates, Ecosystem};
use shared::{ArtifactId, RepositoryId, UserId};
use super::builders::*;

/// Fixtures de datos comunes para pruebas
pub struct Fixtures;

impl Fixtures {
    /// ID de repositorio fijo para pruebas
    pub fn repository_id() -> RepositoryId {
        RepositoryId::from_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
    }

    /// ID de usuario fijo para pruebas
    pub fn user_id() -> UserId {
        UserId::from_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }

    /// ID de artifact fijo para pruebas
    pub fn artifact_id() -> ArtifactId {
        ArtifactId::from_str("550e8400-e29b-41d4-a716-446655440002").unwrap()
    }

    /// Checksum SHA256 válido para pruebas
    pub fn valid_checksum() -> &'static str {
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    }

    /// Crea un artifact básico para pruebas
    pub fn basic_artifact() -> Artifact {
        artifact()
            .with_id(Self::artifact_id())
            .with_repository_id(Self::repository_id())
            .with_version("1.0.0")
            .with_file_name("test-artifact.jar")
            .with_size_bytes(1024)
            .with_checksum(Self::valid_checksum())
            .with_created_by(Self::user_id())
            .build()
    }

    /// Crea un artifact Maven para pruebas
    pub fn maven_artifact() -> Artifact {
        let coords = maven_artifact("com.example", "test-artifact", "1.0.0")
            .build()
            .expect("Invalid Maven coordinates");

        artifact()
            .with_id(Self::artifact_id())
            .with_repository_id(Self::repository_id())
            .with_version("1.0.0")
            .with_file_name("test-artifact-1.0.0.jar")
            .with_size_bytes(2048)
            .with_checksum(Self::valid_checksum())
            .with_created_by(Self::user_id())
            .with_coordinates(coords)
            .build()
    }

    /// Crea un artifact NPM para pruebas
    pub fn npm_artifact() -> Artifact {
        let coords = npm_package("@example/test-package", "1.0.0")
            .build()
            .expect("Invalid NPM coordinates");

        artifact()
            .with_id(Self::artifact_id())
            .with_repository_id(Self::repository_id())
            .with_version("1.0.0")
            .with_file_name("test-package-1.0.0.tgz")
            .with_size_bytes(512)
            .with_checksum(Self::valid_checksum())
            .with_created_by(Self::user_id())
            .with_coordinates(coords)
            .build()
    }

    /// Crea coordenadas Maven válidas para pruebas
    pub fn maven_coordinates() -> PackageCoordinates {
        maven_artifact("com.example", "test-lib", "2.1.0")
            .with_version_normalized("2.1.0")
            .build()
            .expect("Invalid Maven coordinates")
    }

    /// Crea coordenadas NPM válidas para pruebas
    pub fn npm_coordinates() -> PackageCoordinates {
        npm_package("react", "18.2.0")
            .with_version_normalized("18.2.0")
            .build()
            .expect("Invalid NPM coordinates")
    }

    /// Crea coordenadas PyPI válidas para pruebas
    pub fn pypi_coordinates() -> PackageCoordinates {
        pypi_package("django", "4.2.1")
            .with_version_normalized("4.2.1")
            .build()
            .expect("Invalid PyPI coordinates")
    }

    /// Crea coordenadas genéricas válidas para pruebas
    pub fn generic_coordinates() -> PackageCoordinates {
        package_coordinates()
            .generic()
            .with_name("custom-binary")
            .with_version_original("1.0.0-beta")
            .build()
            .expect("Invalid generic coordinates")
    }
}

/// Constantes útiles para pruebas
pub mod constants {
    pub const TEST_FILE_CONTENT: &[u8] = b"test file content for artifact upload";
    pub const TEST_FILE_SIZE: u64 = 37; // tamaño de TEST_FILE_CONTENT
    
    pub const MAVEN_GROUP_ID: &str = "com.example";
    pub const MAVEN_ARTIFACT_ID: &str = "test-artifact";
    pub const MAVEN_VERSION: &str = "1.0.0";
    
    pub const NPM_PACKAGE_NAME: &str = "@scope/package";
    pub const NPM_VERSION: &str = "2.1.0";
    
    pub const PYPI_PACKAGE_NAME: &str = "test-package";
    pub const PYPI_VERSION: &str = "0.1.0";
}
