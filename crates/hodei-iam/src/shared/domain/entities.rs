//! Domain entities for hodei-iam
//!
//! This module defines the core IAM entities: User, Group, ServiceAccount, Namespace
//! All entities implement the agnostic HodeiEntityType and HodeiEntity traits.

use kernel::{
    AttributeName, AttributeType, AttributeValue, HodeiEntity, HodeiEntityType, Hrn, Principal,
    Resource, ResourceTypeName, ServiceName,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// User Entity
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub hrn: Hrn,
    pub name: String,
    pub group_hrns: Vec<Hrn>,
    pub email: String,
    pub tags: Vec<String>,
}

impl User {
    /// Create a new User
    pub fn new(hrn: Hrn, name: String, email: String) -> Self {
        Self {
            hrn,
            name,
            email,
            group_hrns: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// Add user to a group (idempotent - won't add duplicates)
    pub fn add_to_group(&mut self, group_hrn: Hrn) {
        if !self.group_hrns.contains(&group_hrn) {
            self.group_hrns.push(group_hrn);
        }
    }

    /// Remove user from a group
    pub fn remove_from_group(&mut self, group_hrn: &Hrn) {
        self.group_hrns.retain(|hrn| hrn != group_hrn);
    }

    /// Get all groups this user belongs to
    pub fn groups(&self) -> &[Hrn] {
        &self.group_hrns
    }

    /// Get user's email
    pub fn email(&self) -> &str {
        &self.email
    }
}

impl HodeiEntityType for User {
    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("User").expect("Valid resource type")
    }

    fn is_principal_type() -> bool {
        true
    }

    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        vec![
            (
                AttributeName::new("name").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("email").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("tags").expect("Valid attribute name"),
                AttributeType::set(AttributeType::string()),
            ),
        ]
    }
}

impl HodeiEntity for User {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();

        attrs.insert(
            AttributeName::new("name").expect("Valid attribute name"),
            AttributeValue::string(&self.name),
        );

        attrs.insert(
            AttributeName::new("email").expect("Valid attribute name"),
            AttributeValue::string(&self.email),
        );

        let tag_values: Vec<AttributeValue> = self
            .tags
            .iter()
            .map(|t| AttributeValue::string(t))
            .collect();
        attrs.insert(
            AttributeName::new("tags").expect("Valid attribute name"),
            AttributeValue::set(tag_values),
        );

        attrs
    }

    fn parent_hrns(&self) -> Vec<Hrn> {
        self.group_hrns.clone()
    }
}

impl Principal for User {}
impl Resource for User {}

// ============================================================================
// Group Entity
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub hrn: Hrn,
    pub name: String,
    pub tags: Vec<String>,
    pub attached_policy_hrns: Vec<Hrn>,
}

impl Group {
    /// Create a new Group
    pub fn new(hrn: Hrn, name: String) -> Self {
        Self {
            hrn,
            name,
            tags: Vec::new(),
            attached_policy_hrns: Vec::new(),
        }
    }

    /// Attach a policy to this group (idempotent)
    pub fn attach_policy(&mut self, policy_hrn: Hrn) {
        if !self.attached_policy_hrns.contains(&policy_hrn) {
            self.attached_policy_hrns.push(policy_hrn);
        }
    }

    /// Detach a policy from this group
    pub fn detach_policy(&mut self, policy_hrn: &Hrn) {
        self.attached_policy_hrns.retain(|hrn| hrn != policy_hrn);
    }

    /// Get group name
    pub fn group_name(&self) -> &str {
        &self.name
    }

    /// Get attached policies
    pub fn attached_policies(&self) -> &[Hrn] {
        &self.attached_policy_hrns
    }
}

impl HodeiEntityType for Group {
    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("Group").expect("Valid resource type")
    }

    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        vec![
            (
                AttributeName::new("name").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("tags").expect("Valid attribute name"),
                AttributeType::set(AttributeType::string()),
            ),
        ]
    }
}

impl HodeiEntity for Group {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();

        attrs.insert(
            AttributeName::new("name").expect("Valid attribute name"),
            AttributeValue::string(&self.name),
        );

        let tag_values: Vec<AttributeValue> = self
            .tags
            .iter()
            .map(|t| AttributeValue::string(t))
            .collect();
        attrs.insert(
            AttributeName::new("tags").expect("Valid attribute name"),
            AttributeValue::set(tag_values),
        );

        attrs
    }

    fn parent_hrns(&self) -> Vec<Hrn> {
        Vec::new()
    }
}

impl Resource for Group {}

// ============================================================================
// ServiceAccount Entity
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub hrn: Hrn,
    pub name: String,
    pub annotations: HashMap<String, String>,
    pub tags: Vec<String>,
}

impl ServiceAccount {
    /// Create a new ServiceAccount
    pub fn new(hrn: Hrn, name: String) -> Self {
        Self {
            hrn,
            name,
            annotations: HashMap::new(),
            tags: Vec::new(),
        }
    }
}

impl HodeiEntityType for ServiceAccount {
    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("ServiceAccount").expect("Valid resource type")
    }

    fn is_principal_type() -> bool {
        true
    }

    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        vec![
            (
                AttributeName::new("name").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("annotations").expect("Valid attribute name"),
                AttributeType::record(HashMap::new()), // Generic record type
            ),
            (
                AttributeName::new("tags").expect("Valid attribute name"),
                AttributeType::set(AttributeType::string()),
            ),
        ]
    }
}

impl HodeiEntity for ServiceAccount {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();

        attrs.insert(
            AttributeName::new("name").expect("Valid attribute name"),
            AttributeValue::string(&self.name),
        );

        // Convert annotations HashMap to AttributeValue::Record
        let annotation_values: HashMap<String, AttributeValue> = self
            .annotations
            .iter()
            .map(|(k, v)| (k.clone(), AttributeValue::string(v)))
            .collect();
        attrs.insert(
            AttributeName::new("annotations").expect("Valid attribute name"),
            AttributeValue::record(annotation_values),
        );

        let tag_values: Vec<AttributeValue> = self
            .tags
            .iter()
            .map(|t| AttributeValue::string(t))
            .collect();
        attrs.insert(
            AttributeName::new("tags").expect("Valid attribute name"),
            AttributeValue::set(tag_values),
        );

        attrs
    }

    fn parent_hrns(&self) -> Vec<Hrn> {
        Vec::new()
    }
}

impl Principal for ServiceAccount {}
impl Resource for ServiceAccount {}

// ============================================================================
// Namespace Entity
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    pub hrn: Hrn,
    pub name: String,
    pub tags: Vec<String>,
    pub annotations: HashMap<String, String>,
}

impl Namespace {
    /// Create a new Namespace
    pub fn new(hrn: Hrn, name: String) -> Self {
        Self {
            hrn,
            name,
            tags: Vec::new(),
            annotations: HashMap::new(),
        }
    }
}

impl HodeiEntityType for Namespace {
    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("Namespace").expect("Valid resource type")
    }

    fn attributes_schema() -> Vec<(AttributeName, AttributeType)> {
        vec![
            (
                AttributeName::new("name").expect("Valid attribute name"),
                AttributeType::string(),
            ),
            (
                AttributeName::new("annotations").expect("Valid attribute name"),
                AttributeType::record(HashMap::new()),
            ),
            (
                AttributeName::new("tags").expect("Valid attribute name"),
                AttributeType::set(AttributeType::string()),
            ),
        ]
    }
}

impl HodeiEntity for Namespace {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<AttributeName, AttributeValue> {
        let mut attrs = HashMap::new();

        attrs.insert(
            AttributeName::new("name").expect("Valid attribute name"),
            AttributeValue::string(&self.name),
        );

        let annotation_values: HashMap<String, AttributeValue> = self
            .annotations
            .iter()
            .map(|(k, v)| (k.clone(), AttributeValue::string(v)))
            .collect();
        attrs.insert(
            AttributeName::new("annotations").expect("Valid attribute name"),
            AttributeValue::record(annotation_values),
        );

        let tag_values: Vec<AttributeValue> = self
            .tags
            .iter()
            .map(|t| AttributeValue::string(t))
            .collect();
        attrs.insert(
            AttributeName::new("tags").expect("Valid attribute name"),
            AttributeValue::set(tag_values),
        );

        attrs
    }

    fn parent_hrns(&self) -> Vec<Hrn> {
        Vec::new()
    }
}

impl Resource for Namespace {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod group_tests {
    use super::*;

    #[test]
    fn test_group_new_creates_empty_collections() {
        let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
        let group = Group::new(hrn, "Developers".to_string());

        assert_eq!(group.name, "Developers");
        assert_eq!(group.tags.len(), 0);
        assert_eq!(group.attached_policies().len(), 0);
    }

    #[test]
    fn test_group_attach_policy_idempotent() {
        let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
        let mut group = Group::new(hrn, "Developers".to_string());

        let policy_hrn = Hrn::new(
            "hodei".into(),
            "policies".into(),
            "default".into(),
            "Policy".into(),
            "policy1".into(),
        );

        // Attach policy twice
        group.attach_policy(policy_hrn.clone());
        group.attach_policy(policy_hrn.clone());

        // Should only have one policy
        assert_eq!(group.attached_policies().len(), 1);
        assert_eq!(group.attached_policies()[0], policy_hrn);
    }

    #[test]
    fn test_group_detach_policy() {
        let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
        let mut group = Group::new(hrn, "Developers".to_string());

        let policy1 = Hrn::new(
            "hodei".into(),
            "policies".into(),
            "default".into(),
            "Policy".into(),
            "p1".into(),
        );
        let policy2 = Hrn::new(
            "hodei".into(),
            "policies".into(),
            "default".into(),
            "Policy".into(),
            "p2".into(),
        );

        group.attach_policy(policy1.clone());
        group.attach_policy(policy2.clone());
        assert_eq!(group.attached_policies().len(), 2);

        group.detach_policy(&policy1);
        assert_eq!(group.attached_policies().len(), 1);
        assert_eq!(group.attached_policies()[0], policy2);
    }

    #[test]
    fn test_group_detach_nonexistent_policy_does_nothing() {
        let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
        let mut group = Group::new(hrn, "Developers".to_string());

        let policy_hrn = Hrn::new(
            "hodei".into(),
            "policies".into(),
            "default".into(),
            "Policy".into(),
            "p1".into(),
        );

        // Detach policy that doesn't exist
        group.detach_policy(&policy_hrn);

        assert_eq!(group.attached_policies().len(), 0);
    }

    #[test]
    fn test_group_name_getter() {
        let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
        let group = Group::new(hrn, "Developers".to_string());

        assert_eq!(group.group_name(), "Developers");
    }

    #[test]
    fn test_group_multiple_policies() {
        let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
        let mut group = Group::new(hrn, "Developers".to_string());

        let policy1 = Hrn::new(
            "hodei".into(),
            "policies".into(),
            "default".into(),
            "Policy".into(),
            "p1".into(),
        );
        let policy2 = Hrn::new(
            "hodei".into(),
            "policies".into(),
            "default".into(),
            "Policy".into(),
            "p2".into(),
        );
        let policy3 = Hrn::new(
            "hodei".into(),
            "policies".into(),
            "default".into(),
            "Policy".into(),
            "p3".into(),
        );

        group.attach_policy(policy1);
        group.attach_policy(policy2);
        group.attach_policy(policy3);

        assert_eq!(group.attached_policies().len(), 3);
    }

    #[test]
    fn test_group_hodei_entity_type() {
        assert_eq!(Group::service_name().as_str(), "iam");
        assert_eq!(Group::resource_type_name().as_str(), "Group");
        assert_eq!(Group::entity_type_name(), "Iam::Group");
    }

    #[test]
    fn test_group_attributes() {
        let hrn = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());
        let mut group = Group::new(hrn, "Developers".to_string());
        group.tags.push("tag1".to_string());
        group.tags.push("tag2".to_string());

        let attrs = group.attributes();
        assert!(attrs.contains_key(&AttributeName::new("name").unwrap()));
        assert!(attrs.contains_key(&AttributeName::new("tags").unwrap()));

        let name_attr = attrs.get(&AttributeName::new("name").unwrap()).unwrap();
        assert_eq!(name_attr.as_string(), Some("Developers"));
    }
}

#[cfg(test)]
mod user_tests {
    use super::*;

    #[test]
    fn test_user_new_creates_empty_groups() {
        let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
        let user = User::new(hrn, "Alice".to_string(), "alice@example.com".to_string());

        assert_eq!(user.name, "Alice");
        assert_eq!(user.email(), "alice@example.com");
        assert_eq!(user.groups().len(), 0);
        assert_eq!(user.tags.len(), 0);
    }

    #[test]
    fn test_user_add_to_group_idempotent() {
        let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
        let mut user = User::new(hrn, "Alice".to_string(), "alice@example.com".to_string());

        let group_hrn =
            Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());

        // Add to group twice
        user.add_to_group(group_hrn.clone());
        user.add_to_group(group_hrn.clone());

        // Should only be in group once
        assert_eq!(user.groups().len(), 1);
        assert_eq!(user.groups()[0], group_hrn);
    }

    #[test]
    fn test_user_remove_from_group() {
        let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
        let mut user = User::new(hrn, "Alice".to_string(), "alice@example.com".to_string());

        let group1 = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "g1".into());
        let group2 = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "g2".into());

        user.add_to_group(group1.clone());
        user.add_to_group(group2.clone());
        assert_eq!(user.groups().len(), 2);

        user.remove_from_group(&group1);
        assert_eq!(user.groups().len(), 1);
        assert_eq!(user.groups()[0], group2);
    }

    #[test]
    fn test_user_remove_nonexistent_group_does_nothing() {
        let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
        let mut user = User::new(hrn, "Alice".to_string(), "alice@example.com".to_string());

        let group_hrn =
            Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "group1".into());

        // Remove group user is not in
        user.remove_from_group(&group_hrn);

        assert_eq!(user.groups().len(), 0);
    }

    #[test]
    fn test_user_email_getter() {
        let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
        let user = User::new(hrn, "Alice".to_string(), "alice@example.com".to_string());

        assert_eq!(user.email(), "alice@example.com");
    }

    #[test]
    fn test_user_multiple_groups() {
        let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
        let mut user = User::new(hrn, "Alice".to_string(), "alice@example.com".to_string());

        let group1 = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "g1".into());
        let group2 = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "g2".into());
        let group3 = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "g3".into());

        user.add_to_group(group1);
        user.add_to_group(group2);
        user.add_to_group(group3);

        assert_eq!(user.groups().len(), 3);
    }

    #[test]
    fn test_user_hodei_entity_type() {
        assert_eq!(User::service_name().as_str(), "iam");
        assert_eq!(User::resource_type_name().as_str(), "User");
        assert_eq!(User::entity_type_name(), "Iam::User");
        assert!(User::is_principal_type());
    }

    #[test]
    fn test_user_attributes() {
        let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
        let user = User::new(hrn, "Alice".to_string(), "alice@example.com".to_string());

        let attrs = user.attributes();
        assert!(attrs.contains_key(&AttributeName::new("name").unwrap()));
        assert!(attrs.contains_key(&AttributeName::new("email").unwrap()));

        let email_attr = attrs.get(&AttributeName::new("email").unwrap()).unwrap();
        assert_eq!(email_attr.as_string(), Some("alice@example.com"));
    }

    #[test]
    fn test_user_parent_hrns() {
        let hrn = Hrn::for_entity_type::<User>("hodei".into(), "default".into(), "user1".into());
        let mut user = User::new(hrn, "Alice".to_string(), "alice@example.com".to_string());

        let group1 = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "g1".into());
        let group2 = Hrn::for_entity_type::<Group>("hodei".into(), "default".into(), "g2".into());

        user.add_to_group(group1.clone());
        user.add_to_group(group2.clone());

        let parents = user.parent_hrns();
        assert_eq!(parents.len(), 2);
        assert!(parents.contains(&group1));
        assert!(parents.contains(&group2));
    }
}
