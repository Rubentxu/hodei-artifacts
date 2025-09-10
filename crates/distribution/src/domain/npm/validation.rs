// crates/distribution/src/domain/npm/validation.rs

//! Validaciones npm - Funciones puras para validar nombres y versiones

use crate::domain::npm::{NpmPackageName, NpmPackageValidationError, NpmVersion, NpmVersionValidationError};

/// Validar un nombre de paquete npm
pub fn validate_npm_package_name(name: &str) -> Result<(), NpmPackageValidationError> {
    NpmPackageName::new(name)?;
    Ok(())
}

/// Validar una versión npm
pub fn validate_npm_version(version: &str) -> Result<(), NpmVersionValidationError> {
    NpmVersion::new(version)?;
    Ok(())
}

/// Validar un rango de versiones npm
pub fn validate_npm_version_range(range: &str) -> Result<(), NpmVersionValidationError> {
    // Rangos válidos: *, latest, ^1.0.0, ~1.0.0, >=1.0.0, <2.0.0, 1.0.0, etc.
    
    if range == "*" || range == "latest" {
        return Ok(());
    }
    
    // Rangos con prefijos
    if range.starts_with('^') || range.starts_with('~') || 
       range.starts_with(">=") || range.starts_with("<=") ||
       range.starts_with('>') || range.starts_with('<') {
        // Extraer la versión y validarla
        let version_part = if range.len() > 2 {
            &range[2..]
        } else {
            return Err(NpmVersionValidationError::InvalidFormat(
                "Invalid range format".to_string()
            ));
        };
        
        validate_npm_version(version_part)?;
        return Ok(());
    }
    
    // Versión exacta
    validate_npm_version(range)
}

/// Validar un nombre de scope npm
pub fn validate_npm_scope(scope: &str) -> Result<(), NpmPackageValidationError> {
    if !scope.starts_with('@') {
        return Err(NpmPackageValidationError::InvalidScopedFormat(
            "Scope must start with @".to_string()
        ));
    }
    
    let scope_name = &scope[1..]; // Remover el @
    
    if scope_name.is_empty() {
        return Err(NpmPackageValidationError::InvalidScopedFormat(
            "Scope name cannot be empty".to_string()
        ));
    }
    
    if scope_name.len() > 39 {
        return Err(NpmPackageValidationError::InvalidScopedFormat(
            format!("Scope name too long: {} characters (max 39)", scope_name.len())
        ));
    }
    
    // Validar caracteres del scope
    if !scope_name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err(NpmPackageValidationError::InvalidScopedFormat(
            "Scope name contains invalid characters".to_string()
        ));
    }
    
    // No puede empezar ni terminar con - o _
    if scope_name.starts_with('-') || scope_name.starts_with('_') ||
       scope_name.ends_with('-') || scope_name.ends_with('_') {
        return Err(NpmPackageValidationError::InvalidScopedFormat(
            "Scope name cannot start or end with - or _".to_string()
        ));
    }
    
    Ok(())
}

/// Validar un nombre de paquete dentro de un scope
pub fn validate_npm_scoped_package_name(name: &str) -> Result<(), NpmPackageValidationError> {
    if name.is_empty() {
        return Err(NpmPackageValidationError::InvalidName(
            "Package name cannot be empty".to_string()
        ));
    }
    
    if name.len() > 214 {
        return Err(NpmPackageValidationError::NameTooLong(name.len()));
    }
    
    // No puede empezar con . o _
    if name.starts_with('.') || name.starts_with('_') {
        return Err(NpmPackageValidationError::InvalidStartCharacter);
    }
    
    // Validar caracteres
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(NpmPackageValidationError::InvalidCharacters(
            "Package name contains invalid characters".to_string()
        ));
    }
    
    // No puede tener mayúsculas
    if name.chars().any(|c| c.is_uppercase()) {
        return Err(NpmPackageValidationError::InvalidCharacters(
            "Package names must be lowercase".to_string()
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

/// Validar una lista de dependencias
pub fn validate_npm_dependencies(dependencies: &[(String, String)]) -> Result<(), NpmPackageValidationError> {
    for (name, version) in dependencies {
        validate_npm_package_name(name)?;
        validate_npm_version_range(version)?;
    }
    Ok(())
}

/// Validar un archivo package.json
pub fn validate_package_json(package_json: &serde_json::Value) -> Result<(), NpmPackageValidationError> {
    if let Some(name) = package_json.get("name").and_then(|n| n.as_str()) {
        validate_npm_package_name(name)?;
    } else {
        return Err(NpmPackageValidationError::InvalidName(
            "Package name is required".to_string()
        ));
    }
    
    if let Some(version) = package_json.get("version").and_then(|v| v.as_str()) {
        validate_npm_version(version)?;
    }
    
    // Validar dependencias si existen
    if let Some(deps) = package_json.get("dependencies").and_then(|d| d.as_object()) {
        for (name, version_value) in deps {
            if let Some(version) = version_value.as_str() {
                validate_npm_version_range(version)?;
            }
        }
    }
    
    if let Some(dev_deps) = package_json.get("devDependencies").and_then(|d| d.as_object()) {
        for (name, version_value) in dev_deps {
            if let Some(version) = version_value.as_str() {
                validate_npm_version_range(version)?;
            }
        }
    }
    
    if let Some(peer_deps) = package_json.get("peerDependencies").and_then(|d| d.as_object()) {
        for (name, version_value) in peer_deps {
            if let Some(version) = version_value.as_str() {
                validate_npm_version_range(version)?;
            }
        }
    }
    
    Ok(())
}

/// Validar un tarball npm (.tgz)
pub fn validate_npm_tarball_name(name: &str) -> Result<(), NpmPackageValidationError> {
    // Formato esperado: nombre-paquete-version.tgz o @scope/nombre-paquete-version.tgz
    
    if !name.ends_with(".tgz") {
        return Err(NpmPackageValidationError::InvalidName(
            "Tarball name must end with .tgz".to_string()
        ));
    }
    
    let base_name = &name[..name.len() - 4]; // Remover .tgz
    
    // Verificar si es un paquete con scope
    if base_name.contains('/') {
        let parts: Vec<&str> = base_name.split('/').collect();
        if parts.len() != 2 {
            return Err(NpmPackageValidationError::InvalidName(
                "Invalid scoped package tarball format".to_string()
            ));
        }
        
        let scope = parts[0];
        let package_version = parts[1];
        
        // Validar scope
        validate_npm_scope(&format!("@{}", scope))?;
        
        // El resto debe ser nombre-version
        if !package_version.contains('-') {
            return Err(NpmPackageValidationError::InvalidName(
                "Tarball name must contain version".to_string()
            ));
        }
        
        // Extraer nombre y versión
        let name_parts: Vec<&str> = package_version.rsplitn(2, '-').collect();
        if name_parts.len() != 2 {
            return Err(NpmPackageValidationError::InvalidName(
                "Cannot extract package name and version from tarball".to_string()
            ));
        }
        
        let version = name_parts[0];
        let package_name = name_parts[1];
        
        validate_npm_scoped_package_name(package_name)?;
        validate_npm_version(version)?;
        
    } else {
        // Paquete sin scope
        if !base_name.contains('-') {
            return Err(NpmPackageValidationError::InvalidName(
                "Tarball name must contain version".to_string()
            ));
        }
        
        // Extraer nombre y versión
        let parts: Vec<&str> = base_name.rsplitn(2, '-').collect();
        if parts.len() != 2 {
            return Err(NpmPackageValidationError::InvalidName(
                "Cannot extract package name and version from tarball".to_string()
            ));
        }
        
        let version = parts[0];
        let package_name = parts[1];
        
        validate_npm_package_name(package_name)?;
        validate_npm_version(version)?;
    }
    
    Ok(())
}

/// Validar una URL de registry npm
pub fn validate_npm_registry_url(url: &str) -> Result<(), NpmPackageValidationError> {
    if url.is_empty() {
        return Err(NpmPackageValidationError::InvalidName(
            "Registry URL cannot be empty".to_string()
        ));
    }
    
    // Validar que sea una URL válida
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(NpmPackageValidationError::InvalidName(
            "Registry URL must start with http:// or https://".to_string()
        ));
    }
    
    // Validar que termine con / (opcional pero común)
    if !url.ends_with('/') {
        // No es un error, pero podríamos normalizarlo
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_validate_npm_package_name() {
        assert!(validate_npm_package_name("my-package").is_ok());
        assert!(validate_npm_package_name("@myscope/my-package").is_ok());
        assert!(validate_npm_package_name("test_package").is_ok());
        assert!(validate_npm_package_name("test.package").is_ok());
        
        assert!(validate_npm_package_name("").is_err());
        assert!(validate_npm_package_name(".my-package").is_err());
        assert!(validate_npm_package_name("_my-package").is_err());
        assert!(validate_npm_package_name("My-Package").is_err());
        assert!(validate_npm_package_name("my package").is_err());
        assert!(validate_npm_package_name("my..package").is_err());
    }
    
    #[test]
    fn test_validate_npm_version() {
        assert!(validate_npm_version("1.0.0").is_ok());
        assert!(validate_npm_version("1.2.3").is_ok());
        assert!(validate_npm_version("1.0.0-alpha").is_ok());
        assert!(validate_npm_version("1.0.0+build123").is_ok());
        assert!(validate_npm_version("1.0.0-alpha.1+build123").is_ok());
        
        assert!(validate_npm_version("").is_err());
        assert!(validate_npm_version("1").is_err());
        assert!(validate_npm_version("1.2").is_err());
        assert!(validate_npm_version("a.b.c").is_err());
    }
    
    #[test]
    fn test_validate_npm_version_range() {
        assert!(validate_npm_version_range("*").is_ok());
        assert!(validate_npm_version_range("latest").is_ok());
        assert!(validate_npm_version_range("1.0.0").is_ok());
        assert!(validate_npm_version_range("^1.0.0").is_ok());
        assert!(validate_npm_version_range("~1.0.0").is_ok());
        assert!(validate_npm_version_range(">=1.0.0").is_ok());
        assert!(validate_npm_version_range("<2.0.0").is_ok());
        assert!(validate_npm_version_range(">1.0.0").is_ok());
        assert!(validate_npm_version_range("<=1.0.0").is_ok());
        
        assert!(validate_npm_version_range("^invalid").is_err());
        assert!(validate_npm_version_range(">=invalid").is_err());
    }
    
    #[test]
    fn test_validate_npm_scope() {
        assert!(validate_npm_scope("@myscope").is_ok());
        assert!(validate_npm_scope("@test-scope").is_ok());
        assert!(validate_npm_scope("@test_scope").is_ok());
        
        assert!(validate_npm_scope("myscope").is_err());
        assert!(validate_npm_scope("@").is_err());
        assert!(validate_npm_scope("@-invalid").is_err());
        assert!(validate_npm_scope("@_invalid").is_err());
    }
    
    #[test]
    fn test_validate_npm_scoped_package_name() {
        assert!(validate_npm_scoped_package_name("my-package").is_ok());
        assert!(validate_npm_scoped_package_name("test_package").is_ok());
        assert!(validate_npm_scoped_package_name("test.package").is_ok());
        
        assert!(validate_npm_scoped_package_name("").is_err());
        assert!(validate_npm_scoped_package_name(".my-package").is_err());
        assert!(validate_npm_scoped_package_name("_my-package").is_err());
        assert!(validate_npm_scoped_package_name("My-Package").is_err());
        assert!(validate_npm_scoped_package_name("my package").is_err());
        assert!(validate_npm_scoped_package_name("my..package").is_err());
    }
    
    #[test]
    fn test_validate_npm_tarball_name() {
        assert!(validate_npm_tarball_name("my-package-1.0.0.tgz").is_ok());
        assert!(validate_npm_tarball_name("test_package-2.1.0.tgz").is_ok());
        assert!(validate_npm_tarball_name("@myscope/my-package-1.0.0.tgz").is_ok());
        
        assert!(validate_npm_tarball_name("invalid.tgz").is_err());
        assert!(validate_npm_tarball_name("my-package.tgz").is_err());
        assert!(validate_npm_tarball_name("my-package-1.0.0.tar.gz").is_err());
    }
    
    #[test]
    fn test_validate_npm_registry_url() {
        assert!(validate_npm_registry_url("https://registry.npmjs.org/").is_ok());
        assert!(validate_npm_registry_url("http://localhost:4873/").is_ok());
        assert!(validate_npm_registry_url("https://npm.company.com/").is_ok());
        
        assert!(validate_npm_registry_url("").is_err());
        assert!(validate_npm_registry_url("ftp://registry.npmjs.org/").is_err());
        assert!(validate_npm_registry_url("registry.npmjs.org/").is_err());
    }
}