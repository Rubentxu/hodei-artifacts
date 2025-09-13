// crates/distribution/src/domain/npm/package.rs

//! Value Objects para paquetes npm - Dominio puro sin dependencias externas

use std::fmt;
use thiserror::Error;
use serde::{Serialize, Deserialize};

/// Error de validación para paquetes npm
#[derive(Debug, Error, Clone, PartialEq)]
pub enum NpmPackageValidationError {
    #[error("Invalid package name: {0}")]
    InvalidName(String),
    #[error("Package name too long: {0} characters (max 214)")]
    NameTooLong(usize),
    #[error("Package name cannot start with '.' or '_'")]
    InvalidStartCharacter,
    #[error("Package name contains invalid characters: {0}")]
    InvalidCharacters(String),
    #[error("Scoped package format invalid: {0}")]
    InvalidScopedFormat(String),
}

/// Nombre de paquete npm - Value Object inmutable
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NpmPackageName {
    pub full_name: String,
    pub scope: Option<String>,
    pub name: String,
    pub is_scoped: bool,
}

impl NpmPackageName {
    /// Crear un nombre de paquete npm con validación
    pub fn new(name: &str) -> Result<Self, NpmPackageValidationError> {
        Self::validate_name(name)?;
        
        let (scope, package_name, is_scoped) = if name.starts_with('@') {
            // Paquete con scope
            let parts: Vec<&str> = name.split('/').collect();
            if parts.len() != 2 {
                return Err(NpmPackageValidationError::InvalidScopedFormat(
                    "Scoped packages must have format @scope/name".to_string()
                ));
            }
            
            let scope = parts[0].to_string();
            let package_name = parts[1].to_string();
            
            // Validar que el scope empiece con @
            if !scope.starts_with('@') {
                return Err(NpmPackageValidationError::InvalidScopedFormat(
                    "Scope must start with @".to_string()
                ));
            }
            
            // Validar el nombre del paquete dentro del scope
            Self::validate_package_name(&package_name)?;
            
            (Some(scope), package_name, true)
        } else {
            // Paquete sin scope
            Self::validate_package_name(name)?;
            (None, name.to_string(), false)
        };
        
        Ok(Self {
            full_name: name.to_string(),
            scope,
            name: package_name,
            is_scoped,
        })
    }
    
    /// Crear desde partes separadas
    pub fn from_parts(scope: Option<&str>, name: &str) -> Result<Self, NpmPackageValidationError> {
        let full_name = if let Some(scope) = scope {
            format!("{}/{}", scope, name)
        } else {
            name.to_string()
        };
        
        Self::new(&full_name)
    }
    
    /// Obtener el nombre completo
    pub fn full_name(&self) -> &str {
        &self.full_name
    }
    
    /// Obtener el scope (sin @)
    pub fn scope_name(&self) -> Option<&str> {
        self.scope.as_ref().map(|s| &s[1..]) // Remover el @
    }
    
    /// Obtener solo el nombre del paquete
    pub fn package_name(&self) -> &str {
        &self.name
    }
    
    /// Convertir a path para URLs
    pub fn to_path(&self) -> String {
        if self.is_scoped {
            format!("{}/{}", self.scope.as_ref().unwrap(), self.name)
        } else {
            self.name.clone()
        }
    }
    
    /// Convertir a path para el tarball
    pub fn to_tarball_name(&self, version: &str) -> String {
        format!("{}-{}.tgz", self.name, version)
    }
    
    /// Validar si es un nombre válido de paquete
    pub fn is_valid(name: &str) -> bool {
        Self::new(name).is_ok()
    }
    
    // Métodos de validación privados
    
    fn validate_name(name: &str) -> Result<(), NpmPackageValidationError> {
        if name.is_empty() {
            return Err(NpmPackageValidationError::InvalidName("Package name cannot be empty".to_string()));
        }
        
        if name.len() > 214 {
            return Err(NpmPackageValidationError::NameTooLong(name.len()));
        }
        
        // No puede empezar con . o _
        if name.starts_with('.') || name.starts_with('_') {
            return Err(NpmPackageValidationError::InvalidStartCharacter);
        }
        
        // Validar caracteres permitidos
        Self::validate_package_name(name)?;
        
        Ok(())
    }
    
    fn validate_package_name(name: &str) -> Result<(), NpmPackageValidationError> {
        // Para paquetes sin scope o la parte del nombre en paquetes con scope
        
        // No puede empezar con . o _
        if name.starts_with('.') || name.starts_with('_') {
            return Err(NpmPackageValidationError::InvalidStartCharacter);
        }
        
        // Caracteres permitidos: letras, números, -, _, .
        // Pero no puede tener mayúsculas
        if name.chars().any(|c| c.is_uppercase()) {
            return Err(NpmPackageValidationError::InvalidCharacters(
                "Package names must be lowercase".to_string()
            ));
        }
        
        // Verificar caracteres especiales
        let invalid_chars: Vec<char> = name.chars()
            .filter(|&c| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.')
            .collect();
        
        if !invalid_chars.is_empty() {
            return Err(NpmPackageValidationError::InvalidCharacters(
                format!("Invalid characters: {:?}", invalid_chars)
            ));
        }
        
        // No puede tener espacios
        if name.contains(' ') {
            return Err(NpmPackageValidationError::InvalidCharacters(
                "Package names cannot contain spaces".to_string()
            ));
        }
        
        // No puede tener múltiples puntos consecutivos
        if name.contains("..") {
            return Err(NpmPackageValidationError::InvalidCharacters(
                "Package names cannot contain consecutive dots".to_string()
            ));
        }
        
        Ok(())
    }
}

/// Paquete npm completo con información básica
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpmPackage {
    pub name: NpmPackageName,
    pub version: String,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub homepage: Option<String>,
    pub bugs: Option<BugsInfo>,
    pub license: Option<String>,
    pub author: Option<AuthorInfo>,
    pub contributors: Vec<AuthorInfo>,
    pub files: Vec<String>,
    pub main: Option<String>,
    pub bin: Option<std::collections::HashMap<String, String>>,
    pub man: Vec<String>,
    pub directories: Option<DirectoriesInfo>,
    pub repository: Option<RepositoryInfo>,
    pub scripts: std::collections::HashMap<String, String>,
    pub config: Option<serde_json::Value>,
    pub dependencies: std::collections::HashMap<String, String>,
    pub dev_dependencies: std::collections::HashMap<String, String>,
    pub peer_dependencies: std::collections::HashMap<String, String>,
    pub bundled_dependencies: Vec<String>,
    pub optional_dependencies: std::collections::HashMap<String, String>,
    pub engines: std::collections::HashMap<String, String>,
    pub os: Vec<String>,
    pub cpu: Vec<String>,
    pub private: bool,
    publish_config: Option<PublishConfigInfo>,
}

/// Información de bugs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BugsInfo {
    pub url: Option<String>,
    pub email: Option<String>,
}

/// Información del autor
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthorInfo {
    pub name: String,
    pub email: Option<String>,
    pub url: Option<String>,
}

/// Información de directorios
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectoriesInfo {
    pub lib: Option<String>,
    pub bin: Option<String>,
    pub man: Option<String>,
    pub doc: Option<String>,
    pub example: Option<String>,
    pub test: Option<String>,
}

/// Información del repositorio
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryInfo {
    pub type_: String,
    pub url: String,
    pub directory: Option<String>,
}

/// Configuración de publicación
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishConfigInfo {
    pub registry: Option<String>,
    pub access: Option<String>,
    pub tag: Option<String>,
}

impl NpmPackage {
    /// Crear un paquete npm básico
    pub fn new(name: NpmPackageName, version: &str) -> Result<Self, NpmPackageValidationError> {
        validate_npm_version(version)?;
        
        Ok(Self {
            name,
            version: version.to_string(),
            description: None,
            keywords: Vec::new(),
            homepage: None,
            bugs: None,
            license: None,
            author: None,
            contributors: Vec::new(),
            files: Vec::new(),
            main: None,
            bin: None,
            man: Vec::new(),
            directories: None,
            repository: None,
            scripts: std::collections::HashMap::new(),
            config: None,
            dependencies: std::collections::HashMap::new(),
            dev_dependencies: std::collections::HashMap::new(),
            peer_dependencies: std::collections::HashMap::new(),
            bundled_dependencies: Vec::new(),
            optional_dependencies: std::collections::HashMap::new(),
            engines: std::collections::HashMap::new(),
            os: Vec::new(),
            cpu: Vec::new(),
            private: false,
            publish_config: None,
        })
    }
    
    /// Verificar si es un paquete privado
    pub fn is_private(&self) -> bool {
        self.private
    }
    
    /// Obtener el nombre del tarball
    pub fn tarball_name(&self) -> String {
        self.name.to_tarball_name(&self.version)
    }
    
    /// Verificar si tiene dependencias
    pub fn has_dependencies(&self) -> bool {
        !self.dependencies.is_empty() || 
        !self.dev_dependencies.is_empty() || 
        !self.peer_dependencies.is_empty() || 
        !self.optional_dependencies.is_empty()
    }
    
    /// Obtener todas las dependencias combinadas
    pub fn all_dependencies(&self) -> std::collections::HashMap<String, String> {
        let mut all = std::collections::HashMap::new();
        
        // Agregar dependencias normales
        for (name, version) in &self.dependencies {
            all.insert(name.clone(), version.clone());
        }
        
        // Agregar dependencias de desarrollo
        for (name, version) in &self.dev_dependencies {
            all.insert(format!("{} (dev)", name), version.clone());
        }
        
        // Agregar peer dependencies
        for (name, version) in &self.peer_dependencies {
            all.insert(format!("{} (peer)", name), version.clone());
        }
        
        // Agregar dependencias opcionales
        for (name, version) in &self.optional_dependencies {
            all.insert(format!("{} (optional)", name), version.clone());
        }
        
        all
    }
}

impl fmt::Display for NpmPackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.full_name)
    }
}

impl fmt::Display for NpmPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_valid_package_name() {
        let name = NpmPackageName::new("my-package").unwrap();
        assert_eq!(name.full_name(), "my-package");
        assert_eq!(name.package_name(), "my-package");
        assert!(!name.is_scoped);
        assert_eq!(name.to_path(), "my-package");
    }
    
    #[test]
    fn test_scoped_package_name() {
        let name = NpmPackageName::new("@myscope/my-package").unwrap();
        assert_eq!(name.full_name(), "@myscope/my-package");
        assert_eq!(name.scope_name(), Some("myscope"));
        assert_eq!(name.package_name(), "my-package");
        assert!(name.is_scoped);
        assert_eq!(name.to_path(), "@myscope/my-package");
    }
    
    #[test]
    fn test_invalid_package_names() {
        // Vacío
        assert!(NpmPackageName::new("").is_err());
        
        // Empieza con .
        assert!(NpmPackageName::new(".my-package").is_err());
        
        // Empieza con _
        assert!(NpmPackageName::new("_my-package").is_err());
        
        // Con mayúsculas
        assert!(NpmPackageName::new("My-Package").is_err());
        
        // Con espacios
        assert!(NpmPackageName::new("my package").is_err());
        
        // Con caracteres especiales
        assert!(NpmPackageName::new("my@package").is_err());
        
        // Puntos consecutivos
        assert!(NpmPackageName::new("my..package").is_err());
    }
    
    #[test]
    fn test_package_creation() {
        let name = NpmPackageName::new("test-package").unwrap();
        let package = NpmPackage::new(name, "1.0.0").unwrap();
        
        assert_eq!(package.to_string(), "test-package@1.0.0");
        assert_eq!(package.tarball_name(), "test-package-1.0.0.tgz");
        assert!(!package.is_private());
        assert!(!package.has_dependencies());
    }
    
    #[test]
    fn test_scoped_package_creation() {
        let name = NpmPackageName::new("@myscope/test-package").unwrap();
        let package = NpmPackage::new(name, "2.1.0").unwrap();
        
        assert_eq!(package.to_string(), "@myscope/test-package@2.1.0");
        assert_eq!(package.tarball_name(), "test-package-2.1.0.tgz");
    }
}