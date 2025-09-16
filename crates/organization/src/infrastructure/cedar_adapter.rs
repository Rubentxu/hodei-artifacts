//! Cedar Policy adapter for Organization resource

use cedar_policy::{EntityUid, RestrictedExpression, EntityTypeName, EntityId};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::organization::Organization;
use crate::domain::member::Member;
use crate::domain::team::Team;
use crate::domain::organization_settings::OrganizationSettings;
use shared::security::HodeiResource;

/// Cedar Policy adapter implementation for Organization
impl HodeiResource<EntityUid, RestrictedExpression> for Organization {
    fn resource_id(&self) -> EntityUid {
        EntityUid::from_str(self.hrn.as_str()).unwrap()
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "type".to_string(),
            RestrictedExpression::new_string("organization".to_string()),
        );
        attrs.insert(
            "status".to_string(),
            RestrictedExpression::new_string(format!("{:?}", self.status)),
        );
        attrs.insert(
            "primary_region".to_string(),
            RestrictedExpression::new_string(self.primary_region.clone()),
        );
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // Las organizaciones son la raíz de la jerarquía, no tienen padres.
        vec![]
    }
}

/// Cedar Policy adapter implementation for Member
impl HodeiResource<EntityUid, RestrictedExpression> for Member {
    fn resource_id(&self) -> EntityUid {
        // Use entire HRN as ID if parsing fails
        EntityUid::from_str(self.hrn.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("Member").unwrap();
            let entity_id = EntityId::from_str(self.hrn.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "type".to_string(),
            RestrictedExpression::new_string("member".to_string()),
        );
        // roles as set of strings
        let roles = self
            .roles
            .iter()
            .map(|r| RestrictedExpression::new_string(format!("{:?}", r)))
            .collect::<Vec<_>>();
        attrs.insert("roles".to_string(), RestrictedExpression::new_set(roles));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // Parents: organization and user
        vec![
            EntityUid::from_str(self.organization_hrn.0.as_str()).unwrap(),
            EntityUid::from_str(self.user_hrn.0.as_str()).unwrap(),
        ]
    }
}

/// Cedar Policy adapter implementation for Team
impl HodeiResource<EntityUid, RestrictedExpression> for Team {
    fn resource_id(&self) -> EntityUid {
        EntityUid::from_str(self.id.0.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("Team").unwrap();
            let entity_id = EntityId::from_str(self.id.0.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("team".to_string()));
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // Parent: the owning Organization
        vec![EntityUid::from_str(self.organization.0.as_str()).unwrap()]
    }
}

/// Cedar Policy adapter implementation for OrganizationSettings
impl HodeiResource<EntityUid, RestrictedExpression> for OrganizationSettings {
    fn resource_id(&self) -> EntityUid {
        EntityUid::from_str(self.id.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("OrganizationSettings").unwrap();
            let entity_id = EntityId::from_str(self.id.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "type".to_string(),
            RestrictedExpression::new_string("organization_settings".to_string()),
        );
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        vec![EntityUid::from_str(self.organization.0.as_str()).unwrap()]
    }
}
