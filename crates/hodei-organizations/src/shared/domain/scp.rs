use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Cedar Policy ecosystem
use cedar_policy::RestrictedExpression;
use policies::shared::domain::ports::{AttributeType, HodeiEntity, HodeiEntityType, Resource};

// HRN (Hierarchical Resource Name)
use policies::shared::domain::hrn::Hrn;

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
// Cedar Policy Integration
// ============================================================================

impl HodeiEntityType for ServiceControlPolicy {
    fn service_name() -> &'static str {
        "organizations"
    }

    fn resource_type_name() -> &'static str {
        "ServiceControlPolicy"
    }

    fn is_principal_type() -> bool {
        false
    }

    /// Declare Cedar schema attributes exposed for authorization decisions.
    ///
    /// We expose:
    /// - name: String (metadata)
    /// - type: String (fixed discriminator: "service_control_policy")
    /// - document: String (raw policy source; consider hashing or restricting
    ///   exposure in future iterations if size or sensitivity becomes an issue)
    fn cedar_attributes() -> Vec<(&'static str, AttributeType)> {
        vec![
            ("name", AttributeType::Primitive("String")),
            ("type", AttributeType::Primitive("String")),
            ("document", AttributeType::Primitive("String")),
        ]
    }

    /// SCP itself has no structural parents in the Cedar graph (attachments
    /// create relationships from the target side). We keep this empty.
    fn cedar_parents_types() -> Vec<&'static str> {
        Vec::new()
    }
}

impl HodeiEntity for ServiceControlPolicy {
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
            RestrictedExpression::new_string("service_control_policy".to_string()),
        );
        attrs.insert(
            "document".to_string(),
            RestrictedExpression::new_string(self.document.clone()),
        );
        attrs
    }

    fn parents(&self) -> Vec<cedar_policy::EntityUid> {
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
    use cedar_policy::EntityUid;

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
        assert_eq!(ServiceControlPolicy::service_name(), "organizations");
        assert_eq!(
            ServiceControlPolicy::resource_type_name(),
            "ServiceControlPolicy"
        );
        assert!(!ServiceControlPolicy::is_principal_type());
        let attrs = ServiceControlPolicy::cedar_attributes();
        assert!(attrs.iter().any(|(k, _)| *k == "name"));
        assert!(attrs.iter().any(|(k, _)| *k == "type"));
        assert!(attrs.iter().any(|(k, _)| *k == "document"));
    }

    #[test]
    fn scp_attributes_map_contains_expected_keys() {
        let scp = ServiceControlPolicy::new(
            sample_hrn(),
            "AllowAll".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let attrs = scp.attributes();
        assert_eq!(attrs.get("name").is_some(), true);
        assert_eq!(attrs.get("type").is_some(), true);
        assert_eq!(attrs.get("document").is_some(), true);
    }

    #[test]
    fn scp_euid_builds() {
        let scp = ServiceControlPolicy::new(
            sample_hrn(),
            "AllowAll".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        let euid: EntityUid = scp.euid();
        let rendered = format!("{}", euid);
        assert!(
            rendered.contains("Organizations::ServiceControlPolicy")
                || rendered.contains("Organizations::Servicecontrolpolicy"),
            "Rendered EUID should contain namespaced type, got: {rendered}"
        );
        assert!(rendered.contains("scp-123"));
    }

    #[test]
    fn scp_has_no_parents() {
        let scp = ServiceControlPolicy::new(
            sample_hrn(),
            "AllowAll".to_string(),
            "permit(principal, action, resource);".to_string(),
        );
        assert!(scp.parents().is_empty());
    }
}
