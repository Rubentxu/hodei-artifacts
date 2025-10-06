use kernel::Hrn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use kernel::{
    AttributeName, AttributeType, AttributeValue, HodeiEntity, HodeiEntityType, Resource,
    ResourceTypeName, ServiceName,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Option<Hrn>,
    pub attached_scps: HashSet<Hrn>,
}

impl Account {
    pub fn new(hrn: Hrn, name: String, parent_hrn: Option<Hrn>) -> Self {
        Self {
            hrn,
            name,
            parent_hrn,
            attached_scps: HashSet::new(),
        }
    }

    pub fn set_parent(&mut self, parent_hrn: Hrn) {
        self.parent_hrn = Some(parent_hrn);
    }

    pub fn attach_scp(&mut self, scp_hrn: Hrn) {
        self.attached_scps.insert(scp_hrn);
    }

    pub fn detach_scp(&mut self, scp_hrn: &Hrn) -> bool {
        self.attached_scps.remove(scp_hrn)
    }

    pub fn has_scp(&self, scp_hrn: &Hrn) -> bool {
        self.attached_scps.contains(scp_hrn)
    }
}

// ============================================================================
// Kernel Agnostic Integration
// ============================================================================

/// Implementation of HodeiEntityType for Account
/// Provides type-level metadata for schema generation
impl HodeiEntityType for Account {
    fn service_name() -> ServiceName {
        ServiceName::new("organizations").expect("Valid service name")
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("Account").expect("Valid resource type name")
    }

    fn is_principal_type() -> bool {
        false // Account is a Resource, not a Principal
    }

    fn is_resource_type() -> bool {
        true
    }

    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        vec![
            (
                AttributeName::new("name").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("type").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("attached_scps").expect("Valid attribute name"),
                AttributeType::set(AttributeType::string()),
            ),
        ]
    }

    fn parent_types() -> Vec<String> {
        vec!["Organizations::OrganizationalUnit".to_string()]
    }
}

/// Implementation of HodeiEntity for Account
/// Provides instance-level data for authorization
impl HodeiEntity for Account {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();

        attrs.insert(
            AttributeName::new("name").expect("Valid attribute name"),
            AttributeValue::String(self.name.clone()),
        );

        attrs.insert(
            AttributeName::new("type").expect("Valid attribute name"),
            AttributeValue::String("account".to_string()),
        );

        let scps_vec: Vec<AttributeValue> = self
            .attached_scps
            .iter()
            .map(|scp_hrn| AttributeValue::String(scp_hrn.to_string()))
            .collect();
        attrs.insert(
            AttributeName::new("attached_scps").expect("Valid attribute name"),
            AttributeValue::Set(scps_vec),
        );

        attrs
    }

    fn parent_hrns(&self) -> Vec<Hrn> {
        if let Some(parent) = &self.parent_hrn {
            vec![parent.clone()]
        } else {
            Vec::new()
        }
    }
}

/// Marker trait implementation: Account is a Resource
impl Resource for Account {}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hrn() -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "Account".to_string(),
            "acc-123".to_string(),
        )
    }

    fn sample_parent_hrn() -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "OrganizationalUnit".to_string(),
            "ou-456".to_string(),
        )
    }

    #[test]
    fn account_entity_type_metadata() {
        let service = Account::service_name();
        assert_eq!(service.as_str(), "organizations");

        let resource_type = Account::resource_type_name();
        assert_eq!(resource_type.as_str(), "Account");

        assert!(!Account::is_principal_type());
        assert!(Account::is_resource_type());
    }

    #[test]
    fn account_entity_type_name_generation() {
        let entity_type_name = Account::entity_type_name();
        assert_eq!(entity_type_name, "Organizations::Account");
    }

    #[test]
    fn account_attributes_schema_contains_expected_fields() {
        let schema = Account::attributes_schema();
        assert_eq!(schema.len(), 3);

        let names: Vec<String> = schema
            .iter()
            .map(|(name, _)| name.as_str().to_string())
            .collect();
        assert!(names.contains(&"name".to_string()));
        assert!(names.contains(&"type".to_string()));
        assert!(names.contains(&"attached_scps".to_string()));
    }

    #[test]
    fn account_parent_types_includes_ou() {
        let parent_types = Account::parent_types();
        assert_eq!(parent_types.len(), 1);
        assert_eq!(parent_types[0], "Organizations::OrganizationalUnit");
    }

    #[test]
    fn account_creation_and_hrn() {
        let hrn = sample_hrn();
        let account = Account::new(hrn.clone(), "Production".to_string(), None);

        assert_eq!(account.hrn(), &hrn);
        assert_eq!(account.name, "Production");
        assert!(account.parent_hrn.is_none());
        assert!(account.attached_scps.is_empty());
    }

    #[test]
    fn account_set_parent() {
        let mut account = Account::new(sample_hrn(), "Production".to_string(), None);
        let parent = sample_parent_hrn();

        account.set_parent(parent.clone());
        assert_eq!(account.parent_hrn, Some(parent));
    }

    #[test]
    fn account_attach_and_detach_scp() {
        let mut account = Account::new(sample_hrn(), "Production".to_string(), None);
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "ServiceControlPolicy".to_string(),
            "scp-789".to_string(),
        );

        // Attach SCP
        account.attach_scp(scp_hrn.clone());
        assert!(account.has_scp(&scp_hrn));
        assert_eq!(account.attached_scps.len(), 1);

        // Detach SCP
        let removed = account.detach_scp(&scp_hrn);
        assert!(removed);
        assert!(!account.has_scp(&scp_hrn));
        assert!(account.attached_scps.is_empty());
    }

    #[test]
    fn account_attributes_contains_expected_keys() {
        let account = Account::new(sample_hrn(), "Production".to_string(), None);
        let attrs = account.attributes();
        assert_eq!(attrs.len(), 3);

        let name_attr = AttributeName::new("name").unwrap();
        let type_attr = AttributeName::new("type").unwrap();
        let scps_attr = AttributeName::new("attached_scps").unwrap();

        assert!(attrs.contains_key(&name_attr));
        assert!(attrs.contains_key(&type_attr));
        assert!(attrs.contains_key(&scps_attr));

        // Verify values
        assert_eq!(
            attrs.get(&name_attr),
            Some(&AttributeValue::String("Production".to_string()))
        );
        assert_eq!(
            attrs.get(&type_attr),
            Some(&AttributeValue::String("account".to_string()))
        );
    }

    #[test]
    fn account_parent_hrns_empty_when_no_parent() {
        let account = Account::new(sample_hrn(), "Production".to_string(), None);
        assert!(account.parent_hrns().is_empty());
    }

    #[test]
    fn account_parent_hrns_returns_parent_when_set() {
        let parent = sample_parent_hrn();
        let account = Account::new(sample_hrn(), "Production".to_string(), Some(parent.clone()));

        let parent_hrns = account.parent_hrns();
        assert_eq!(parent_hrns.len(), 1);
        assert_eq!(parent_hrns[0], parent);
    }
}
