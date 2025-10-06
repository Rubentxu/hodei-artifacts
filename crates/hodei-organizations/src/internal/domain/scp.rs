use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use kernel::{
    AttributeName, AttributeType, AttributeValue, HodeiEntity, HodeiEntityType, Hrn, Resource,
    ResourceTypeName, ServiceName,
};

/// Domain entity representing an Organization Service Control Policy (SCP)
///
/// An SCP is a policy document (in Cedar DSL) that can be attached to different
/// organizational targets (Accounts, Organizational Units, or the Root).
///
/// This struct is intentionally minimal - higher level use cases handle
/// validation, attachment lifecycle, and event emission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceControlPolicy {
    /// Unique identifier HRN
    pub hrn: Hrn,
    /// Human-friendly name
    pub name: String,
    /// Raw Cedar policy document (source form)
    pub document: String,
}

impl ServiceControlPolicy {
    /// Create a new Service Control Policy
    pub fn new(hrn: Hrn, name: String, document: String) -> Self {
        Self {
            hrn,
            name,
            document,
        }
    }
}

// ============================================================================
// Kernel Agnostic Integration
// ============================================================================

impl HodeiEntityType for ServiceControlPolicy {
    fn service_name() -> ServiceName {
        ServiceName::new("organizations").expect("Valid service name")
    }

    fn resource_type_name() -> ResourceTypeName {
        ResourceTypeName::new("ServiceControlPolicy").expect("Valid resource type name")
    }

    fn is_principal_type() -> bool {
        false
    }

    fn is_resource_type() -> bool {
        true
    }

    /// Declare schema attributes exposed for authorization decisions.
    ///
    /// We expose:
    /// - name: String (metadata)
    /// - type: String (fixed discriminator: "service_control_policy")
    /// - document: String (raw policy source)
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
                AttributeName::new("document").expect("Valid attribute name"),
                AttributeType::string(),
            ),
        ]
    }

    /// SCP itself has no structural parents in the authorization graph
    /// (attachments create relationships from the target side).
    fn parent_types() -> Vec<String> {
        Vec::new()
    }
}

impl HodeiEntity for ServiceControlPolicy {
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
            AttributeValue::String("service_control_policy".to_string()),
        );
        attrs.insert(
            AttributeName::new("document").expect("Valid attribute name"),
            AttributeValue::String(self.document.clone()),
        );
        attrs
    }

    fn parent_hrns(&self) -> Vec<Hrn> {
        // Attachments (to Accounts / OUs / Root) are represented elsewhere; SCP
        // itself has no inherent parent edge.
        Vec::new()
    }
}

/// Marker trait: SCPs are Resources (never Principals)
impl Resource for ServiceControlPolicy {}

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
            "ServiceControlPolicy".to_string(),
            "scp-123".to_string(),
        )
    }

    #[test]
    fn scp_entity_type_metadata() {
        let service = ServiceControlPolicy::service_name();
        assert_eq!(service.as_str(), "organizations");

        let resource_type = ServiceControlPolicy::resource_type_name();
        assert_eq!(resource_type.as_str(), "ServiceControlPolicy");

        assert!(!ServiceControlPolicy::is_principal_type());
        assert!(ServiceControlPolicy::is_resource_type());
    }

    #[test]
    fn scp_entity_type_name_generation() {
        let entity_type_name = ServiceControlPolicy::entity_type_name();
        assert_eq!(entity_type_name, "Organizations::ServiceControlPolicy");
    }

    #[test]
    fn scp_attributes_schema_contains_expected_fields() {
        let schema = ServiceControlPolicy::attributes_schema();
        assert_eq!(schema.len(), 3);

        let names: Vec<String> = schema
            .iter()
            .map(|(name, _)| name.as_str().to_string())
            .collect();
        assert!(names.contains(&"name".to_string()));
        assert!(names.contains(&"type".to_string()));
        assert!(names.contains(&"document".to_string()));
    }

    #[test]
    fn scp_attributes_map_contains_expected_keys() {
        let scp = ServiceControlPolicy::new(
            sample_hrn(),
            "AllowAll".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let attrs = scp.attributes();
        assert_eq!(attrs.len(), 3);

        let name_attr = AttributeName::new("name").unwrap();
        let type_attr = AttributeName::new("type").unwrap();
        let document_attr = AttributeName::new("document").unwrap();

        assert!(attrs.contains_key(&name_attr));
        assert!(attrs.contains_key(&type_attr));
        assert!(attrs.contains_key(&document_attr));

        // Verify values
        assert_eq!(
            attrs.get(&name_attr),
            Some(&AttributeValue::String("AllowAll".to_string()))
        );
        assert_eq!(
            attrs.get(&type_attr),
            Some(&AttributeValue::String(
                "service_control_policy".to_string()
            ))
        );
    }

    #[test]
    fn scp_hrn_reference() {
        let hrn = sample_hrn();
        let scp = ServiceControlPolicy::new(
            hrn.clone(),
            "TestPolicy".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        assert_eq!(scp.hrn(), &hrn);
    }

    #[test]
    fn scp_has_no_parents() {
        let scp = ServiceControlPolicy::new(
            sample_hrn(),
            "AllowAll".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        assert!(scp.parent_hrns().is_empty());
    }

    #[test]
    fn scp_parent_types_empty() {
        let parent_types = ServiceControlPolicy::parent_types();
        assert!(parent_types.is_empty());
    }
}
