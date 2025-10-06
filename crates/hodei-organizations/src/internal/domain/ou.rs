use kernel::Hrn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use kernel::{
    AttributeName, AttributeType, AttributeValue, HodeiEntity, HodeiEntityType, Resource,
    ResourceTypeName, ServiceName,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalUnit {
    pub hrn: Hrn,
    pub name: String,
    pub parent_hrn: Hrn,
    pub child_ous: HashSet<Hrn>,
    pub child_accounts: HashSet<Hrn>,
    pub attached_scps: HashSet<Hrn>,
}

impl OrganizationalUnit {
    pub fn new(name: String, parent_hrn: Hrn) -> Self {
        let hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            name.clone(),
        );
        Self {
            hrn,
            name,
            parent_hrn,
            child_ous: HashSet::new(),
            child_accounts: HashSet::new(),
            attached_scps: HashSet::new(),
        }
    }

    pub fn add_child_ou(&mut self, child_hrn: Hrn) {
        self.child_ous.insert(child_hrn);
    }

    pub fn remove_child_ou(&mut self, child_hrn: &Hrn) {
        self.child_ous.remove(child_hrn);
    }

    pub fn add_child_account(&mut self, account_hrn: Hrn) {
        self.child_accounts.insert(account_hrn);
    }

    pub fn remove_child_account(&mut self, account_hrn: &Hrn) {
        self.child_accounts.remove(account_hrn);
    }

    pub fn attach_scp(&mut self, scp_hrn: Hrn) {
        self.attached_scps.insert(scp_hrn);
    }

    pub fn detach_scp(&mut self, scp_hrn: &Hrn) {
        self.attached_scps.remove(scp_hrn);
    }
}

// ============================================================================
// Kernel Agnostic Integration
// ============================================================================

/// Implementation of HodeiEntityType for OrganizationalUnit
/// Provides type-level metadata for schema generation
impl HodeiEntityType for OrganizationalUnit {
    fn service_name() -> ServiceName {
        ServiceName::new("organizations").expect("Valid service name")
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("OrganizationalUnit").expect("Valid resource type name")
    }

    fn is_principal_type() -> bool {
        false // OrganizationalUnit is a Resource, not a Principal
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
                AttributeName::new("child_count").expect("Valid attribute name"),
                AttributeType::long(),
            ),
            (
                AttributeName::new("attached_scps").expect("Valid attribute name"),
                AttributeType::set(AttributeType::string()),
            ),
        ]
    }

    fn parent_types() -> Vec<String> {
        vec!["Organizations::OrganizationalUnit".to_string()] // Can have parent OU
    }
}

/// Implementation of HodeiEntity for OrganizationalUnit
/// Provides instance-level data for authorization
impl HodeiEntity for OrganizationalUnit {
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
            AttributeValue::String("organizational_unit".to_string()),
        );

        let child_count = self.child_ous.len() + self.child_accounts.len();
        attrs.insert(
            AttributeName::new("child_count").expect("Valid attribute name"),
            AttributeValue::Long(child_count as i64),
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
        vec![self.parent_hrn.clone()]
    }
}

/// Marker trait implementation: OrganizationalUnit is a Resource
impl Resource for OrganizationalUnit {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_parent_hrn() -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "root".to_string(),
            "root-1".to_string(),
        )
    }

    #[test]
    fn ou_entity_type_metadata() {
        let service = OrganizationalUnit::service_name();
        assert_eq!(service.as_str(), "organizations");

        let resource_type = OrganizationalUnit::resource_type_name();
        assert_eq!(resource_type.as_str(), "OrganizationalUnit");

        assert!(!OrganizationalUnit::is_principal_type());
        assert!(OrganizationalUnit::is_resource_type());
    }

    #[test]
    fn ou_entity_type_name_generation() {
        let entity_type_name = OrganizationalUnit::entity_type_name();
        assert_eq!(entity_type_name, "Organizations::OrganizationalUnit");
    }

    #[test]
    fn ou_attributes_schema_contains_expected_fields() {
        let schema = OrganizationalUnit::attributes_schema();
        assert_eq!(schema.len(), 4);

        let names: Vec<String> = schema
            .iter()
            .map(|(name, _)| name.as_str().to_string())
            .collect();
        assert!(names.contains(&"name".to_string()));
        assert!(names.contains(&"type".to_string()));
        assert!(names.contains(&"child_count".to_string()));
        assert!(names.contains(&"attached_scps".to_string()));
    }

    #[test]
    fn ou_parent_types_includes_self() {
        let parent_types = OrganizationalUnit::parent_types();
        assert_eq!(parent_types.len(), 1);
        assert_eq!(parent_types[0], "Organizations::OrganizationalUnit");
    }

    #[test]
    fn test_new_ou_is_valid() {
        let parent_hrn = sample_parent_hrn();
        let ou = OrganizationalUnit::new("TestOU".to_string(), parent_hrn.clone());

        assert_eq!(ou.name, "TestOU");
        assert_eq!(ou.parent_hrn, parent_hrn);
        assert!(ou.child_ous.is_empty());
        assert!(ou.child_accounts.is_empty());
        assert!(ou.attached_scps.is_empty());
        assert!(!ou.hrn.to_string().is_empty());
    }

    #[test]
    fn test_add_child_ou() {
        let mut ou = OrganizationalUnit::new("ParentOU".to_string(), sample_parent_hrn());
        let child_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "child-1".to_string(),
        );
        ou.add_child_ou(child_hrn.clone());

        assert!(ou.child_ous.contains(&child_hrn));
        assert_eq!(ou.child_ous.len(), 1);
    }

    #[test]
    fn test_remove_child_ou() {
        let mut ou = OrganizationalUnit::new("ParentOU".to_string(), sample_parent_hrn());
        let child_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "ou".to_string(),
            "child-2".to_string(),
        );
        ou.add_child_ou(child_hrn.clone());

        assert!(ou.child_ous.contains(&child_hrn));

        ou.remove_child_ou(&child_hrn);
        assert!(!ou.child_ous.contains(&child_hrn));
        assert_eq!(ou.child_ous.len(), 0);
    }

    #[test]
    fn test_add_child_account() {
        let mut ou = OrganizationalUnit::new("ParentOU".to_string(), sample_parent_hrn());
        let account_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-1".to_string(),
        );
        ou.add_child_account(account_hrn.clone());

        assert!(ou.child_accounts.contains(&account_hrn));
        assert_eq!(ou.child_accounts.len(), 1);
    }

    #[test]
    fn test_remove_child_account() {
        let mut ou = OrganizationalUnit::new("ParentOU".to_string(), sample_parent_hrn());
        let account_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "account".to_string(),
            "acc-2".to_string(),
        );
        ou.add_child_account(account_hrn.clone());

        assert!(ou.child_accounts.contains(&account_hrn));

        ou.remove_child_account(&account_hrn);
        assert!(!ou.child_accounts.contains(&account_hrn));
        assert_eq!(ou.child_accounts.len(), 0);
    }

    #[test]
    fn test_attach_scp() {
        let mut ou = OrganizationalUnit::new("TestOU".to_string(), sample_parent_hrn());
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-1".to_string(),
        );
        ou.attach_scp(scp_hrn.clone());

        assert!(ou.attached_scps.contains(&scp_hrn));
        assert_eq!(ou.attached_scps.len(), 1);
    }

    #[test]
    fn test_detach_scp() {
        let mut ou = OrganizationalUnit::new("TestOU".to_string(), sample_parent_hrn());
        let scp_hrn = Hrn::new(
            "aws".to_string(),
            "hodei".to_string(),
            "default".to_string(),
            "scp".to_string(),
            "scp-2".to_string(),
        );
        ou.attach_scp(scp_hrn.clone());

        assert!(ou.attached_scps.contains(&scp_hrn));

        ou.detach_scp(&scp_hrn);
        assert!(!ou.attached_scps.contains(&scp_hrn));
        assert_eq!(ou.attached_scps.len(), 0);
    }

    #[test]
    fn ou_attributes_contains_expected_keys() {
        let ou = OrganizationalUnit::new("TestOU".to_string(), sample_parent_hrn());
        let attrs = ou.attributes();
        assert_eq!(attrs.len(), 4);

        let name_attr = AttributeName::new("name").unwrap();
        let type_attr = AttributeName::new("type").unwrap();
        let child_count_attr = AttributeName::new("child_count").unwrap();
        let scps_attr = AttributeName::new("attached_scps").unwrap();

        assert!(attrs.contains_key(&name_attr));
        assert!(attrs.contains_key(&type_attr));
        assert!(attrs.contains_key(&child_count_attr));
        assert!(attrs.contains_key(&scps_attr));

        // Verify values
        assert_eq!(
            attrs.get(&name_attr),
            Some(&AttributeValue::String("TestOU".to_string()))
        );
        assert_eq!(
            attrs.get(&type_attr),
            Some(&AttributeValue::String("organizational_unit".to_string()))
        );
        assert_eq!(attrs.get(&child_count_attr), Some(&AttributeValue::Long(0)));
    }

    #[test]
    fn ou_parent_hrns_returns_parent() {
        let parent = sample_parent_hrn();
        let ou = OrganizationalUnit::new("TestOU".to_string(), parent.clone());

        let parent_hrns = ou.parent_hrns();
        assert_eq!(parent_hrns.len(), 1);
        assert_eq!(parent_hrns[0], parent);
    }

    #[test]
    fn ou_hrn_reference() {
        let parent = sample_parent_hrn();
        let ou = OrganizationalUnit::new("TestOU".to_string(), parent);

        assert_eq!(ou.hrn().resource_type(), "ou");
        assert_eq!(ou.hrn().resource_id(), "TestOU");
    }
}
