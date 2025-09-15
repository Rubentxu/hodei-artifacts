//! Cedar Policy adapter for PackageVersion resource

use cedar_policy::{EntityId, EntityTypeName, EntityUid, RestrictedExpression};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::package_version::PackageVersion;
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
