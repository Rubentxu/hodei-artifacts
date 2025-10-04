use policies::shared::Hrn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// Cedar Policy traits
use cedar_policy::{EntityUid, RestrictedExpression};
use policies::shared::domain::ports::{AttributeType, HodeiEntity, HodeiEntityType, Resource};
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
// Cedar Policy Integration
// ============================================================================

/// Implementation of HodeiEntityType for Account
/// Provides type-level metadata for Cedar schema generation
impl HodeiEntityType for Account {
    fn service_name() -> &'static str {
        "organizations"
    }

    fn resource_type_name() -> &'static str {
        "Account"
    }

    fn is_principal_type() -> bool {
        false // Account is a Resource, not a Principal
    }

    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("type", AttributeType::Primitive("String")),
            (
                "attached_scps",
                AttributeType::Set(Box::new(AttributeType::Primitive("String"))),
            ),
        ]
    }

    fn cedar_parents_types() -> Vec<&'static str> {
        vec!["OrganizationalUnit"]
    }
}

/// Implementation of HodeiEntity for Account
/// Provides instance-level data for Cedar authorization
impl HodeiEntity for Account {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();

        attrs.insert(
            "name".to_string(),
            RestrictedExpression::new_string(self.name.clone()),
        );

        attrs.insert(
            "type".to_string(),
            RestrictedExpression::new_string("account".to_string()),
        );

        let scps_expr: Vec<RestrictedExpression> = self
            .attached_scps
            .iter()
            .map(|scp_hrn| RestrictedExpression::new_string(scp_hrn.to_string()))
            .collect();
        attrs.insert(
            "attached_scps".to_string(),
            RestrictedExpression::new_set(scps_expr),
        );

        attrs
    }

    fn parents(&self) -> Vec<EntityUid> {
        if let Some(parent) = &self.parent_hrn {
            vec![parent.to_euid()]
        } else {
            Vec::new()
        }
    }
}

/// Marker trait implementation: Account is a Resource
impl Resource for Account {}
