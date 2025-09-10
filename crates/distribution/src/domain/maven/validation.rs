// crates/distribution/src/domain/maven/validation.rs

//! Funciones de validación para Maven - Lógica pura sin dependencias

use crate::domain::maven::coordinates::{MavenCoordinates, MavenValidationError};

/// Validar coordenadas Maven completas
pub fn validate_maven_coordinates(coordinates: &MavenCoordinates) -> Result<(), MavenValidationError> {
    // Reutilizar las validaciones internas de MavenCoordinates
    MavenCoordinates::validate_group_id(&coordinates.group_id)?;
    MavenCoordinates::validate_artifact_id(&coordinates.artifact_id)?;
    MavenCoordinates::validate_version(&coordinates.version)?;
    
    if let Some(classifier) = &coordinates.classifier {
        MavenCoordinates::validate_classifier(classifier)?;
    }
    
    MavenCoordinates::validate_extension(&coordinates.extension)?;
    
    Ok(())
}

/// Validar un Group ID Maven
pub fn validate_maven_group_id(group_id: &str) -> Result<(), MavenValidationError> {
    MavenCoordinates::validate_group_id(group_id)
}

/// Validar un Artifact ID Maven
pub fn validate_maven_artifact_id(artifact_id: &str) -> Result<(), MavenValidationError> {
    MavenCoordinates::validate_artifact_id(artifact_id)
}

/// Validar una versión Maven
pub fn validate_maven_version(version: &str) -> Result<(), MavenValidationError> {
    MavenCoordinates::validate_version(version)
}

/// Validar un clasificador Maven
pub fn validate_maven_classifier(classifier: &str) -> Result<(), MavenValidationError> {
    MavenCoordinates::validate_classifier(classifier)
}

/// Validar una extensión Maven
pub fn validate_maven_extension(extension: &str) -> Result<(), MavenValidationError> {
    MavenCoordinates::validate_extension(extension)
}

/// Validar si una versión es válida según las reglas de Maven
pub fn is_valid_maven_version(version: &str) -> bool {
    validate_maven_version(version).is_ok()
}

/// Validar si un Group ID es válido
pub fn is_valid_maven_group_id(group_id: &str) -> bool {
    validate_maven_group_id(group_id).is_ok()
}

/// Validar si un Artifact ID es válido
pub fn is_valid_maven_artifact_id(artifact_id: &str) -> bool {
    validate_maven_artifact_id(artifact_id).is_ok()
}

/// Validar si un path Maven es válido
pub fn is_valid_maven_path(path: &str) -> bool {
    match MavenCoordinates::from_path(path) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Normalizar un Group ID Maven
pub fn normalize_maven_group_id(group_id: &str) -> String {
    let normalized = group_id
        .trim()
        .to_lowercase()
        .replace('_', "-");
    
    // Eliminar caracteres especiales consecutivos
    let mut result = String::new();
    let mut prev_special = false;
    
    for ch in normalized.chars() {
        if ch == '.' || ch == '-' {
            if !prev_special {
                result.push(ch);
                prev_special = true;
            }
        } else {
            result.push(ch);
            prev_special = false;
        }
    }
    
    // Eliminar caracteres especiales al inicio/final
    result.trim_matches(|c| c == '.' || c == '-').to_string()
}

/// Normalizar un Artifact ID Maven
pub fn normalize_maven_artifact_id(artifact_id: &str) -> String {
    let normalized = artifact_id
        .trim()
        .to_lowercase()
        .replace('_', "-");
    
    // Eliminar guiones consecutivos
    let mut result = String::new();
    let mut prev_hyphen = false;
    
    for ch in normalized.chars() {
        if ch == '-' {
            if !prev_hyphen {
                result.push(ch);
                prev_hyphen = true;
            }
        } else {
            result.push(ch);
            prev_hyphen = false;
        }
    }
    
    // Eliminar guiones al inicio/final
    result.trim_matches('-').to_string()
}

/// Comparar dos versiones Maven según las reglas de Maven
pub fn compare_maven_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
    // Implementación simplificada de comparación de versiones Maven
    // En producción, esto sería más complejo con soporte para SNAPSHOT, etc.
    
    // Separar en partes
    let parts1: Vec<&str> = v1.split(&['.', '-', '_'][..]).collect();
    let parts2: Vec<&str> = v2.split(&['.', '-', '_'][..]).collect();
    
    // Comparar parte por parte
    for i in 0..parts1.len().max(parts2.len()) {
        let part1 = parts1.get(i).unwrap_or(&"");
        let part2 = parts2.get(i).unwrap_or(&"");
        
        // Intentar comparar como números
        if let (Ok(num1), Ok(num2)) = (part1.parse::<u32>(), part2.parse::<u32>()) {
            match num1.cmp(&num2) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        } else {
            // Comparar como strings
            match part1.cmp(part2) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
    }
    
    std::cmp::Ordering::Equal
}

/// Determinar si una versión es mayor que otra
pub fn is_maven_version_greater(v1: &str, v2: &str) -> bool {
    compare_maven_versions(v1, v2) == std::cmp::Ordering::Greater
}

/// Determinar si una versión es menor que otra
pub fn is_maven_version_less(v1: &str, v2: &str) -> bool {
    compare_maven_versions(v1, v2) == std::cmp::Ordering::Less
}

/// Determinar si una versión es igual a otra
pub fn is_maven_version_equal(v1: &str, v2: &str) -> bool {
    compare_maven_versions(v1, v2) == std::cmp::Ordering::Equal
}

/// Extraer el número de versión principal (major)
pub fn extract_major_version(version: &str) -> Option<u32> {
    version.split('.').next()
        .and_then(|part| part.parse::<u32>().ok())
}

/// Extraer el número de versión menor (minor)
pub fn extract_minor_version(version: &str) -> Option<u32> {
    version.split('.').nth(1)
        .and_then(|part| {
            // Eliminar cualquier sufijo
            let clean_part = part.split(&['-', '_'][..]).next().unwrap_or("");
            clean_part.parse::<u32>().ok()
        })
}

/// Extraer el número de versión de parche (patch)
pub fn extract_patch_version(version: &str) -> Option<u32> {
    version.split('.').nth(2)
        .and_then(|part| {
            // Eliminar cualquier sufijo
            let clean_part = part.split(&['-', '_'][..]).next().unwrap_or("");
            clean_part.parse::<u32>().ok()
        })
}

/// Determinar si una versión es SNAPSHOT
pub fn is_snapshot_version(version: &str) -> bool {
    version.ends_with("-SNAPSHOT")
}

/// Determinar si una versión es RELEASE
pub fn is_release_version(version: &str) -> bool {
    !is_snapshot_version(version)
}

/// Validar si un path Maven representa un artefacto válido
pub fn is_maven_artifact_path(path: &str) -> bool {
    if let Ok(coordinates) = MavenCoordinates::from_path(path) {
        // Verificar que el filename coincida con las coordenadas
        let expected_filename = if let Some(classifier) = &coordinates.classifier {
            format!("{}-{}-{}.{}", 
                coordinates.artifact_id, 
                coordinates.version, 
                classifier, 
                coordinates.extension)
        } else {
            format!("{}-{}.{}", 
                coordinates.artifact_id, 
                coordinates.version, 
                coordinates.extension)
        };
        
        path.ends_with(&expected_filename)
    } else {
        false
    }
}

/// Validar si un path Maven representa metadata válida
pub fn is_maven_metadata_path(path: &str) -> bool {
    path.ends_with("/maven-metadata.xml") && path.split('/').count() >= 3
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_validate_coordinates() {
        let coords = MavenCoordinates::new("com.example", "my-app", "1.0.0").unwrap();
        assert!(validate_maven_coordinates(&coords).is_ok());
    }
    
    #[test]
    fn test_normalize_group_id() {
        assert_eq!(normalize_maven_group_id("com.example.test"), "com.example.test");
        assert_eq!(normalize_maven_group_id("COM.EXAMPLE.TEST"), "com.example.test");
        assert_eq!(normalize_maven_group_id("com__example..test"), "com.example.test");
        assert_eq!(normalize_maven_group_id(".com.example."), "com.example");
    }
    
    #[test]
    fn test_normalize_artifact_id() {
        assert_eq!(normalize_maven_artifact_id("my-app"), "my-app");
        assert_eq!(normalize_maven_artifact_id("MY_APP"), "my-app");
        assert_eq!(normalize_maven_artifact_id("my--app"), "my-app");
        assert_eq!(normalize_maven_artifact_id("-my-app-"), "my-app");
    }
    
    #[test]
    fn test_compare_versions() {
        assert_eq!(compare_maven_versions("1.0.0", "2.0.0"), std::cmp::Ordering::Less);
        assert_eq!(compare_maven_versions("2.0.0", "1.0.0"), std::cmp::Ordering::Greater);
        assert_eq!(compare_maven_versions("1.0.0", "1.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare_maven_versions("1.0.0", "1.0.1"), std::cmp::Ordering::Less);
    }
    
    #[test]
    fn test_extract_version_parts() {
        assert_eq!(extract_major_version("1.2.3"), Some(1));
        assert_eq!(extract_minor_version("1.2.3"), Some(2));
        assert_eq!(extract_patch_version("1.2.3"), Some(3));
        
        assert_eq!(extract_major_version("1.2.3-SNAPSHOT"), Some(1));
        assert_eq!(extract_minor_version("1.2.3-SNAPSHOT"), Some(2));
        assert_eq!(extract_patch_version("1.2.3-SNAPSHOT"), Some(3));
    }
    
    #[test]
    fn test_is_snapshot_version() {
        assert!(is_snapshot_version("1.0.0-SNAPSHOT"));
        assert!(!is_snapshot_version("1.0.0"));
        assert!(!is_snapshot_version("1.0.0-RELEASE"));
    }
    
    #[test]
    fn test_is_maven_artifact_path() {
        assert!(is_maven_artifact_path("com/example/my-app/1.0.0/my-app-1.0.0.jar"));
        assert!(is_maven_artifact_path("com/example/my-app/1.0.0/my-app-1.0.0-sources.jar"));
        assert!(!is_maven_artifact_path("com/example/my-app/1.0.0/my-app-1.0.0.wrong"));
        assert!(!is_maven_artifact_path("com/example/my-app/1.0.0/wrong-file.jar"));
    }
    
    #[test]
    fn test_is_maven_metadata_path() {
        assert!(is_maven_metadata_path("com/example/my-app/maven-metadata.xml"));
        assert!(is_maven_metadata_path("com/example/maven-metadata.xml"));
        assert!(!is_maven_metadata_path("com/example/my-app/1.0.0/my-app-1.0.0.jar"));
        assert!(!is_maven_metadata_path("maven-metadata.xml"));
    }
}