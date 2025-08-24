use artifact::domain::model::{
    Artifact, ArtifactVersion, ArtifactChecksum, PackageCoordinates, 
    Version, Ecosystem, CoordinatesError
};
use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
use std::collections::BTreeMap;

/// Builder para crear instancias de Artifact en tests
#[derive(Debug, Clone)]
pub struct ArtifactBuilder {
    id: Option<ArtifactId>,
    repository_id: Option<RepositoryId>,
    version: Option<ArtifactVersion>,
    file_name: Option<String>,
    size_bytes: Option<u64>,
    checksum: Option<ArtifactChecksum>,
    created_at: Option<IsoTimestamp>,
    created_by: Option<UserId>,
    coordinates: Option<PackageCoordinates>,
}

impl Default for ArtifactBuilder {
    fn default() -> Self {
        Self {
            id: None,
            repository_id: None,
            version: None,
            file_name: None,
            size_bytes: None,
            checksum: None,
            created_at: None,
            created_by: None,
            coordinates: None,
        }
    }
}

impl ArtifactBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_id(mut self, id: ArtifactId) -> Self {
        self.id = Some(id);
        self
    }

    pub fn with_repository_id(mut self, repository_id: RepositoryId) -> Self {
        self.repository_id = Some(repository_id);
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(ArtifactVersion::new(version));
        self
    }

    pub fn with_file_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    pub fn with_size_bytes(mut self, size_bytes: u64) -> Self {
        self.size_bytes = Some(size_bytes);
        self
    }

    pub fn with_checksum(mut self, checksum: impl Into<String>) -> Self {
        self.checksum = Some(ArtifactChecksum::new(checksum));
        self
    }

    pub fn with_created_at(mut self, created_at: IsoTimestamp) -> Self {
        self.created_at = Some(created_at);
        self
    }

    pub fn with_created_by(mut self, created_by: UserId) -> Self {
        self.created_by = Some(created_by);
        self
    }

    pub fn with_coordinates(mut self, coordinates: PackageCoordinates) -> Self {
        self.coordinates = Some(coordinates);
        self
    }

    pub fn build(self) -> Artifact {
        let repository_id = self.repository_id.unwrap_or_else(|| RepositoryId::new());
        let version = self.version.unwrap_or_else(|| ArtifactVersion::new("1.0.0"));
        let file_name = self.file_name.unwrap_or_else(|| "test-artifact.jar".to_string());
        let size_bytes = self.size_bytes.unwrap_or(1024);
        let checksum = self.checksum.unwrap_or_else(|| ArtifactChecksum::new("abc123"));
        let created_by = self.created_by.unwrap_or_else(|| UserId::new());

        let mut artifact = Artifact::new(
            repository_id,
            version,
            file_name,
            size_bytes,
            checksum,
            created_by,
        );

        if let Some(id) = self.id {
            artifact.id = id;
        }

        if let Some(created_at) = self.created_at {
            artifact.created_at = created_at;
        }

        if let Some(coordinates) = self.coordinates {
            artifact = artifact.with_coordinates(coordinates);
        }

        artifact
    }
}

/// Builder para crear instancias de PackageCoordinates en tests
#[derive(Debug, Clone)]
pub struct PackageCoordinatesBuilder {
    ecosystem: Option<Ecosystem>,
    namespace: Option<String>,
    name: Option<String>,
    version_original: Option<String>,
    version_normalized: Option<String>,
    qualifiers: BTreeMap<String, String>,
}

impl Default for PackageCoordinatesBuilder {
    fn default() -> Self {
        Self {
            ecosystem: None,
            namespace: None,
            name: None,
            version_original: None,
            version_normalized: None,
            qualifiers: BTreeMap::new(),
        }
    }
}

impl PackageCoordinatesBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_ecosystem(mut self, ecosystem: Ecosystem) -> Self {
        self.ecosystem = Some(ecosystem);
        self
    }

    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_version_original(mut self, version: impl Into<String>) -> Self {
        self.version_original = Some(version.into());
        self
    }

    pub fn with_version_normalized(mut self, version: impl Into<String>) -> Self {
        self.version_normalized = Some(version.into());
        self
    }

    pub fn with_qualifier(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.qualifiers.insert(key.into(), value.into());
        self
    }

    pub fn maven(self) -> Self {
        self.with_ecosystem(Ecosystem::Maven)
    }

    pub fn npm(self) -> Self {
        self.with_ecosystem(Ecosystem::Npm)
    }

    pub fn pypi(self) -> Self {
        self.with_ecosystem(Ecosystem::Pypi)
    }

    pub fn generic(self) -> Self {
        self.with_ecosystem(Ecosystem::Generic)
    }

    pub fn build(self) -> Result<PackageCoordinates, CoordinatesError> {
        let ecosystem = self.ecosystem.unwrap_or(Ecosystem::Generic);
        let name = self.name.unwrap_or_else(|| "test-package".to_string());
        let version_original = self.version_original.unwrap_or_else(|| "1.0.0".to_string());

        PackageCoordinates::build(
            ecosystem,
            self.namespace,
            name,
            version_original,
            self.version_normalized,
            self.qualifiers,
        )
    }
}

/// Métodos de conveniencia para crear builders predefinidos
pub fn artifact() -> ArtifactBuilder {
    ArtifactBuilder::new()
}

pub fn package_coordinates() -> PackageCoordinatesBuilder {
    PackageCoordinatesBuilder::new()
}

pub fn maven_artifact(group_id: &str, artifact_id: &str, version: &str) -> PackageCoordinatesBuilder {
    package_coordinates()
        .maven()
        .with_namespace(group_id)
        .with_name(artifact_id)
        .with_version_original(version)
}

pub fn npm_package(name: &str, version: &str) -> PackageCoordinatesBuilder {
    package_coordinates()
        .npm()
        .with_name(name)
        .with_version_original(version)
}

pub fn pypi_package(name: &str, version: &str) -> PackageCoordinatesBuilder {
    package_coordinates()
        .pypi()
        .with_name(name)
        .with_version_original(version)
}
