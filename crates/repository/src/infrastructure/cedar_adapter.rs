//! Cedar Policy adapters for Repository resources

use cedar_policy::{EntityUid, EntityTypeName, EntityId, RestrictedExpression};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::repository::Repository;
use crate::domain::storage::StorageBackend;
use shared::security::HodeiResource;

/// Cedar Policy adapter implementation for Repository
impl HodeiResource<EntityUid, RestrictedExpression> for Repository {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:repository:<region>:<org_id>:repository/<repo_name>
        let parts: Vec<&str> = self.hrn.0.as_str().split(':').collect();
        if parts.len() >= 6 {
            let entity_type = EntityTypeName::from_str("Repository").unwrap();
            let entity_id = EntityId::from_str(parts[5]).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        } else {
            // Fallback: use entire HRN as ID
            let entity_type = EntityTypeName::from_str("Repository").unwrap();
            let entity_id = EntityId::from_str(self.hrn.0.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        }
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("repository".to_string()));
        attrs.insert("repo_type".to_string(), RestrictedExpression::new_string(format!("{:?}", self.repo_type)));
        attrs.insert("format".to_string(), RestrictedExpression::new_string(format!("{:?}", self.format)));
        attrs.insert("region".to_string(), RestrictedExpression::new_string(self.region.clone()));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de un repositorio es su organización.
        vec![EntityUid::from_str(self.organization_hrn.0.as_str()).unwrap()]
    }
}

// TODO: Add StorageBackend implementation when available