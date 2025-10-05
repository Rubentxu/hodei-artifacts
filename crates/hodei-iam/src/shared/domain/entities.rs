use cedar_policy::{EntityUid, RestrictedExpression};
use serde::{Deserialize, Serialize};
use kernel::AttributeType::*;
/// Domain entities for hodei-iam
///
/// This module defines the core IAM entities: User, Group, ServiceAccount, Namespace
use kernel::Hrn;
use kernel::{AttributeType, HodeiEntity, HodeiEntityType, Principal, Resource};
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
