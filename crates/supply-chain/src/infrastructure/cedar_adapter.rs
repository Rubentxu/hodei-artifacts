//! Cedar Policy adapters for Supply Chain resources

use cedar_policy::{EntityUid, EntityTypeName, EntityId, RestrictedExpression};
use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::attestation::Attestation;
use crate::domain::scan_result::ScanResult;
use shared::security::HodeiResource;

/// Cedar Policy adapter implementation for Attestation
impl HodeiResource<EntityUid, RestrictedExpression> for Attestation {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:supply-chain:<region>:<org_id>:attestation/<attestation_id>
        let parts: Vec<&str> = self.hrn.as_str().split(':').collect();
        if parts.len() >= 6 {
            let entity_type = EntityTypeName::from_str("Attestation").unwrap();
            let entity_id = EntityId::from_str(parts[5]).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        } else {
            // Fallback: use entire HRN as ID
            let entity_type = EntityTypeName::from_str("Attestation").unwrap();
            let entity_id = EntityId::from_str(self.hrn.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        }

/// Cedar Policy adapter implementation for ScanResult
impl HodeiResource<EntityUid, RestrictedExpression> for ScanResult {
    fn resource_id(&self) -> EntityUid {
        // Parse HRN format: hrn:hodei:supply-chain:<region>:<org_id>:scan-result/<id>
        let parts: Vec<&str> = self.id.as_str().split(':').collect();
        if parts.len() >= 6 {
            let entity_type = EntityTypeName::from_str("ScanResult").unwrap();
            let entity_id = EntityId::from_str(parts[5]).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        } else {
            // Fallback: use entire HRN as ID
            let entity_type = EntityTypeName::from_str("ScanResult").unwrap();
            let entity_id = EntityId::from_str(self.id.as_str()).unwrap();
            EntityUid::from_type_name_and_id(entity_type, entity_id)
        }
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("scan_result".to_string()));
        attrs.insert("scanner".to_string(), RestrictedExpression::new_string(self.scanner.clone()));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // Parent: the physical artifact scanned
        vec![EntityUid::from_str(self.artifact.0.as_str()).unwrap()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::{Hrn, PhysicalArtifactId};
    use time::OffsetDateTime;

    fn scan_result() -> ScanResult {
        ScanResult {
            id: Hrn::new("hrn:hodei:supply-chain:eu-west-1:acme:scan-result/scan-1").unwrap(),
            artifact: PhysicalArtifactId(Hrn::new("hrn:hodei:artifact:eu-west-1:acme:physical-artifact/sha256-deadbeef").unwrap()),
            scanner: "grype".into(),
            results: "{}".into(),
            scanned_at: OffsetDateTime::now_utc(),
        }
    }

    #[test]
    fn scan_result_adapter_smoke() {
        let sr = scan_result();
        let id = <ScanResult as HodeiResource<EntityUid, RestrictedExpression>>::resource_id(&sr);
        let attrs = <ScanResult as HodeiResource<EntityUid, RestrictedExpression>>::resource_attributes(&sr);
        let parents = <ScanResult as HodeiResource<EntityUid, RestrictedExpression>>::resource_parents(&sr);
        assert!(!id.to_string().is_empty());
        assert!(attrs.contains_key("scanner"));
        assert_eq!(parents.len(), 1);
    }
}
    }

    fn resource_attributes(&self) -> HashMap<String, RestrictedExpression> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), RestrictedExpression::new_string("attestation".to_string()));
        attrs.insert("predicate_type".to_string(), RestrictedExpression::new_string(format!("{:?}", self.predicate_type)));
        attrs
    }

    fn resource_parents(&self) -> Vec<EntityUid> {
        // El padre de una atestación es su organización.
        vec![EntityUid::from_str(self.organization_hrn.0.as_str()).unwrap()]
    }
}