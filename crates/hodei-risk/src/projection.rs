//! The projection of the risk into the graph.
//!
//! The inventory says which components an artefact has and what depends
//! on what; the advisory says which components are affected. The graph is
//! where the two meet, and the projection is what puts them there:
//!
//! ```text
//! artefact  --contains-->  component
//! component --depends_on-> component
//! component --affected_by-> vulnerability
//! ```
//!
//! The projection is a pure function of what it is given. It does not
//! query anything and it does not decide: it says what the graph should
//! hold, and the graph is written once or not at all.

use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType,
};
use hodei_evidence::domain::standards::sbom::Sbom;
use kernel::Hrn;

use crate::osv::OsvAdvisory;

/// The identity of a component in the graph.
///
/// The name of the package is the identity, because it is what the
/// advisory and the inventory both carry. The reference of the document is
/// not: two documents may call the same component different things, and
/// the graph would end up with two components that are one.
pub fn component_hrn(tenant: &Hrn, name: &str) -> Hrn {
    Hrn::with_region(
        tenant.partition().to_string(),
        tenant.service().to_string(),
        tenant.region().to_string(),
        tenant.account_id().to_string(),
        "Component".to_string(),
        sanitize(name),
    )
}

/// The identity of a vulnerability in the graph.
pub fn vulnerability_hrn(tenant: &Hrn, advisory_id: &str) -> Hrn {
    Hrn::with_region(
        tenant.partition().to_string(),
        tenant.service().to_string(),
        tenant.region().to_string(),
        tenant.account_id().to_string(),
        "Vulnerability".to_string(),
        sanitize(advisory_id),
    )
}

/// The name of a node, with the characters an HRN does not take.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => character,
            _ => '-',
        })
        .collect()
}

/// The graph changes that an inventory and an advisory produce.
///
/// The inventory alone gives the components and the dependencies between
/// them. The advisory gives the relation with the vulnerability. And the
/// artefact that owns the inventory holds its direct components.
pub fn project(
    tenant: &Hrn,
    artifact: &Hrn,
    inventory: &Sbom,
    advisory: Option<&OsvAdvisory>,
    evidence_id: hodei_evidence::EvidenceId,
    observed_at: chrono::DateTime<chrono::Utc>,
) -> GraphChangeSet {
    let mut changes = GraphChangeSet::empty();

    // The components of the inventory.
    for component in &inventory.components {
        let id = component_hrn(tenant, &component.name);
        changes = changes.with_node(GraphNode::new(
            id.clone(),
            tenant.clone(),
            NodeKind::Component,
            observed_at,
        ));

        // The artefact holds the components it depends on directly.
        let direct = match &inventory.subject {
            Some(subject) => inventory.dependencies_of(&subject.reference),
            None => Vec::new(),
        };
        if direct
            .iter()
            .any(|reference| reference == &component.reference)
        {
            changes = changes.with_relation(GraphRelation::new(
                RelationType::Contains,
                artifact.clone(),
                id.clone(),
                tenant.clone(),
                evidence_id.clone(),
                observed_at,
            ));
        }
    }

    // What depends on what.
    for dependency in &inventory.dependencies {
        // The edges that start at the subject are the ones the artefact
        // holds its components with. They are not dependencies between
        // components, and writing them twice would count the dependency
        // twice in the walk.
        if inventory
            .subject
            .as_ref()
            .is_some_and(|subject| subject.reference == dependency.reference)
        {
            continue;
        }

        let Some(from) = inventory.component(&dependency.reference) else {
            continue;
        };
        for depends_on in &dependency.depends_on {
            let Some(to) = inventory.component(depends_on) else {
                continue;
            };
            changes = changes.with_relation(GraphRelation::new(
                RelationType::DependsOn,
                component_hrn(tenant, &from.name),
                component_hrn(tenant, &to.name),
                tenant.clone(),
                evidence_id.clone(),
                observed_at,
            ));
        }
    }

    // And the relation with the vulnerability.
    if let Some(advisory) = advisory {
        let vulnerability = vulnerability_hrn(tenant, &advisory.id);
        changes = changes.with_node(GraphNode::new(
            vulnerability.clone(),
            tenant.clone(),
            NodeKind::Vulnerability,
            observed_at,
        ));

        for affected in &advisory.affected {
            for component in &inventory.components {
                if !crate::ingestion::matches_package(
                    &affected.package.name,
                    &component.name,
                    affected.package.purl.as_deref(),
                ) {
                    continue;
                }
                changes = changes.with_relation(GraphRelation::new(
                    RelationType::AffectedBy,
                    component_hrn(tenant, &component.name),
                    vulnerability.clone(),
                    tenant.clone(),
                    evidence_id.clone(),
                    observed_at,
                ));
            }
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osv::read_osv;

    const ADVISORY: &str = r#"{
      "id": "GHSA-test-0020",
      "affected": [
        {"package": {"ecosystem": "Maven", "name": "org.example:lib-b"}}
      ]
    }"#;

    fn hrn(text: &str) -> Hrn {
        Hrn::from_string(text).expect("an HRN")
    }

    fn tenant() -> Hrn {
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
    }

    #[test]
    fn the_projection_names_the_components_and_the_relations() {
        let inventory = hodei_evidence::domain::standards::sbom::Sbom {
            format: hodei_evidence::domain::standards::sbom::SbomFormat::CycloneDx17,
            normalizer_version: "1.0.0".to_string(),
            subject: Some(hodei_evidence::domain::standards::sbom::SbomSubject {
                reference: "pkg:oci/web".to_string(),
                name: "web".to_string(),
                version: Some("1.0.0".to_string()),
                kind: hodei_evidence::domain::standards::sbom::ComponentKind::Application,
                identities: Vec::new(),
            }),
            components: vec![
                // The subject is also a component of the inventory.
                hodei_evidence::domain::standards::sbom::SbomComponent {
                    reference: "pkg:oci/web".to_string(),
                    name: "web".to_string(),
                    version: Some("1.0.0".to_string()),
                    kind: hodei_evidence::domain::standards::sbom::ComponentKind::Application,
                    identities: Vec::new(),
                },
                hodei_evidence::domain::standards::sbom::SbomComponent {
                    reference: "lib-a".to_string(),
                    name: "org.example:lib-a".to_string(),
                    version: Some("1.0.0".to_string()),
                    kind: hodei_evidence::domain::standards::sbom::ComponentKind::Library,
                    identities: Vec::new(),
                },
                hodei_evidence::domain::standards::sbom::SbomComponent {
                    reference: "lib-b".to_string(),
                    name: "org.example:lib-b".to_string(),
                    version: Some("2.0.0".to_string()),
                    kind: hodei_evidence::domain::standards::sbom::ComponentKind::Library,
                    identities: Vec::new(),
                },
            ],
            dependencies: vec![
                hodei_evidence::domain::standards::sbom::SbomDependency {
                    reference: "pkg:oci/web".to_string(),
                    depends_on: vec!["lib-a".to_string()],
                },
                hodei_evidence::domain::standards::sbom::SbomDependency {
                    reference: "lib-a".to_string(),
                    depends_on: vec!["lib-b".to_string()],
                },
            ],
            source_digest: hodei_evidence::ContentDigest::of_bytes(b"sbom"),
            document_timestamp: None,
        };

        let advisory = read_osv(ADVISORY).expect("the document is read");
        let artifact = hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0");
        let changes = project(
            &tenant(),
            &artifact,
            &inventory,
            Some(&advisory),
            hodei_evidence::EvidenceId::new("ev-sbom"),
            chrono::Utc::now(),
        );

        assert_eq!(
            changes.nodes.len(),
            4,
            "the subject, two components and the vulnerability"
        );
        assert_eq!(
            changes.relations.len(),
            3,
            "contains, depends_on and affected_by"
        );

        let kinds: Vec<RelationType> = changes
            .relations
            .iter()
            .map(|relation| relation.relation.clone())
            .collect();
        assert!(kinds.contains(&RelationType::Contains));
        assert!(kinds.contains(&RelationType::DependsOn));
        assert!(kinds.contains(&RelationType::AffectedBy));
    }
}
