use super::dto::{NpmDependency, ParsedNpmMetadata};
use super::error::MetadataError;
use serde_json;
use std::collections::HashMap;

/// Structure representing a package.json file
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NpmPackage {
    pub name: String,
    pub version: String,
    pub description: Option<String>,

    #[serde(rename = "license")]
    pub license_single: Option<String>,

    #[serde(rename = "licenses")]
    pub licenses_multiple: Option<Vec<NpmLicense>>,

    pub dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    pub dev_dependencies: Option<HashMap<String, String>>,
}

/// License structure in package.json
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NpmLicense {
    pub type_: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
}

/// Parser for NPM package.json files
pub struct NpmParser;

impl NpmParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse NPM package.json content and extract metadata
    pub fn parse(&self, json_content: &str) -> Result<ParsedNpmMetadata, MetadataError> {
        // Parse JSON content
        let package: NpmPackage = serde_json::from_str(json_content).map_err(|e| {
            MetadataError::ParseError(format!("Failed to parse package.json: {}", e))
        })?;

        // Extract licenses
        let licenses = self.extract_licenses(&package);

        // Extract dependencies
        let mut dependencies = Vec::new();

        // Regular dependencies
        if let Some(deps) = &package.dependencies {
            for (name, version) in deps {
                dependencies.push(NpmDependency {
                    name: name.clone(),
                    version: version.clone(),
                    is_dev_dependency: false,
                });
            }
        }

        // Dev dependencies
        if let Some(dev_deps) = &package.dev_dependencies {
            for (name, version) in dev_deps {
                dependencies.push(NpmDependency {
                    name: name.clone(),
                    version: version.clone(),
                    is_dev_dependency: true,
                });
            }
        }

        Ok(ParsedNpmMetadata {
            name: package.name,
            version: package.version,
            description: package.description,
            licenses,
            dependencies,
        })
    }

    /// Extract license information from package.json
    fn extract_licenses(&self, package: &NpmPackage) -> Vec<String> {
        let mut licenses = Vec::new();

        // Single license field
        if let Some(single_license) = &package.license_single {
            licenses.push(single_license.clone());
        }

        // Multiple licenses field
        if let Some(multiple_licenses) = &package.licenses_multiple {
            for license in multiple_licenses {
                if let Some(name) = &license.name {
                    licenses.push(name.clone());
                } else if let Some(type_) = &license.type_ {
                    licenses.push(type_.clone());
                }
            }
        }

        licenses
    }
}
