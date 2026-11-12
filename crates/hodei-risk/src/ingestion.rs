//! The ingestion of the risk: what a vulnerability does to an artefact.
//!
//! This is where an advisory and an inventory meet. The advisory says
//! which packages and which versions are affected; the inventory says
//! which packages and which versions the artefact has. The result is a
//! list of findings, each one saying whether the artefact is affected, how
//! far the dependency is, and -- when the inventory is not there -- that
//! nobody knows.
//!
//! The ingestion never asks for anything it was not given. It does not
//! scan, it does not query a database, and it does not decide that an
//! artefact is fine because it found nothing.

use hodei_evidence::domain::standards::sbom::Sbom;
use serde::{Deserialize, Serialize};

use crate::affected::{AffectedState, DependencyKind, Finding, state_without_vex};
use crate::osv::OsvAdvisory;
use crate::version::match_version;

/// The name of a component, as the inventory and the advisory write it.
///
/// The two do not always write it the same way: the inventory names a
/// component by its reference and the advisory by its package name. The
/// comparison is by the coordinates, which is what both carry.
pub fn matches_package(package: &str, component_name: &str, purl: Option<&str>) -> bool {
    if package == component_name {
        return true;
    }
    // The PURL of the advisory names the package inside it, and that name
    // is compared with the name of the COMPONENT. Comparing it with the
    // name of the package would say that every component matches an
    // advisory that carries a PURL, because the PURL always names the
    // package the advisory is about.
    let Some(purl) = purl else {
        return false;
    };
    let Some(rest) = purl.strip_prefix("pkg:") else {
        return false;
    };
    let without_type = rest.split_once('/').map(|(_, name)| name).unwrap_or(rest);
    let name = without_type
        .split_once('@')
        .map(|(name, _)| name)
        .unwrap_or(without_type);
    let name = name.split_once('?').map(|(name, _)| name).unwrap_or(name);

    name == component_name || name.replace('/', ":") == component_name
}

/// The component of an inventory, with the version the advisory compares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryComponent {
    /// The reference of the component inside the inventory.
    pub reference: String,
    /// The name of the package.
    pub name: String,
    /// The version of the package.
    pub version: Option<String>,
    /// The PURL of the package.
    pub purl: Option<String>,
}

/// The result of ingesting an advisory against an inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskIngestion {
    /// The findings, one per component that the advisory is about.
    pub findings: Vec<Finding>,
    /// Whether the inventory was there at all.
    ///
    /// It is said out loud because its absence changes everything: without
    /// an inventory there are no findings and the state of the artefact is
    /// unknown.
    pub inventory_present: bool,
}

/// Ingests an advisory against an inventory.
///
/// The inventory may be absent, and then there is nothing to compare and
/// the answer is that nobody knows.
pub fn ingest(
    advisory: &OsvAdvisory,
    inventory: Option<&Sbom>,
    components: &[InventoryComponent],
) -> RiskIngestion {
    let Some(inventory) = inventory else {
        return RiskIngestion {
            findings: Vec::new(),
            inventory_present: false,
        };
    };

    let mut findings = Vec::new();

    for affected in &advisory.affected {
        for component in components {
            if !matches_package(
                &affected.package.name,
                &component.name,
                affected.package.purl.as_deref(),
            ) {
                continue;
            }

            let dependency = dependency_of(component, inventory);
            let match_result = match &component.version {
                Some(version) => match_version(affected, version),
                None => crate::version::Match::Unknown,
            };

            findings.push(state_without_vex(
                &advisory.id,
                &component.name,
                component.version.as_deref(),
                true,
                match_result,
                dependency,
            ));
        }
    }

    RiskIngestion {
        findings,
        inventory_present: true,
    }
}

/// Whether the component is a direct dependency of the subject or a
/// transitive one.
///
/// The inventory knows: the subject depends on its direct components, and
/// each component depends on the next. A component that the subject does
/// not depend on directly is transitive.
fn dependency_of(component: &InventoryComponent, inventory: &Sbom) -> DependencyKind {
    // The subject depends on its direct components, and each component
    // depends on the next. A component that the subject does not depend
    // on directly is transitive.
    let direct = match &inventory.subject {
        Some(subject) => inventory.dependencies_of(&subject.reference),
        None => Vec::new(),
    };
    if direct
        .iter()
        .any(|reference| reference == &component.reference)
    {
        DependencyKind::Direct
    } else {
        DependencyKind::Transitive
    }
}

/// The state of an artefact with respect to an advisory.
///
/// It is `Affected` when any finding is affected, `Unaffected` when the
/// inventory was there and no finding is affected, and `Unknown` when the
/// inventory was not there or when a finding is unknown. The third one
/// wins over the second: one unknown finding among five unafffected ones
/// means that nobody can say the artefact is fine.
pub fn state_of(ingestion: &RiskIngestion) -> AffectedState {
    if !ingestion.inventory_present {
        return AffectedState::Unknown;
    }
    if ingestion
        .findings
        .iter()
        .any(|finding| finding.state == AffectedState::Affected)
    {
        return AffectedState::Affected;
    }
    if ingestion
        .findings
        .iter()
        .any(|finding| finding.state == AffectedState::Unknown)
    {
        return AffectedState::Unknown;
    }
    AffectedState::Unaffected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osv::read_osv;

    const ADVISORY: &str = r#"{
      "id": "GHSA-test-0010",
      "affected": [
        {
          "package": {"ecosystem": "Maven", "name": "org.example:lib-b"},
          "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.0.1"}]}]
        }
      ]
    }"#;

    #[test]
    fn without_an_inventory_nobody_knows() {
        let advisory = read_osv(ADVISORY).expect("the document is read");
        let ingestion = ingest(&advisory, None, &[]);
        assert!(!ingestion.inventory_present);
        assert_eq!(state_of(&ingestion), AffectedState::Unknown);
    }
}
