
// crates/shared/src/enums.rs

use serde::{Serialize, Deserialize};
use std::str::FromStr;

/// Ecosistemas de paquetes soportados por el sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ecosystem {
    Maven, Npm, Docker, Oci, Pypi, Nuget, Go, RubyGems, Helm, Generic,
}

impl FromStr for Ecosystem {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Maven" => Ok(Ecosystem::Maven),
            "Npm" => Ok(Ecosystem::Npm),
            "Docker" => Ok(Ecosystem::Docker),
            "Oci" => Ok(Ecosystem::Oci),
            "Pypi" => Ok(Ecosystem::Pypi),
            "Nuget" => Ok(Ecosystem::Nuget),
            "Go" => Ok(Ecosystem::Go),
            "RubyGems" => Ok(Ecosystem::RubyGems),
            "Helm" => Ok(Ecosystem::Helm),
            "Generic" => Ok(Ecosystem::Generic),
            _ => Err(()),
        }
    }
}

/// Algoritmos de hash soportados para la verificación de integridad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Sha256, Sha512, Md5,
}

/// Niveles de severidad de vulnerabilidades, basados en estándares como CVSS.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Ord, PartialOrd, PartialEq, Eq)]
pub enum VulnerabilitySeverity {
    Critical, High, Medium, Low, Info, Unknown,
}

/// El tipo de un fichero físico dentro de un paquete.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactType {
    Primary, Signature, Sbom, Metadata, Documentation, Sources, Other,
}

/// El rol semántico que un fichero físico juega dentro de un `PackageVersion`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactRole {
    Main, Pom, PackageJson, Sources, Javadoc, TypeDefinitions, Signature, Sbom, Other,
}
