use cedar_policy::{EntityUid, RestrictedExpression};
use kernel::AttributeType::*;
/// Domain entities for hodei-iam
///
/// This module defines the core IAM entities: User, Group, ServiceAccount, Namespace
use kernel::Hrn;
use kernel::{AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub hrn: Hrn,
    pub name: String,
    pub annotations: HashMap<String, String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    pub hrn: Hrn,
    pub name: String,
    pub tags: Vec<String>,
    pub annotations: HashMap<String, String>,
}

// --- Implementaciones para User ---

impl HodeiEntityType for User {
    fn service_name() -> &'static str {
        "iam"
    }

    fn resource_type_name() -> &'static str {
        "User"
    }

    fn is_principal_type() -> bool {
        true
    }

    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("email", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for User {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn parents(&self) -> Vec<EntityUid> {
        self.group_hrns.iter().map(|hrn| hrn.to_euid()).collect()
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        attrs.insert(
            "email".to_string(),
            RestrictedExpression::new_string(self.email.clone()),
        );
        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }
}

impl Principal for User {}
impl Resource for User {}

// --- Implementaciones para Group ---

impl HodeiEntityType for Group {
    fn service_name() -> &'static str {
        "iam"
    }

    fn resource_type_name() -> &'static str {
        "Group"
    }

    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for Group {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );
        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for Group {}

// --- Implementaciones para ServiceAccount ---

impl HodeiEntityType for ServiceAccount {
    fn service_name() -> &'static str {
        "iam"
    }

    fn resource_type_name() -> &'static str {
        "ServiceAccount"
    }

    fn is_principal_type() -> bool {
        true
    }

    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("annotations", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for ServiceAccount {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );

        let annotation_map: BTreeMap<String, RestrictedExpression> = self
            .annotations
            .iter()
            .map(|(k, v)| (k.clone(), RestrictedExpression::new_string(v.clone())))
            .collect();
        attrs.insert(
            "annotations".to_string(),
            RestrictedExpression::new_record(annotation_map).unwrap_or_else(|_| {
                RestrictedExpression::new_string("error_creating_record".to_string())
            }),
        );

        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Principal for ServiceAccount {}
impl Resource for ServiceAccount {}

// --- Implementaciones para Namespace ---

impl HodeiEntityType for Namespace {
    fn service_name() -> &'static str {
        "iam"
    }
    fn resource_type_name() -> &'static str {
        "Namespace"
    }

    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("annotations", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

impl HodeiEntity for Namespace {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );

        let annotation_map: BTreeMap<String, RestrictedExpression> = self
            .annotations
            .iter()
            .map(|(k, v)| (k.clone(), RestrictedExpression::new_string(v.clone())))
            .collect();
        attrs.insert(
            "annotations".to_string(),
            RestrictedExpression::new_record(annotation_map).unwrap_or_else(|_| {
                RestrictedExpression::new_string("error_creating_record".to_string())
            }),
        );

        let tag_exprs: Vec<RestrictedExpression> = self
            .tags
            .iter()
            .map(|t| RestrictedExpression::new_string(t.clone()))
            .collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        Vec::new()
    }
}

impl Resource for Namespace {}

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
}
