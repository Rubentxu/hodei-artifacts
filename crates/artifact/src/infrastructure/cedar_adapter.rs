//! Cedar Policy adapter for PackageVersion resource

use cedar_policy::{EntityId, EntityTypeName, EntityUid, RestrictedExpression};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::package_version::PackageVersion;
use crate::domain::physical_artifact::PhysicalArtifact;
use crate::domain::artifact::Artifact;
use crate::domain::sbom::Sbom;
use shared::security::HodeiResource;

/// Cedar Policy adapter implementation for PackageVersion
impl HodeiResource<EntityUid, RestrictedExpression> for PackageVersion {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:artifact:<region>:<org_id>:package-version/<repo_name>/<path>
        let parts: Vec<&str> = self.hrn.as_str().split(':').collect();
        if parts.len() >= 6 {
            let entity_type = EntityTypeName::from_str("PackageVersion").unwrap();
            let entity_id = EntityId::from_str(parts[5]).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        } else {
            // Fallback: use entire HRN as ID
            let entity_type = EntityTypeName::from_str("PackageVersion").unwrap();
            let entity_id = EntityId::from_str(self.hrn.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        }

/// Cedar Policy adapter implementation for logical Artifact
impl HodeiResource<EntityUid, RestrictedExpression> for Artifact {
    fn resource_id(&self) -> EntityUid {
        // Prefer parsing full HRN; fallback to type+id
        EntityUid::from_str(self.id.0.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("Artifact").unwrap();
            let entity_id = EntityId::from_str(self.id.0.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("artifact".to_string()));
        // Coordinates as record-like: name, version, qualifier
        attrs.insert(
            "coordinates.name".to_string(),
            RestrictedExpression::new_string(self.coordinates.name.clone()),
        );
        attrs.insert(
            "coordinates.version".to_string(),
            RestrictedExpression::new_string(self.coordinates.version.clone()),
        );
        attrs.insert(
            "coordinates.qualifier".to_string(),
            RestrictedExpression::new_string(self.coordinates.qualifier.clone()),
        );
        attrs.insert(
            "artifact_type".to_string(),
            RestrictedExpression::new_string(format!("{:?}", self.artifact_type)),
        );
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // Parent: repository; optionally could include uploader as another parent, but keep minimal
        vec![create_entity_uid_from_hrn(self.repository_id.0.as_str())]
    }
}

    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "type".to_string(),
            RestrictedExpression::new_string("package_version".to_string()),
        );
        attrs.insert(
            "status".to_string(),
            RestrictedExpression::new_string(format!("{:?}", self.status)),
        );

        // Convert tags to a set of strings
        let tags_set = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect::<Vec<_>>();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tags_set));

        // Add license information if available
        if !self.metadata.licenses.is_empty() {
            let licenses_set = self
                .metadata
                .licenses
                .iter()
                .map(|l| RestrictedExpression::new_string(l.clone()))
                .collect::<Vec<_>>();
            attrs.insert(
                "licenses".to_string(),
                RestrictedExpression::new_set(licenses_set),
            );
        }

        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        vec![
            create_entity_uid_from_hrn(self.repository_hrn.0.as_str()),
            create_entity_uid_from_hrn(self.organization_hrn.0.as_str()),
        ]
    }
}


/// Cedar Policy adapter implementation for Sbom
impl HodeiResource<EntityUid, RestrictedExpression> for Sbom {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:artifact:<region>:<org_id>:sbom/<hash_algorithm>-<hash_value>
        EntityUid::from_str(self.hrn.0.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("Sbom").unwrap();
            let entity_id = EntityId::from_str(
                self.hrn.0.split(':').last().unwrap_or_default()
            ).unwrap_or_else(|_| EntityId::new("unknown").unwrap());
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        
        // Basic type and format information
        attrs.insert("type".to_string(), RestrictedExpression::new_string("sbom".to_string()));
        attrs.insert("format".to_string(), RestrictedExpression::new_string(format!("{:?}", self.format)));
        attrs.insert("format_version".to_string(), RestrictedExpression::new_string(self.format_version.clone()));
        
        // Add signatures count and algorithms
        let signature_algorithms = self.signatures
            .iter()
            .map(|s| RestrictedExpression::new_string(s.algorithm.clone()))
            .collect::<Vec<_>>();
            
        attrs.insert("signature_count".to_string(), RestrictedExpression::new_long(self.signatures.len() as i64));
        attrs.insert("signature_algorithms".to_string(), RestrictedExpression::new_set(signature_algorithms));
        
        // Add metadata as individual attributes
        for (key, value) in &self.metadata {
            attrs.insert(
                format!("metadata.{}", key.replace('.', "_")), 
                RestrictedExpression::new_string(value.clone())
            );
        }
        
        // Add timestamps
        attrs.insert(
            "generated_at".to_string(), 
            RestrictedExpression::new_string(self.generated_at.to_string())
        );
        
        if let Some(generated_by) = &self.generated_by {
            attrs.insert("generated_by".to_string(), RestrictedExpression::new_string(generated_by.clone()));
        }
        
        if let Some(generated_using) = &self.generated_using {
            attrs.insert("generated_using".to_string(), RestrictedExpression::new_string(generated_using.clone()));
        }
        
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // The parent of an SBOM is the physical artifact it describes
        vec![create_entity_uid_from_hrn(self.physical_artifact_id.0.as_str())]
    }
}


/// Cedar Policy adapter implementation for PhysicalArtifact
impl HodeiResource<EntityUid, RestrictedExpression> for PhysicalArtifact {
    fn resource_id(&self) -> EntityUid {
        // Build an EntityUid from the HRN directly if possible
        EntityUid::from_str(self.hrn.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("PhysicalArtifact").unwrap();
            let entity_id = EntityId::from_str(self.hrn.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "type".to_string(),
            RestrictedExpression::new_string("physical_artifact".to_string()),
        );
        attrs.insert(
            "mime_type".to_string(),
            RestrictedExpression::new_string(self.mime_type.clone()),
        );
        attrs.insert(
            "size_in_bytes".to_string(),
            RestrictedExpression::new_string(self.size_in_bytes.to_string()),
        );
        attrs.insert(
            "hash_algorithm".to_string(),
            RestrictedExpression::new_string(format!("{:?}", self.content_hash.algorithm)),
        );
        attrs.insert(
            "hash_value".to_string(),
            RestrictedExpression::new_string(self.content_hash.value.clone()),
        );
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // Parent: the owning Organization
        vec![create_entity_uid_from_hrn(self.organization_hrn.0.as_str())]
    }
}


/// Helper function to create EntityUid from HRN string
fn create_entity_uid_from_hrn(hrn: &str) -> EntityUid {
    let parts: Vec<&str> = hrn.split(':').collect();
    if parts.len() >= 6 {
        let entity_type = match parts[2] {
            "artifact" => "Artifact",
            "iam" => "IAM",
            "repository" => "Repository",
            "organization" => "Organization",
            _ => "Resource",
        };
        let entity_type = EntityTypeName::from_str(entity_type).unwrap();
        let entity_id = EntityId::from_str(parts[5]).unwrap();
        EntityUid::from_type_name_and_id(entity_type, entity_id)
    } else {
        // Fallback: use entire HRN as ID
        let entity_type = EntityTypeName::from_str("Resource").unwrap();
        let entity_id = EntityId::from_str(hrn).unwrap();
        EntityUid::from_type_name_and_id(entity_type, entity_id)
    }
}

