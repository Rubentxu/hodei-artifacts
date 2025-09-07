use serde::{Serialize, Deserialize};
use shared::{ArtifactId, RepositoryId, IsoTimestamp, UserId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactVersion(pub String);

impl ArtifactVersion { pub fn new(raw: impl Into<String>) -> Self { Self(raw.into()) } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactChecksum(pub String); // sha256
impl ArtifactChecksum { pub fn new(v: impl Into<String>) -> Self { Self(v.into()) } }

/// Ecosistema del paquete (uniforme para Maven / NPM / PyPI / genérico).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    Maven,
    Npm,
    Pypi,
    Generic,
}

impl Default for Ecosystem {
    fn default() -> Self { Ecosystem::Generic }
}

/// Versión normalizada (si se puede interpretar semver) manteniendo original.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub original: String,
    pub normalized: Option<String>,
}

impl Version {
    pub fn new(original: impl Into<String>, normalized: Option<String>) -> Self {
        Self { original: original.into(), normalized }
    }
    pub fn original(&self) -> &str { &self.original }
    pub fn normalized(&self) -> Option<&str> { self.normalized.as_deref() }
}

/// Coordenadas de un paquete de cualquier ecosistema.
/// canonical = "{ecosystem}:{namespace?}:{name}:{version_original}"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageCoordinates {
    pub ecosystem: Ecosystem,
    pub namespace: Option<String>,
    pub name: String,
    pub version: Version,
    pub qualifiers: std::collections::BTreeMap<String, String>,
    pub canonical: String,
}

impl PackageCoordinates {
    pub fn build(
        ecosystem: Ecosystem,
        namespace: Option<String>,
        name: impl Into<String>,
        version_original: impl Into<String>,
        version_normalized: Option<String>,
        qualifiers: std::collections::BTreeMap<String, String>,
    ) -> Result<Self, CoordinatesError> {
        let name_s = name.into();
        if name_s.trim().is_empty() {
            return Err(CoordinatesError::EmptyName);
        }
        // Validaciones mínimas específicas (se pueden ampliar en adapters / factories especializadas).
        if matches!(ecosystem, Ecosystem::Maven) {
            if let Some(ns) = &namespace {
                if ns.is_empty() { return Err(CoordinatesError::InvalidNamespace); }
            }
        }
        let version_original_s = version_original.into();
        if version_original_s.is_empty() {
            return Err(CoordinatesError::EmptyVersion);
        }
        let canonical = format!(
            "{}:{}:{}:{}",
            to_ecosystem_str(ecosystem),
            namespace.as_deref().unwrap_or("-"),
            name_s,
            version_original_s
        );
        Ok(Self {
            ecosystem,
            namespace,
            name: name_s,
            version: Version::new(version_original_s, version_normalized),
            qualifiers,
            canonical,
        })
    }
}

fn to_ecosystem_str(e: Ecosystem) -> &'static str {
    match e {
        Ecosystem::Maven => "maven",
        Ecosystem::Npm => "npm",
        Ecosystem::Pypi => "pypi",
        Ecosystem::Generic => "generic",
    }
}

/// Errores de construcción / validación de coordenadas (dominio puro).
#[derive(Debug, thiserror::Error)]
pub enum CoordinatesError {
    #[error("nombre vacío")]
    EmptyName,
    #[error("versión vacía")]
    EmptyVersion,
    #[error("namespace inválido")]
    InvalidNamespace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub repository_id: RepositoryId,
    pub version: ArtifactVersion,
    pub file_name: String,
    pub size_bytes: u64,
    pub checksum: ArtifactChecksum,
    pub created_at: IsoTimestamp,
    pub created_by: UserId,
    /// Coordenadas del paquete (uniforme multi-ecosistema). Opcional mientras se migra (Generic por defecto).
    pub coordinates: Option<PackageCoordinates>,
}

impl Artifact {
    pub fn new(repository_id: RepositoryId, version: ArtifactVersion, file_name: String, size_bytes: u64, checksum: ArtifactChecksum, created_by: UserId) -> Self {
        Self {
            id: ArtifactId::new(),
            repository_id,
            version,
            file_name,
            size_bytes,
            checksum,
            created_at: IsoTimestamp::now(),
            created_by,
            coordinates: None,
        }
    }

    pub fn with_coordinates(mut self, coords: PackageCoordinates) -> Self {
        self.coordinates = Some(coords);
        self
    }
}

