//! Cedar Policy adapters for Supply Chain resources

use cedar_policy::{EntityUid, EntityTypeName, EntityId, RestrictedExpression};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::attestation::Attestation;
use shared::security::HodeiResource;

/// Cedar Policy adapter implementation for Attestation
impl HodeiResource<EntityUid, RestrictedExpression> for Attestation {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:supply-chain:<region>:<org_id>:attestation/<attestation_id>
        let parts: Vec<&str> = self.hrn.as_str().split(':').collect();
        if parts.len() >= 6 {
            let entity_type = EntityTypeName::from_str("Attestation").unwrap();
            let entity_id = EntityId::from_str(parts[5]).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        } else {
            // Fallback: use entire HRN as ID
            let entity_type = EntityTypeName::from_str("Attestation").unwrap();
            let entity_id = EntityId::from_str(self.hrn.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        }
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("attestation".to_string()));
        attrs.insert("predicate_type".to_string(), RestrictedExpression::new_string(format!("{:?}", self.predicate_type)));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de una atestación es su organización.
        vec![EntityUid::from_str(self.organization_hrn.0.as_str()).unwrap()]
    }
}