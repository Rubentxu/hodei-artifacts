// crates/distribution/src/domain/npm/version.rs

//! Versiones npm - Value Objects para versionado semántico npm

use std::cmp::Ordering;
use std::fmt;
use thiserror::Error;
use serde::{Serialize, Deserialize};

/// Error de validación para versiones npm
#[derive(Debug, Error, Clone, PartialEq)]
pub enum NpmVersionValidationError {
    #[error("Invalid version format: {0}")]
    InvalidFormat(String),
    #[error("Invalid major version: {0}")]
    InvalidMajor(String),
    #[error("Invalid minor version: {0}")]
    InvalidMinor(String),
    #[error("Invalid patch version: {0}")]
    InvalidPatch(String),
    #[error("Invalid prerelease: {0}")]
    InvalidPrerelease(String),
    #[error("Invalid build metadata: {0}")]
    InvalidBuildMetadata(String),
    #[error("Version cannot be empty")]
    EmptyVersion,
}

/// Versión npm siguiendo semver
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpmVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub prerelease: Option<String>,
    pub build_metadata: Option<String>,
    pub original: String,
}

impl NpmVersion {
    /// Crear una versión npm desde string
    pub fn new(version: &str) -> Result<Self, NpmVersionValidationError> {
        if version.is_empty() {
            return Err(NpmVersionValidationError::EmptyVersion);
        }
        
        let original = version.to_string();
        
        // Separar build metadata
        let (version_part, build_metadata) = if let Some(pos) = version.find('+') {
            let (v, b) = version.split_at(pos);
            (v, Some(b[1..].to_string()))
        } else {
            (version, None)
        };
        
        // Separar prerelease
        let (core_version, prerelease) = if let Some(pos) = version_part.find('-') {
            let (c, p) = version_part.split_at(pos);
            (c, Some(p[1..].to_string()))
        } else {
            (version_part, None)
        };
        
        // Parsear versión core
        let parts: Vec<&str> = core_version.split('.').collect();
        if parts.len() != 3 {
            return Err(NpmVersionValidationError::InvalidFormat(
                format!("Expected 3 parts, got {}", parts.len())
            ));
        }
        
        let major = parts[0].parse::<u32>()
            .map_err(|_| NpmVersionValidationError::InvalidMajor(parts[0].to_string()))?;
        
        let minor = parts[1].parse::<u32>()
            .map_err(|_| NpmVersionValidationError::InvalidMinor(parts[1].to_string()))?;
        
        let patch = parts[2].parse::<u32>()
            .map_err(|_| NpmVersionValidationError::InvalidPatch(parts[2].to_string()))?;
        
        // Validar prerelease si existe
        if let Some(ref pre) = prerelease {
            Self::validate_prerelease(pre)?;
        }
        
        // Validar build metadata si existe
        if let Some(ref build) = build_metadata {
            Self::validate_build_metadata(build)?;
        }
        
        Ok(Self {
            major,
            minor,
            patch,
            prerelease,
            build_metadata,
            original,
        })
    }
    
    /// Crear una versión desde componentes
    pub fn from_components(
        major: u32, 
        minor: u32, 
        patch: u32,
        prerelease: Option<&str>,
        build_metadata: Option<&str>
    ) -> Result<Self, NpmVersionValidationError> {
        let mut version = format!("{}.{}.{}", major, minor, patch);
        
        if let Some(pre) = prerelease {
            Self::validate_prerelease(pre)?;
            version.push('-');
            version.push_str(pre);
        }
        
        if let Some(build) = build_metadata {
            Self::validate_build_metadata(build)?;
            version.push('+');
            version.push_str(build);
        }
        
        Self::new(&version)
    }
    
    /// Obtener la versión como string
    pub fn to_string(&self) -> String {
        self.original.clone()
    }
    
    /// Obtener solo la parte de versión (sin build metadata)
    pub fn to_version_string(&self) -> String {
        let mut version = format!("{}.{}.{}", self.major, self.minor, self.patch);
        if let Some(ref pre) = self.prerelease {
            version.push('-');
            version.push_str(pre);
        }
        version
    }
    
    /// Verificar si es una versión prerelease
    pub fn is_prerelease(&self) -> bool {
        self.prerelease.is_some()
    }
    
    /// Verificar si es una versión estable (no prerelease)
    pub fn is_stable(&self) -> bool {
        self.prerelease.is_none()
    }
    
    /// Comparar con otra versión (sin considerar build metadata)
    pub fn compare(&self, other: &Self) -> Ordering {
        // Comparar major, minor, patch
        match self.major.cmp(&other.major) {
            Ordering::Equal => {},
            other => return other,
        }
        
        match self.minor.cmp(&other.minor) {
            Ordering::Equal => {},
            other => return other,
        }
        
        match self.patch.cmp(&other.patch) {
            Ordering::Equal => {},
            other => return other,
        }
        
        // Comparar prerelease
        match (&self.prerelease, &other.prerelease) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater, // Stable > prerelease
            (Some(_), None) => Ordering::Less,   // Prerelease < stable
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
    
    /// Verificar si esta versión satisface un rango
    pub fn satisfies_range(&self, range: &str) -> Result<bool, NpmVersionValidationError> {
        // Implementación simplificada - en producción usaría semver crate
        match range {
            "*" => Ok(true),
            "latest" => Ok(self.is_stable()),
            _ => {
                // Parsear rangos simples como ^1.0.0, ~1.0.0, >=1.0.0, etc.
                if range.starts_with('^') {
                    self.satisfies_caret(&range[1..])
                } else if range.starts_with('~') {
                    self.satisfies_tilde(&range[1..])
                } else if range.starts_with(">=") {
                    let other = NpmVersion::new(&range[2..])?;
                    Ok(self.compare(&other) != Ordering::Less)
                } else if range.starts_with("<=") {
                    let other = NpmVersion::new(&range[2..])?;
                    Ok(self.compare(&other) != Ordering::Greater)
                } else if range.starts_with('>') {
                    let other = NpmVersion::new(&range[1..])?;
                    Ok(self.compare(&other) == Ordering::Greater)
                } else if range.starts_with('<') {
                    let other = NpmVersion::new(&range[1..])?;
                    Ok(self.compare(&other) == Ordering::Less)
                } else {
                    // Versión exacta
                    let other = NpmVersion::new(range)?;
                    Ok(self.compare(&other) == Ordering::Equal)
                }
            }
        }
    }
    
    /// Satisfacer rango caret (^)
    fn satisfies_caret(&self, version_str: &str) -> Result<bool, NpmVersionValidationError> {
        let base = NpmVersion::new(version_str)?;
        
        if base.major == 0 {
            if base.minor == 0 {
                // ^0.0.x := >=0.0.x <0.0.(x+1)
                Ok(self.major == base.major && 
                   self.minor == base.minor && 
                   self.patch >= base.patch &&
                   self.patch < base.patch + 1)
            } else {
                // ^0.x.y := >=0.x.y <0.(x+1).0
                Ok(self.major == base.major && 
                   self.minor >= base.minor &&
                   self.minor < base.minor + 1)
            }
        } else {
            // ^x.y.z := >=x.y.z <(x+1).0.0
            Ok(self.major == base.major && 
               self.compare(&base) != Ordering::Less)
        }
    }
    
    /// Satisfacer rango tilde (~)
    fn satisfies_tilde(&self, version_str: &str) -> Result<bool, NpmVersionValidationError> {
        let base = NpmVersion::new(version_str)?;
        
        // ~x.y.z := >=x.y.z <x.(y+1).0
        Ok(self.major == base.major && 
           self.minor == base.minor && 
           self.compare(&base) != Ordering::Less &&
           self.patch < base.patch + 1)
    }
    
    /// Incrementar versión según el tipo
    pub fn increment(&self, increment_type: VersionIncrement) -> Self {
        match increment_type {
            VersionIncrement::Major => {
                Self::from_components(self.major + 1, 0, 0, None, None).unwrap()
            },
            VersionIncrement::Minor => {
                Self::from_components(self.major, self.minor + 1, 0, None, None).unwrap()
            },
            VersionIncrement::Patch => {
                Self::from_components(self.major, self.minor, self.patch + 1, None, None).unwrap()
            },
        }
    }
    
    /// Obtener el número de versión principal
    pub fn major(&self) -> u32 {
        self.major
    }
    
    /// Obtener el número de versión menor
    pub fn minor(&self) -> u32 {
        self.minor
    }
    
    /// Obtener el número de versión de parche
    pub fn patch(&self) -> u32 {
        self.patch
    }
    
    /// Obtener el prerelease
    pub fn prerelease(&self) -> Option<&str> {
        self.prerelease.as_ref().map(|s| s.as_str())
    }
    
    /// Obtener la build metadata
    pub fn build_metadata(&self) -> Option<&str> {
        self.build_metadata.as_ref().map(|s| s.as_str())
    }
    
    // Métodos de validación
    
    fn validate_prerelease(prerelease: &str) -> Result<(), NpmVersionValidationError> {
        if prerelease.is_empty() {
            return Err(NpmVersionValidationError::InvalidPrerelease(
                "Prerelease cannot be empty".to_string()
            ));
        }
        
        // Prerelease debe contener solo caracteres alfanuméricos y -
        if !prerelease.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err(NpmVersionValidationError::InvalidPrerelease(
                "Prerelease can only contain alphanumeric characters and hyphens".to_string()
            ));
        }
        
        Ok(())
    }
    
    fn validate_build_metadata(build: &str) -> Result<(), NpmVersionValidationError> {
        if build.is_empty() {
            return Err(NpmVersionValidationError::InvalidBuildMetadata(
                "Build metadata cannot be empty".to_string()
            ));
        }
        
        // Build metadata debe contener solo caracteres alfanuméricos y -
        if !build.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err(NpmVersionValidationError::InvalidBuildMetadata(
                "Build metadata can only contain alphanumeric characters and hyphens".to_string()
            ));
        }
        
        Ok(())
    }
}

/// Tipo de incremento de versión
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VersionIncrement {
    Major,
    Minor,
    Patch,
}

impl PartialOrd for NpmVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.compare(other))
    }
}

impl Ord for NpmVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare(other)
    }
}

impl fmt::Display for NpmVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.original)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_valid_version() {
        let version = NpmVersion::new("1.2.3").unwrap();
        assert_eq!(version.major, 1);
        assert_eq!(version.minor, 2);
        assert_eq!(version.patch, 3);
        assert!(!version.is_prerelease());
        assert!(version.is_stable());
    }
    
    #[test]
    fn test_version_with_prerelease() {
        let version = NpmVersion::new("1.2.3-alpha.1").unwrap();
        assert_eq!(version.major, 1);
        assert_eq!(version.minor, 2);
        assert_eq!(version.patch, 3);
        assert_eq!(version.prerelease(), Some("alpha.1"));
        assert!(version.is_prerelease());
        assert!(!version.is_stable());
    }
    
    #[test]
    fn test_version_with_build() {
        let version = NpmVersion::new("1.2.3+build.123").unwrap();
        assert_eq!(version.major, 1);
        assert_eq!(version.minor, 2);
        assert_eq!(version.patch, 3);
        assert_eq!(version.build_metadata(), Some("build.123"));
    }
    
    #[test]
    fn test_version_comparison() {
        let v1 = NpmVersion::new("1.0.0").unwrap();
        let v2 = NpmVersion::new("2.0.0").unwrap();
        let v3 = NpmVersion::new("1.1.0").unwrap();
        let v4 = NpmVersion::new("1.0.1").unwrap();
        
        assert!(v1 < v2);
        assert!(v1 < v3);
        assert!(v1 < v4);
        assert!(v2 > v3);
    }
    
    #[test]
    fn test_prerelease_comparison() {
        let stable = NpmVersion::new("1.0.0").unwrap();
        let prerelease = NpmVersion::new("1.0.0-alpha").unwrap();
        
        assert!(stable > prerelease);
        assert!(prerelease < stable);
    }
    
    #[test]
    fn test_range_satisfaction() {
        let version = NpmVersion::new("1.2.3").unwrap();
        
        // Versión exacta
        assert!(version.satisfies_range("1.2.3").unwrap());
        assert!(!version.satisfies_range("1.2.4").unwrap());
        
        // Caret range
        assert!(version.satisfies_range("^1.0.0").unwrap());
        assert!(version.satisfies_range("^1.2.0").unwrap());
        assert!(!version.satisfies_range("^2.0.0").unwrap());
        
        // Tilde range
        assert!(version.satisfies_range("~1.2.0").unwrap());
        assert!(!version.satisfies_range("~1.3.0").unwrap());
        
        // Mayor o igual
        assert!(version.satisfies_range(">=1.0.0").unwrap());
        assert!(version.satisfies_range(">=1.2.3").unwrap());
        assert!(!version.satisfies_range(">=2.0.0").unwrap());
    }
    
    #[test]
    fn test_version_increment() {
        let version = NpmVersion::new("1.2.3").unwrap();
        
        let major = version.increment(VersionIncrement::Major);
        assert_eq!(major.to_string(), "2.0.0");
        
        let minor = version.increment(VersionIncrement::Minor);
        assert_eq!(minor.to_string(), "1.3.0");
        
        let patch = version.increment(VersionIncrement::Patch);
        assert_eq!(patch.to_string(), "1.2.4");
    }
    
    #[test]
    fn test_invalid_versions() {
        assert!(NpmVersion::new("").is_err());
        assert!(NpmVersion::new("1").is_err());
        assert!(NpmVersion::new("1.2").is_err());
        assert!(NpmVersion::new("1.2.3.4").is_err());
        assert!(NpmVersion::new("a.b.c").is_err());
        assert!(NpmVersion::new("1.2.3-").is_err());
        assert!(NpmVersion::new("1.2.3+").is_err());
    }
}