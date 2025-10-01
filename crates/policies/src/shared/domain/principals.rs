use crate::shared::domain::hrn::Hrn;
use crate::shared::domain::ports::{self, HodeiEntity, HodeiEntityType};
use ports::AttributeType::*;
use cedar_policy::{EntityUid, RestrictedExpression};
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_group(id: &str) -> Group {
        Group {
            hrn: Hrn::new(
                "aws".into(),
                "hodei".into(),
                "123".into(),
                "Group".into(),
                id.into(),
            ),
            name: format!("group-{}", id),
            tags: vec!["team".into()],
        }
    }

    #[test]
    fn group_attributes_contains_expected_keys() {
        let g = sample_group("dev");
        let attrs = g.attributes();
        assert!(attrs.contains_key("name"));
        assert!(attrs.contains_key("tags"));
    }

    #[test]
    fn user_parents_produce_entityuids() {
        let groups = vec![
            Hrn::new("aws".into(), "hodei".into(), "123".into(), "Group".into(), "dev".into()),
            Hrn::new("aws".into(), "hodei".into(), "123".into(), "Group".into(), "ops".into()),
        ];
        let user = User {
            hrn: Hrn::new("aws".into(), "hodei".into(), "123".into(), "User".into(), "alice".into()),
            name: "Alice".into(),
            group_hrns: groups,
            email: "alice@example.com".into(),
            tags: vec!["admin".into()],
        };
        let parents = user.parents();
        assert_eq!(parents.len(), 2);
        let s0 = format!("{}", parents[0]);
        assert!(s0.contains("Group"));
    }

    #[test]
    fn user_attributes_contains_expected() {
        let user = User {
            hrn: Hrn::new("aws".into(), "hodei".into(), "123".into(), "User".into(), "alice".into()),
            name: "Alice".into(),
            group_hrns: vec![],
            email: "alice@example.com".into(),
            tags: vec!["owner".into()],
        };
        let attrs = user.attributes();
        assert!(attrs.contains_key("name"));
        assert!(attrs.contains_key("email"));
        assert!(attrs.contains_key("tags"));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub hrn: Hrn,
    pub name: String,
    pub tags: Vec<String>,
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
    fn entity_type_name() -> &'static str {
        "User"
    }

    fn is_principal_type() -> bool { true }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
        vec![
            ("name", Primitive("String")),
            ("email", Primitive("String")),
            ("tags", Set(Box::new(Primitive("String")))),
        ]
    }
}

// --- Implementaciones para Group ---

impl HodeiEntityType for Group {
    fn entity_type_name() -> &'static str {
        "Group"
    }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
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
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        let tag_exprs: Vec<RestrictedExpression> = self.tags.iter().map(|t| RestrictedExpression::new_string(t.clone())).collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }
    
    fn parents(&self) -> Vec<EntityUid> {
        // Groups don't have parents in this model
        Vec::new()
    }
}

// --- Implementaciones para ServiceAccount ---

impl HodeiEntityType for ServiceAccount {
    fn entity_type_name() -> &'static str {
        "ServiceAccount"
    }

    fn is_principal_type() -> bool { true }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
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
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        
        // Convert annotations to a record
        let annotation_map: BTreeMap<String, RestrictedExpression> = self.annotations
            .iter()
            .map(|(k, v)| (k.clone(), RestrictedExpression::new_string(v.clone())))
            .collect();
        attrs.insert("annotations".to_string(), RestrictedExpression::new_record(annotation_map).unwrap_or_else(|_|
            RestrictedExpression::new_string("error_creating_record".to_string())));
        
        let tag_exprs: Vec<RestrictedExpression> = self.tags.iter().map(|t| RestrictedExpression::new_string(t.clone())).collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }
    
    fn parents(&self) -> Vec<EntityUid> {
        // Service accounts don't have parents in this model
        Vec::new()
    }
}

// --- Implementaciones para Namespace ---

impl HodeiEntityType for Namespace {
    fn entity_type_name() -> &'static str {
        "Namespace"
    }

    fn cedar_attributes() -> Vec<(&'static str, ports::AttributeType)> {
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
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        
        // Convert annotations to a record
        let annotation_map: BTreeMap<String, RestrictedExpression> = self.annotations
            .iter()
            .map(|(k, v)| (k.clone(), RestrictedExpression::new_string(v.clone())))
            .collect();
        attrs.insert("annotations".to_string(), RestrictedExpression::new_record(annotation_map).unwrap_or_else(|_|
            RestrictedExpression::new_string("error_creating_record".to_string())));
        
        let tag_exprs: Vec<RestrictedExpression> = self.tags.iter().map(|t| RestrictedExpression::new_string(t.clone())).collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }
    
    fn parents(&self) -> Vec<EntityUid> { Vec::new() }
}

impl HodeiEntity for User {
    fn hrn(&self) -> &Hrn {
        &self.hrn
    }

    // El método `parents` es la implementación a nivel de instancia de `memberOf`.
    fn parents(&self) -> Vec<EntityUid> {
        self.group_hrns.iter().map(|hrn| hrn.euid()).collect()
    }

    fn attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), RestrictedExpression::new_string(self.name.clone()));
        attrs.insert("email".to_string(), RestrictedExpression::new_string(self.email.clone()));
        let tag_exprs: Vec<RestrictedExpression> = self.tags.iter().map(|t| RestrictedExpression::new_string(t.clone())).collect();
        attrs.insert("tags".to_string(), RestrictedExpression::new_set(tag_exprs));
        attrs
    }
}