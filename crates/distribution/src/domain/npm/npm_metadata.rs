// crates/distribution/src/domain/npm/npm_metadata.rs

use serde::{Serialize, Deserialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use crate::domain::error::{FormatError, DistributionResult};
use shared::hrn::RepositoryId;
use std::collections::HashMap;

/// Metadata de un paquete npm individual
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmPackageVersion {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub main: Option<String>,
    pub scripts: HashMap<String, String>,
    pub dependencies: HashMap<String, String>,
    pub dev_dependencies: HashMap<String, String>,
    pub peer_dependencies: HashMap<String, String>,
    pub optional_dependencies: HashMap<String, String>,
    pub bundled_dependencies: Vec<String>,
    pub keywords: Vec<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub repository: Option<RepositoryInfo>,
    pub bugs: Option<BugsInfo>,
    pub homepage: Option<String>,
    pub engines: HashMap<String, String>,
    pub os: Vec<String>,
    pub cpu: Vec<String>,
    pub dist: DistInfo,
    pub directories: Option<DirectoriesInfo>,
    pub files: Vec<String>,
    publish_config: Option<HashMap<String, serde_json::Value>>,
    pub _has_shrinkwrap: Option<bool>,
}

/// Información del repositorio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryInfo {
    #[serde(rename = "type")]
    pub repo_type: String,
    pub url: String,
    pub directory: Option<String>,
}

/// Información de bugs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugsInfo {
    pub url: Option<String>,
    pub email: Option<String>,
}

/// Información de distribución
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistInfo {
    pub integrity: String,
    pub shasum: String,
    pub tarball: String,
    pub file_count: Option<u64>,
    pub unpacked_size: Option<u64>,
    pub npm_signature: Option<String>,
}

/// Información de directorios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoriesInfo {
    pub lib: Option<String>,
    pub bin: Option<String>,
    pub man: Option<String>,
    pub doc: Option<String>,
    pub example: Option<String>,
    pub test: Option<String>,
}

/// Metadata completa del paquete npm para el registro
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmPackageMetadata {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,
    pub versions: HashMap<String, NpmPackageVersion>,
    pub time: HashMap<String, String>,
    pub maintainers: Vec<PersonInfo>,
    pub author: Option<PersonInfo>,
    pub license: Option<String>,
    pub readme: Option<String>,
    pub readme_filename: Option<String>,
    pub homepage: Option<String>,
    pub keywords: Vec<String>,
    pub repository: Option<RepositoryInfo>,
    pub bugs: Option<BugsInfo>,
    pub users: HashMap<String, bool>,
}

/// Información de persona (autor, mantenedor)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonInfo {
    pub name: String,
    pub email: Option<String>,
    pub url: Option<String>,
}

/// Generador de metadata npm
pub struct NpmMetadataGenerator;

impl NpmMetadataGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Genera metadata completa para un paquete npm
    pub fn generate_package_metadata(
        &self,
        package_name: &str,
        versions: Vec<(String, NpmPackageVersion)>,
        dist_tags: HashMap<String, String>,
    ) -> DistributionResult<NpmPackageMetadata> {
        let mut versions_map = HashMap::new();
        let mut time_map = HashMap::new();
        
        // Generar timestamps para cada versión
        let now = OffsetDateTime::now_utc();
        time_map.insert("created".to_string(), now.format(&Rfc3339).unwrap());
        time_map.insert("modified".to_string(), now.format(&Rfc3339).unwrap());

        for (version, version_metadata) in versions {
            time_map.insert(version.clone(), now.format(&Rfc3339).unwrap());
            versions_map.insert(version, version_metadata);
        }

        // Crear metadata del paquete
        let metadata = NpmPackageMetadata {
            name: package_name.to_string(),
            description: Some(format!("npm package {}", package_name)),
            dist_tags,
            versions: versions_map,
            time: time_map,
            maintainers: vec![PersonInfo {
                name: "system".to_string(),
                email: None,
                url: None,
            }],
            author: Some(PersonInfo {
                name: "system".to_string(),
                email: None,
                url: None,
            }),
            license: Some("MIT".to_string()),
            readme: Some(format!("# {}\n\nPackage description", package_name)),
            readme_filename: Some("README.md".to_string()),
            homepage: Some(format!("https://www.npmjs.com/package/{}", package_name)),
            keywords: vec!["npm".to_string(), "package".to_string()],
            repository: Some(RepositoryInfo {
                repo_type: "git".to_string(),
                url: format!("https://github.com/user/{}.git", package_name),
                directory: None,
            }),
            bugs: Some(BugsInfo {
                url: Some(format!("https://github.com/user/{}/issues", package_name)),
                email: None,
            }),
            users: HashMap::new(),
        };

        Ok(metadata)
    }

    /// Genera metadata para una versión específica
    pub fn generate_version_metadata(
        &self,
        package_name: &str,
        version: &str,
        tarball_url: &str,
        shasum: &str,
        integrity: &str,
    ) -> DistributionResult<NpmPackageVersion> {
        let now = OffsetDateTime::now_utc();

        let version_metadata = NpmPackageVersion {
            name: package_name.to_string(),
            version: version.to_string(),
            description: Some(format!("{} version {}", package_name, version)),
            main: Some("index.js".to_string()),
            scripts: HashMap::new(),
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            peer_dependencies: HashMap::new(),
            optional_dependencies: HashMap::new(),
            bundled_dependencies: vec![],
            keywords: vec!["npm".to_string(), "package".to_string()],
            author: Some("System <system@example.com>".to_string()),
            license: Some("MIT".to_string()),
            repository: Some(RepositoryInfo {
                repo_type: "git".to_string(),
                url: format!("https://github.com/user/{}.git", package_name),
                directory: None,
            }),
            bugs: Some(BugsInfo {
                url: Some(format!("https://github.com/user/{}/issues", package_name)),
                email: None,
            }),
            homepage: Some(format!("https://github.com/user/{}", package_name)),
            engines: HashMap::new(),
            os: vec![],
            cpu: vec![],
            dist: DistInfo {
                integrity: integrity.to_string(),
                shasum: shasum.to_string(),
                tarball: tarball_url.to_string(),
                file_count: Some(10),
                unpacked_size: Some(1024),
                npm_signature: None,
            },
            directories: None,
            files: vec!["index.js".to_string(), "package.json".to_string()],
            publish_config: None,
            _has_shrinkwrap: Some(false),
        };

        Ok(version_metadata)
    }

    /// Genera dist-tags por defecto
    pub fn generate_default_dist_tags(&self, versions: &[String]) -> HashMap<String, String> {
        let mut dist_tags = HashMap::new();
        
        if let Some(latest) = versions.last() {
            dist_tags.insert("latest".to_string(), latest.clone());
        }

        dist_tags
    }

    /// Parsea package.json existente
    pub fn parse_package_json(&self, json_content: &str) -> DistributionResult<NpmPackageVersion> {
        serde_json::from_str(json_content)
            .map_err(|e| FormatError::NpmError(format!("Failed to parse package.json: {}", e)).into())
    }

    /// Convierte metadata a JSON para respuesta
    pub fn metadata_to_json(&self, metadata: &NpmPackageMetadata) -> DistributionResult<String> {
        serde_json::to_string_pretty(metadata)
            .map_err(|e| FormatError::NpmError(format!("Failed to serialize metadata: {}", e)).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_package_metadata() {
        let generator = NpmMetadataGenerator::new();
        
        let versions = vec![
            ("1.0.0".to_string(), create_test_version("my-package", "1.0.0")),
            ("1.1.0".to_string(), create_test_version("my-package", "1.1.0")),
        ];
        
        let mut dist_tags = HashMap::new();
        dist_tags.insert("latest".to_string(), "1.1.0".to_string());
        
        let metadata = generator.generate_package_metadata("my-package", versions, dist_tags).unwrap();
        
        assert_eq!(metadata.name, "my-package");
        assert_eq!(metadata.versions.len(), 2);
        assert!(metadata.versions.contains_key("1.0.0"));
        assert!(metadata.versions.contains_key("1.1.0"));
        assert_eq!(metadata.dist_tags.get("latest"), Some(&"1.1.0".to_string()));
    }

    #[test]
    fn test_generate_version_metadata() {
        let generator = NpmMetadataGenerator::new();
        
        let version_metadata = generator.generate_version_metadata(
            "my-package",
            "1.0.0",
            "https://registry.npmjs.org/my-package/-/my-package-1.0.0.tgz",
            "abc123",
            "sha512-xyz789"
        ).unwrap();
        
        assert_eq!(version_metadata.name, "my-package");
        assert_eq!(version_metadata.version, "1.0.0");
        assert_eq!(version_metadata.dist.tarball, "https://registry.npmjs.org/my-package/-/my-package-1.0.0.tgz");
        assert_eq!(version_metadata.dist.shasum, "abc123");
        assert_eq!(version_metadata.dist.integrity, "sha512-xyz789");
    }

    #[test]
    fn test_generate_default_dist_tags() {
        let generator = NpmMetadataGenerator::new();
        
        let versions = vec![
            "1.0.0".to_string(),
            "1.1.0".to_string(),
            "2.0.0".to_string(),
        ];
        
        let dist_tags = generator.generate_default_dist_tags(&versions);
        
        assert_eq!(dist_tags.get("latest"), Some(&"2.0.0".to_string()));
        assert_eq!(dist_tags.len(), 1);
    }

    #[test]
    fn test_parse_package_json() {
        let generator = NpmMetadataGenerator::new();
        
        let package_json = r#"{
            "name": "test-package",
            "version": "1.0.0",
            "description": "A test package",
            "main": "index.js",
            "scripts": {
                "test": "jest"
            },
            "dependencies": {
                "lodash": "^4.17.21"
            },
            "author": "Test Author",
            "license": "MIT"
        }"#;
        
        let version_metadata = generator.parse_package_json(package_json).unwrap();
        
        assert_eq!(version_metadata.name, "test-package");
        assert_eq!(version_metadata.version, "1.0.0");
        assert_eq!(version_metadata.description, Some("A test package".to_string()));
        assert_eq!(version_metadata.main, Some("index.js".to_string()));
        assert_eq!(version_metadata.author, Some("Test Author".to_string()));
        assert_eq!(version_metadata.license, Some("MIT".to_string()));
        assert!(version_metadata.dependencies.contains_key("lodash"));
    }

    #[test]
    fn test_metadata_to_json() {
        let generator = NpmMetadataGenerator::new();
        
        let mut versions = HashMap::new();
        versions.insert("1.0.0".to_string(), create_test_version("my-package", "1.0.0"));
        
        let metadata = NpmPackageMetadata {
            name: "my-package".to_string(),
            description: Some("Test package".to_string()),
            dist_tags: HashMap::new(),
            versions,
            time: HashMap::new(),
            maintainers: vec![],
            author: None,
            license: Some("MIT".to_string()),
            readme: None,
            readme_filename: None,
            homepage: None,
            keywords: vec![],
            repository: None,
            bugs: None,
            users: HashMap::new(),
        };
        
        let json = generator.metadata_to_json(&metadata).unwrap();
        
        assert!(json.contains("\"name\": \"my-package\""));
        assert!(json.contains("\"description\": \"Test package\""));
        assert!(json.contains("\"license\": \"MIT\""));
    }

    // Helper function para crear versiones de prueba
    fn create_test_version(package_name: &str, version: &str) -> NpmPackageVersion {
        NpmPackageVersion {
            name: package_name.to_string(),
            version: version.to_string(),
            description: Some(format!("{} version {}", package_name, version)),
            main: Some("index.js".to_string()),
            scripts: HashMap::new(),
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            peer_dependencies: HashMap::new(),
            optional_dependencies: HashMap::new(),
            bundled_dependencies: vec![],
            keywords: vec![],
            author: Some("Test Author".to_string()),
            license: Some("MIT".to_string()),
            repository: None,
            bugs: None,
            homepage: None,
            engines: HashMap::new(),
            os: vec![],
            cpu: vec![],
            dist: DistInfo {
                integrity: "sha512-test".to_string(),
                shasum: "test123".to_string(),
                tarball: format!("https://registry.npmjs.org/{}/-/{}-{}.tgz", package_name, package_name, version),
                file_count: Some(5),
                unpacked_size: Some(512),
                npm_signature: None,
            },
            directories: None,
            files: vec![],
            publish_config: None,
            _has_shrinkwrap: Some(false),
        }
    }
}