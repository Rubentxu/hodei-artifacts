//! Cedar Policy adapters for IAM resources

use cedar_policy::{EntityUid, EntityTypeName, EntityId, RestrictedExpression};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::user::User;
use crate::domain::group::Group;
use crate::domain::service_account::ServiceAccount;
use crate::domain::api_key::ApiKey;
use shared::security::HodeiResource;
use shared::hrn::{Hrn, OrganizationId, UserId};

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

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::{Hrn, OrganizationId, UserId};
    use time::OffsetDateTime;
    use shared::lifecycle::Lifecycle;
    use crate::domain::api_key::ApiKey;

    fn org() -> OrganizationId {
        OrganizationId(Hrn::new("hrn:hodei:iam:global:acme:organization").unwrap())
    }

    fn user() -> User {
        User {
            hrn: Hrn::new("hrn:hodei:iam:global:acme:user/alice").unwrap(),
            email: "alice@acme.io".into(),
            status: crate::domain::user::UserStatus::Active,
            profile: crate::domain::user::UserProfile::default(),
            organization_memberships: vec![org()],
            group_memberships: vec![],
            external_id: None,
            lifecycle: Lifecycle::new(Hrn::new("hrn:hodei:iam:global:acme:user/alice").unwrap()),
        }
    }
    
    fn api_key() -> ApiKey {
        let owner_hrn = Hrn::new("hrn:hodei:iam:global:acme:user/alice").unwrap();
        ApiKey {
            id: "test-key-123".to_string(),
            hrn: Hrn::new("hrn:hodei:iam:global:acme:apikey/test-key-123").unwrap(),
            owner_hrn: owner_hrn.clone(),
            hashed_token: "$2b$12$...".to_string(),
            description: Some("Test API Key".to_string()),
            expires_at: Some(OffsetDateTime::now_utc() + time::Duration::days(30)),
            lifecycle: Lifecycle::new(owner_hrn),
        }
    }

    fn group() -> Group {
        Group {
            hrn: Hrn::new("hrn:hodei:iam:global:acme:group/devs").unwrap(),
            organization_hrn: org(),
            name: "devs".into(),
            description: Some("Developers".into()),
            lifecycle: Lifecycle::new(Hrn::new("hrn:hodei:iam:global:acme:user/admin").unwrap()),
        }
    }

    fn service_account() -> ServiceAccount {
        ServiceAccount {
            hrn: Hrn::new("hrn:hodei:iam:global:acme:service-account/ci").unwrap(),
            organization_hrn: org(),
            name: "ci".into(),
            description: Some("CI pipeline".into()),
            status: crate::domain::service_account::ServiceAccountStatus::Active,
            lifecycle: Lifecycle::new(Hrn::new("hrn:hodei:iam:global:acme:user/admin").unwrap()),
        }
    }

    #[test]
    fn user_adapter_smoke() {
        let u = user();
        let id = <User as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&u);
        let attrs = <User as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&u);
        let parents = <User as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&u);
        assert!(!id.to_string().is_empty());
        assert!(attrs.contains_key("type"));
        assert!(parents.is_empty());
    }

    #[test]
    fn group_adapter_smoke() {
        let g = group();
        let id = <Group as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&g);
        let attrs = <Group as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&g);
        let parents = <Group as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&g);
        assert!(!id.to_string().is_empty());
        assert!(attrs.contains_key("type"));
        assert_eq!(parents.len(), 1);
    }

    #[test]
    fn service_account_adapter_smoke() {
        let sa = service_account();
        let id = <ServiceAccount as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&sa);
        let attrs = <ServiceAccount as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&sa);
        let parents = <ServiceAccount as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&sa);
        assert!(!id.to_string().is_empty());
        assert!(attrs.contains_key("type"));
        assert_eq!(parents.len(), 1);
    }
    
    #[test]
    fn api_key_adapter_smoke() {
        let key = api_key();
        let id = <ApiKey as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&key);
        let attrs = <ApiKey as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&key);
        let parents = <ApiKey as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&key);
        
        assert!(!id.to_string().is_empty());
        assert!(attrs.contains_key("type"));
        assert!(attrs.contains_key("owner_hrn"));
        assert!(attrs.contains_key("description"));
        assert_eq!(parents.len(), 1);
    }
}

/// Cedar Policy adapter implementation for ApiKey
impl HodeiResource<EntityUid, RestrictedExpression> for ApiKey {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:iam:global:<org_id>:apikey/<key_id>
        EntityUid::from_str(self.hrn.0.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("ApiKey").unwrap();
            let entity_id = EntityId::from_str(&self.id).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("api-key".to_string()));
        attrs.insert("owner_hrn".to_string(), RestrictedExpression::new_string(self.owner_hrn.0.to_string()));
        
        if let Some(expires_at) = self.expires_at {
            attrs.insert("expires_at".to_string(), RestrictedExpression::new_string(expires_at.to_string()));
        }
        
        if let Some(description) = &self.description {
            attrs.insert("description".to_string(), RestrictedExpression::new_string(description.clone()));
        }
        
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de una ApiKey es su propietario (User o ServiceAccount)
        vec![EntityUid::from_str(self.owner_hrn.0.as_str()).unwrap()]
    }
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

/// Cedar Policy adapter implementation for ApiKey
impl HodeiResource<EntityUid, RestrictedExpression> for ApiKey {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:iam:global:<org_id>:apikey/<key_id>
        EntityUid::from_str(self.hrn.0.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("ApiKey").unwrap();
            let entity_id = EntityId::from_str(&self.id).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("api-key".to_string()));
        attrs.insert("owner_hrn".to_string(), RestrictedExpression::new_string(self.owner_hrn.0.to_string()));
        
        if let Some(expires_at) = self.expires_at {
            attrs.insert("expires_at".to_string(), RestrictedExpression::new_string(expires_at.to_string()));
        }
        
        if let Some(description) = &self.description {
            attrs.insert("description".to_string(), RestrictedExpression::new_string(description.clone()));
        }
        
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de una ApiKey es su propietario (User o ServiceAccount)
        vec![EntityUid::from_str(self.owner_hrn.0.as_str()).unwrap()]
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

/// Cedar Policy adapter implementation for ApiKey
impl HodeiResource<EntityUid, RestrictedExpression> for ApiKey {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:iam:global:<org_id>:apikey/<key_id>
        EntityUid::from_str(self.hrn.0.as_str()).unwrap_or_else(|_| {
            let entity_type = EntityTypeName::from_str("ApiKey").unwrap();
            let entity_id = EntityId::from_str(&self.id).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        })
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("api-key".to_string()));
        attrs.insert("owner_hrn".to_string(), RestrictedExpression::new_string(self.owner_hrn.0.to_string()));
        
        if let Some(expires_at) = self.expires_at {
            attrs.insert("expires_at".to_string(), RestrictedExpression::new_string(expires_at.to_string()));
        }
        
        if let Some(description) = &self.description {
            attrs.insert("description".to_string(), RestrictedExpression::new_string(description.clone()));
        }
        
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de una ApiKey es su propietario (User o ServiceAccount)
        vec![EntityUid::from_str(self.owner_hrn.0.as_str()).unwrap()]
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