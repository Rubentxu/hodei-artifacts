//! Cedar Policy adapters for IAM resources

use cedar_policy::{EntityUid, EntityTypeName, EntityId, RestrictedExpression};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::user::User;
use crate::domain::group::Group;
use crate::domain::service_account::ServiceAccount;
use shared::security::HodeiResource;

/// Cedar Policy adapter implementation for User
impl HodeiResource<EntityUid, RestrictedExpression> for User {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:iam:global:<org_id>:user/<user_id>
        let parts: Vec<&str> = self.hrn.0.as_str().split(':').collect();
        if parts.len() >= 6 {
            let entity_type = EntityTypeName::from_str("User").unwrap();
            let entity_id = EntityId::from_str(parts[5]).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        } else {
            // Fallback: use entire HRN as ID
            let entity_type = EntityTypeName::from_str("User").unwrap();
            let entity_id = EntityId::from_str(self.hrn.0.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        }
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("user".to_string()));
        attrs.insert("status".to_string(), RestrictedExpression::new_string(format!("{:?}", self.status)));
        attrs.insert("email".to_string(), RestrictedExpression::new_string(self.email.clone()));
        // Nota: Para atributos con valores complejos como membresías de grupos,
        // se necesitaría una estrategia de mapeo diferente usando sets o records
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // La relación de un usuario con una organización se modela mejor como un atributo
        // (`memberOfOrgs`), ya que un usuario puede pertenecer a varias.
        // Por lo tanto, un usuario no tiene un padre jerárquico directo.
        vec![]
    }
}

/// Cedar Policy adapter implementation for Group
impl HodeiResource<EntityUid, RestrictedExpression> for Group {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:iam:global:<org_id>:group/<group_name>
        let parts: Vec<&str> = self.hrn.0.as_str().split(':').collect();
        if parts.len() >= 6 {
            let entity_type = EntityTypeName::from_str("Group").unwrap();
            let entity_id = EntityId::from_str(parts[5]).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        } else {
            // Fallback: use entire HRN as ID
            let entity_type = EntityTypeName::from_str("Group").unwrap();
            let entity_id = EntityId::from_str(self.hrn.0.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        }
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("group".to_string()));
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de un grupo es su organización.
        vec![EntityUid::from_str(self.organization_hrn.0.as_str()).unwrap()]
    }
}

/// Cedar Policy adapter implementation for ServiceAccount
impl HodeiResource<EntityUid, RestrictedExpression> for ServiceAccount {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:iam:global:<org_id>:service-account/<sa_name>
        let parts: Vec<&str> = self.hrn.0.as_str().split(':').collect();
        if parts.len() >= 6 {
            let entity_type = EntityTypeName::from_str("ServiceAccount").unwrap();
            let entity_id = EntityId::from_str(parts[5]).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        } else {
            // Fallback: use entire HRN as ID
            let entity_type = EntityTypeName::from_str("ServiceAccount").unwrap();
            let entity_id = EntityId::from_str(self.hrn.0.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        }
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("service-account".to_string()));
        attrs.insert("status".to_string(), RestrictedExpression::new_string(format!("{:?}", self.status)));
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de una cuenta de servicio es su organización.
        vec![EntityUid::from_str(self.organization_hrn.0.as_str()).unwrap()]
    }
}