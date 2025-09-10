// crates/distribution/src/domain/npm/metadata.rs

//! Metadata npm - Entidades para package.json y metadata del repositorio

use std::collections::HashMap;
use time::{OffsetDateTime, Date};
use serde::{Serialize, Deserialize};
use crate::domain::npm::{NpmPackageName, NpmVersion, NpmPackage, NpmPackageValidationError};

/// Metadata de un paquete npm específico
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpmPackageMetadata {
    pub name: NpmPackageName,
    pub version: NpmVersion,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub homepage: Option<String>,
    pub bugs: Option<BugsInfo>,
    pub license: Option<String>,
    pub author: Option<AuthorInfo>,
    pub contributors: Vec<AuthorInfo>,
    pub main: Option<String>,
    pub bin: Option<HashMap<String, String>>,
    pub man: Vec<String>,
    pub directories: Option<DirectoriesInfo>,
    pub repository: Option<RepositoryInfo>,
    pub scripts: HashMap<String, String>,
    pub config: Option<serde_json::Value>,
    pub dependencies: HashMap<String, String>,
    pub dev_dependencies: HashMap<String, String>,
    pub peer_dependencies: HashMap<String, String>,
    pub bundled_dependencies: Vec<String>,
    pub optional_dependencies: HashMap<String, String>,
    pub engines: HashMap<String, String>,
    pub os: Vec<String>,
    pub cpu: Vec<String>,
    pub private: bool,
    pub publish_config: Option<PublishConfigInfo>,
    pub dist: Option<DistInfo>,
    pub _id: Option<String>,
    pub _node_version: Option<String>,
    pub _npm_version: Option<String>,
    pub _npm_user: Option<AuthorInfo>,
    pub maintainers: Vec<AuthorInfo>,
    pub _has_shrinkwrap: Option<bool>,
    pub _from: Option<String>,
    pub _resolved: Option<String>,
    pub _integrity: Option<String>,
    pub _shasum: Option<String>,
    pub _shrinkwrap: Option<serde_json::Value>,
    pub _args: Vec<String>,
    pub _development: Option<bool>,
    pub _optional: Option<bool>,
    pub _required_by: Vec<String>,
}

/// Información de distribución
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistInfo {
    pub integrity: Option<String>,
    pub shasum: Option<String>,
    pub tarball: String,
    pub file_count: Option<u32>,
    pub unpacked_size: Option<u64>,
    pub signatures: Vec<SignatureInfo>,
}

/// Información de firma
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignatureInfo {
    pub keyid: String,
    pub sig: String,
}

/// Información de bugs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BugsInfo {
    pub url: Option<String>,
    pub email: Option<String>,
}

/// Información del autor/maintainer
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

impl NpmPackageMetadata {
    /// Crear metadata desde un paquete
    pub fn from_package(package: &NpmPackage, tarball_url: String) -> Result<Self, NpmPackageValidationError> {
        Ok(Self {
            name: package.name.clone(),
            version: NpmVersion::new(&package.version)?,
            description: package.description.clone(),
            keywords: package.keywords.clone(),
            homepage: package.homepage.clone(),
            bugs: package.bugs.clone(),
            license: package.license.clone(),
            author: package.author.clone(),
            contributors: package.contributors.clone(),
            main: package.main.clone(),
            bin: package.bin.clone(),
            man: package.man.clone(),
            directories: package.directories.clone(),
            repository: package.repository.clone(),
            scripts: package.scripts.clone(),
            config: package.config.clone(),
            dependencies: package.dependencies.clone(),
            dev_dependencies: package.dev_dependencies.clone(),
            peer_dependencies: package.peer_dependencies.clone(),
            bundled_dependencies: package.bundled_dependencies.clone(),
            optional_dependencies: package.optional_dependencies.clone(),
            engines: package.engines.clone(),
            os: package.os.clone(),
            cpu: package.cpu.clone(),
            private: package.private,
            publish_config: package.publish_config.clone(),
            dist: Some(DistInfo {
                integrity: None,
                shasum: None,
                tarball: tarball_url,
                file_count: None,
                unpacked_size: None,
                signatures: Vec::new(),
            }),
            _id: Some(format!("{}@{}", package.name, package.version)),
            _node_version: None,
            _npm_version: None,
            _npm_user: None,
            maintainers: Vec::new(),
            _has_shrinkwrap: None,
            _from: None,
            _resolved: None,
            _integrity: None,
            _shasum: None,
            _shrinkwrap: None,
            _args: Vec::new(),
            _development: None,
            _optional: None,
            _required_by: Vec::new(),
        })
    }
    
    /// Verificar si es un paquete privado
    pub fn is_private(&self) -> bool {
        self.private
    }
    
    /// Obtener el nombre del tarball
    pub fn tarball_name(&self) -> String {
        format!("{}-{}.tgz", self.name.package_name(), self.version.to_string())
    }
    
    /// Obtener la URL del tarball
    pub fn tarball_url(&self) -> Option<&str> {
        self.dist.as_ref().map(|d| d.tarball.as_str())
    }
    
    /// Verificar si tiene dependencias
    pub fn has_dependencies(&self) -> bool {
        !self.dependencies.is_empty() || 
        !self.dev_dependencies.is_empty() || 
        !self.peer_dependencies.is_empty() || 
        !self.optional_dependencies.is_empty()
    }
    
    /// Obtener todas las dependencias combinadas
    pub fn all_dependencies(&self) -> HashMap<String, String> {
        let mut all = HashMap::new();
        
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
    
    /// Convertir a JSON para package.json
    pub fn to_package_json(&self) -> serde_json::Value {
        let mut package_json = serde_json::Map::new();
        
        package_json.insert("name".to_string(), serde_json::Value::String(self.name.full_name().to_string()));
        package_json.insert("version".to_string(), serde_json::Value::String(self.version.to_string()));
        
        if let Some(ref desc) = self.description {
            package_json.insert("description".to_string(), serde_json::Value::String(desc.clone()));
        }
        
        if !self.keywords.is_empty() {
            package_json.insert("keywords".to_string(), 
                serde_json::Value::Array(self.keywords.iter().map(|k| serde_json::Value::String(k.clone())).collect()));
        }
        
        if let Some(ref homepage) = self.homepage {
            package_json.insert("homepage".to_string(), serde_json::Value::String(homepage.clone()));
        }
        
        if let Some(ref bugs) = self.bugs {
            let mut bugs_obj = serde_json::Map::new();
            if let Some(ref url) = bugs.url {
                bugs_obj.insert("url".to_string(), serde_json::Value::String(url.clone()));
            }
            if let Some(ref email) = bugs.email {
                bugs_obj.insert("email".to_string(), serde_json::Value::String(email.clone()));
            }
            package_json.insert("bugs".to_string(), serde_json::Value::Object(bugs_obj));
        }
        
        if let Some(ref license) = self.license {
            package_json.insert("license".to_string(), serde_json::Value::String(license.clone()));
        }
        
        if let Some(ref author) = self.author {
            package_json.insert("author".to_string(), author_to_json(author));
        }
        
        if !self.contributors.is_empty() {
            package_json.insert("contributors".to_string(), 
                serde_json::Value::Array(self.contributors.iter().map(author_to_json).collect()));
        }
        
        if !self.files.is_empty() {
            package_json.insert("files".to_string(), 
                serde_json::Value::Array(self.files.iter().map(|f| serde_json::Value::String(f.clone())).collect()));
        }
        
        if let Some(ref main) = self.main {
            package_json.insert("main".to_string(), serde_json::Value::String(main.clone()));
        }
        
        if let Some(ref bin) = self.bin {
            package_json.insert("bin".to_string(), 
                serde_json::Value::Object(bin.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
        }
        
        if !self.man.is_empty() {
            package_json.insert("man".to_string(), 
                serde_json::Value::Array(self.man.iter().map(|m| serde_json::Value::String(m.clone())).collect()));
        }
        
        if let Some(ref directories) = self.directories {
            let mut dirs_obj = serde_json::Map::new();
            if let Some(ref lib) = directories.lib {
                dirs_obj.insert("lib".to_string(), serde_json::Value::String(lib.clone()));
            }
            if let Some(ref bin) = directories.bin {
                dirs_obj.insert("bin".to_string(), serde_json::Value::String(bin.clone()));
            }
            if let Some(ref man) = directories.man {
                dirs_obj.insert("man".to_string(), serde_json::Value::String(man.clone()));
            }
            if let Some(ref doc) = directories.doc {
                dirs_obj.insert("doc".to_string(), serde_json::Value::String(doc.clone()));
            }
            if let Some(ref example) = directories.example {
                dirs_obj.insert("example".to_string(), serde_json::Value::String(example.clone()));
            }
            if let Some(ref test) = directories.test {
                dirs_obj.insert("test".to_string(), serde_json::Value::String(test.clone()));
            }
            package_json.insert("directories".to_string(), serde_json::Value::Object(dirs_obj));
        }
        
        if let Some(ref repository) = self.repository {
            let mut repo_obj = serde_json::Map::new();
            repo_obj.insert("type".to_string(), serde_json::Value::String(repository.type_.clone()));
            repo_obj.insert("url".to_string(), serde_json::Value::String(repository.url.clone()));
            if let Some(ref directory) = repository.directory {
                repo_obj.insert("directory".to_string(), serde_json::Value::String(directory.clone()));
            }
            package_json.insert("repository".to_string(), serde_json::Value::Object(repo_obj));
        }
        
        if !self.scripts.is_empty() {
            package_json.insert("scripts".to_string(), 
                serde_json::Value::Object(self.scripts.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
        }
        
        if let Some(ref config) = self.config {
            package_json.insert("config".to_string(), config.clone());
        }
        
        if !self.dependencies.is_empty() {
            package_json.insert("dependencies".to_string(), 
                serde_json::Value::Object(self.dependencies.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
        }
        
        if !self.dev_dependencies.is_empty() {
            package_json.insert("devDependencies".to_string(), 
                serde_json::Value::Object(self.dev_dependencies.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
        }
        
        if !self.peer_dependencies.is_empty() {
            package_json.insert("peerDependencies".to_string(), 
                serde_json::Value::Object(self.peer_dependencies.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
        }
        
        if !self.bundled_dependencies.is_empty() {
            package_json.insert("bundledDependencies".to_string(), 
                serde_json::Value::Array(self.bundled_dependencies.iter().map(|d| serde_json::Value::String(d.clone())).collect()));
        }
        
        if !self.optional_dependencies.is_empty() {
            package_json.insert("optionalDependencies".to_string(), 
                serde_json::Value::Object(self.optional_dependencies.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
        }
        
        if !self.engines.is_empty() {
            package_json.insert("engines".to_string(), 
                serde_json::Value::Object(self.engines.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
        }
        
        if !self.os.is_empty() {
            package_json.insert("os".to_string(), 
                serde_json::Value::Array(self.os.iter().map(|o| serde_json::Value::String(o.clone())).collect()));
        }
        
        if !self.cpu.is_empty() {
            package_json.insert("cpu".to_string(), 
                serde_json::Value::Array(self.cpu.iter().map(|c| serde_json::Value::String(c.clone())).collect()));
        }
        
        if self.private {
            package_json.insert("private".to_string(), serde_json::Value::Bool(true));
        }
        
        if let Some(ref publish_config) = self.publish_config {
            let mut pub_obj = serde_json::Map::new();
            if let Some(ref registry) = publish_config.registry {
                pub_obj.insert("registry".to_string(), serde_json::Value::String(registry.clone()));
            }
            if let Some(ref access) = publish_config.access {
                pub_obj.insert("access".to_string(), serde_json::Value::String(access.clone()));
            }
            if let Some(ref tag) = publish_config.tag {
                pub_obj.insert("tag".to_string(), serde_json::Value::String(tag.clone()));
            }
            package_json.insert("publishConfig".to_string(), serde_json::Value::Object(pub_obj));
        }
        
        serde_json::Value::Object(package_json)
    }
}

fn author_to_json(author: &AuthorInfo) -> serde_json::Value {
    if author.email.is_none() && author.url.is_none() {
        serde_json::Value::String(author.name.clone())
    } else {
        let mut author_obj = serde_json::Map::new();
        author_obj.insert("name".to_string(), serde_json::Value::String(author.name.clone()));
        if let Some(ref email) = author.email {
            author_obj.insert("email".to_string(), serde_json::Value::String(email.clone()));
        }
        if let Some(ref url) = author.url {
            author_obj.insert("url".to_string(), serde_json::Value::String(url.clone()));
        }
        serde_json::Value::Object(author_obj)
    }
}

/// Metadata del repositorio npm completo
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpmRepositoryMetadata {
    pub name: String,
    pub versions: HashMap<String, NpmPackageMetadata>,
    pub time: HashMap<String, String>,
    pub users: HashMap<String, bool>,
    pub dist_tags: HashMap<String, String>,
    pub _id: String,
    pub _rev: String,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub repository: Option<RepositoryInfo>,
    pub author: Option<AuthorInfo>,
    pub license: Option<String>,
    pub readme: Option<String>,
    pub readme_filename: Option<String>,
    pub homepage: Option<String>,
    pub bugs: Option<BugsInfo>,
}

impl NpmRepositoryMetadata {
    /// Crear metadata de repositorio vacía
    pub fn new(name: String) -> Self {
        Self {
            name: name.clone(),
            versions: HashMap::new(),
            time: HashMap::new(),
            users: HashMap::new(),
            dist_tags: HashMap::new(),
            _id: name.clone(),
            _rev: "1-0".to_string(),
            description: None,
            keywords: Vec::new(),
            repository: None,
            author: None,
            license: None,
            readme: None,
            readme_filename: None,
            homepage: None,
            bugs: None,
        }
    }
    
    /// Agregar una versión
    pub fn add_version(&mut self, metadata: NpmPackageMetadata) -> Result<(), NpmPackageValidationError> {
        let version_str = metadata.version.to_string();
        
        // Verificar que el nombre coincida
        if metadata.name.full_name() != self.name {
            return Err(NpmPackageValidationError::InvalidName(
                format!("Package name mismatch: expected {}, got {}", self.name, metadata.name.full_name())
            ));
        }
        
        // Agregar la versión
        self.versions.insert(version_str.clone(), metadata);
        
        // Actualizar dist-tags si es la última versión estable
        if !self.versions.is_empty() {
            let latest_stable = self.versions.values()
                .filter(|v| v.version.is_stable())
                .max_by_key(|v| &v.version);
            
            if let Some(latest) = latest_stable {
                self.dist_tags.insert("latest".to_string(), latest.version.to_string());
            }
        }
        
        Ok(())
    }
    
    /// Obtener la versión más reciente
    pub fn get_latest_version(&self) -> Option<&NpmPackageMetadata> {
        self.dist_tags.get("latest")
            .and_then(|version| self.versions.get(version))
    }
    
    /// Obtener todas las versiones ordenadas
    pub fn get_all_versions(&self) -> Vec<&NpmPackageMetadata> {
        let mut versions: Vec<&NpmPackageMetadata> = self.versions.values().collect();
        versions.sort_by_key(|v| &v.version);
        versions
    }
    
    /// Verificar si existe una versión
    pub fn has_version(&self, version: &str) -> bool {
        self.versions.contains_key(version)
    }
    
    /// Obtener metadata de una versión específica
    pub fn get_version(&self, version: &str) -> Option<&NpmPackageMetadata> {
        self.versions.get(version)
    }
    
    /// Generar package.json del repositorio (versión simplificada)
    pub fn to_repository_package_json(&self) -> serde_json::Value {
        let mut package_json = serde_json::Map::new();
        
        package_json.insert("name".to_string(), serde_json::Value::String(self.name.clone()));
        
        if let Some(ref desc) = self.description {
            package_json.insert("description".to_string(), serde_json::Value::String(desc.clone()));
        }
        
        if !self.keywords.is_empty() {
            package_json.insert("keywords".to_string(), 
                serde_json::Value::Array(self.keywords.iter().map(|k| serde_json::Value::String(k.clone())).collect()));
        }
        
        if let Some(ref author) = self.author {
            package_json.insert("author".to_string(), author_to_json(author));
        }
        
        if let Some(ref license) = self.license {
            package_json.insert("license".to_string(), serde_json::Value::String(license.clone()));
        }
        
        if let Some(ref homepage) = self.homepage {
            package_json.insert("homepage".to_string(), serde_json::Value::String(homepage.clone()));
        }
        
        if let Some(ref bugs) = self.bugs {
            let mut bugs_obj = serde_json::Map::new();
            if let Some(ref url) = bugs.url {
                bugs_obj.insert("url".to_string(), serde_json::Value::String(url.clone()));
            }
            if let Some(ref email) = bugs.email {
                bugs_obj.insert("email".to_string(), serde_json::Value::String(email.clone()));
            }
            package_json.insert("bugs".to_string(), serde_json::Value::Object(bugs_obj));
        }
        
        if let Some(ref repo) = self.repository {
            let mut repo_obj = serde_json::Map::new();
            repo_obj.insert("type".to_string(), serde_json::Value::String(repo.type_.clone()));
            repo_obj.insert("url".to_string(), serde_json::Value::String(repo.url.clone()));
            if let Some(ref directory) = repo.directory {
                repo_obj.insert("directory".to_string(), serde_json::Value::String(directory.clone()));
            }
            package_json.insert("repository".to_string(), serde_json::Value::Object(repo_obj));
        }
        
        // Agregar todas las versiones disponibles
        if !self.versions.is_empty() {
            let mut versions_obj = serde_json::Map::new();
            for (version, metadata) in &self.versions {
                versions_obj.insert(version.clone(), metadata.to_package_json());
            }
            package_json.insert("versions".to_string(), serde_json::Value::Object(versions_obj));
        }
        
        // Agregar dist-tags
        if !self.dist_tags.is_empty() {
            package_json.insert("dist-tags".to_string(), 
                serde_json::Value::Object(self.dist_tags.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
        }
        
        // Agregar tiempo
        if !self.time.is_empty() {
            package_json.insert("time".to_string(), 
                serde_json::Value::Object(self.time.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()));
        }
        
        // Agregar usuarios
        if !self.users.is_empty() {
            package_json.insert("users".to_string(), 
                serde_json::Value::Object(self.users.iter().map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v))).collect()));
        }
        
        serde_json::Value::Object(package_json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::npm::NpmPackage;
    
    #[test]
    fn test_package_metadata_creation() {
        let name = NpmPackageName::new("test-package").unwrap();
        let package = NpmPackage::new(name.clone(), "1.0.0").unwrap();
        let metadata = NpmPackageMetadata::from_package(&package, "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string()).unwrap();
        
        assert_eq!(metadata.name.full_name(), "test-package");
        assert_eq!(metadata.version.to_string(), "1.0.0");
        assert_eq!(metadata.tarball_name(), "test-package-1.0.0.tgz");
        assert!(!metadata.is_private());
    }
    
    #[test]
    fn test_repository_metadata() {
        let mut repo_metadata = NpmRepositoryMetadata::new("test-package".to_string());
        
        let name = NpmPackageName::new("test-package").unwrap();
        let package = NpmPackage::new(name, "1.0.0").unwrap();
        let metadata = NpmPackageMetadata::from_package(&package, "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string()).unwrap();
        
        repo_metadata.add_version(metadata).unwrap();
        
        assert_eq!(repo_metadata.name, "test-package");
        assert_eq!(repo_metadata.versions.len(), 1);
        assert!(repo_metadata.has_version("1.0.0"));
        
        let latest = repo_metadata.get_latest_version().unwrap();
        assert_eq!(latest.version.to_string(), "1.0.0");
    }
    
    #[test]
    fn test_multiple_versions() {
        let mut repo_metadata = NpmRepositoryMetadata::new("test-package".to_string());
        
        // Agregar versión 1.0.0
        let name1 = NpmPackageName::new("test-package").unwrap();
        let package1 = NpmPackage::new(name1, "1.0.0").unwrap();
        let metadata1 = NpmPackageMetadata::from_package(&package1, "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string()).unwrap();
        repo_metadata.add_version(metadata1).unwrap();
        
        // Agregar versión 1.1.0
        let name2 = NpmPackageName::new("test-package").unwrap();
        let package2 = NpmPackage::new(name2, "1.1.0").unwrap();
        let metadata2 = NpmPackageMetadata::from_package(&package2, "https://registry.npmjs.org/test-package/-/test-package-1.1.0.tgz".to_string()).unwrap();
        repo_metadata.add_version(metadata2).unwrap();
        
        assert_eq!(repo_metadata.versions.len(), 2);
        assert!(repo_metadata.has_version("1.0.0"));
        assert!(repo_metadata.has_version("1.1.0"));
        
        let latest = repo_metadata.get_latest_version().unwrap();
        assert_eq!(latest.version.to_string(), "1.1.0");
    }
    
    #[test]
    fn test_to_package_json() {
        let name = NpmPackageName::new("test-package").unwrap();
        let package = NpmPackage::new(name, "1.0.0").unwrap();
        let metadata = NpmPackageMetadata::from_package(&package, "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string()).unwrap();
        
        let json = metadata.to_package_json();
        
        assert_eq!(json["name"], "test-package");
        assert_eq!(json["version"], "1.0.0");
        assert!(json.get("dependencies").is_some());
    }
    
    #[test]
    fn test_to_repository_package_json() {
        let mut repo_metadata = NpmRepositoryMetadata::new("test-package".to_string());
        repo_metadata.description = Some("A test package".to_string());
        repo_metadata.keywords = vec!["test".to_string(), "npm".to_string()];
        
        let name = NpmPackageName::new("test-package").unwrap();
        let package = NpmPackage::new(name, "1.0.0").unwrap();
        let metadata = NpmPackageMetadata::from_package(&package, "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string()).unwrap();
        repo_metadata.add_version(metadata).unwrap();
        
        let json = repo_metadata.to_repository_package_json();
        
        assert_eq!(json["name"], "test-package");
        assert_eq!(json["description"], "A test package");
        assert!(json.get("keywords").is_some());
        assert!(json.get("versions").is_some());
        assert!(json.get("dist-tags").is_some());
    }
}