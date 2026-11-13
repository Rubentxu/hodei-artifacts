//! The traversal of the blast radius: from a vulnerability to the
//! deployments that run something it affects.
//!
//! The walk is `vulnerability -> component -> dependent -> artefact ->
//! deployment`, and the interesting thing about it is not the walk but
//! **the depth**. A dependency that the artefact has directly is a
//! different thing from a dependency it has through three others, and a
//! report that says "affected" without saying which is a report that
//! people stop reading.
//!
//! The depth is a property of the **path**, and not of the edge: the edge
//! from a component to the one it depends on does not know whether the
//! dependency is direct or transitive, and the traversal is what tells
//! them apart.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use hodei_evidence::domain::graph::RelationType;
use kernel::Hrn;

use crate::case::{BlastRadius, SubjectDepth};

/// One edge of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// Where it starts.
    pub from: Hrn,
    /// Where it ends.
    pub to: Hrn,
    /// What it says.
    pub relation: RelationType,
}

/// The path from an artefact to an affected component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffectedPath {
    /// The artefact that contains the dependency.
    pub artifact: Hrn,
    /// The component the vulnerability is about.
    pub component: Hrn,
    /// How far the dependency is.
    ///
    /// One is a direct dependency: the artefact contains the component
    /// itself. More than one is a transitive one.
    pub depth: usize,
    /// The deployments that run the artefact.
    pub deployments: Vec<Hrn>,
}

impl AffectedPath {
    /// Whether the dependency is direct.
    pub fn is_direct(&self) -> bool {
        self.depth == 1
    }

    /// Whether the dependency is transitive.
    pub fn is_transitive(&self) -> bool {
        self.depth > 1
    }
}

/// The traversal of the blast radius of a vulnerability.
///
/// The relations are given and the walk is over them: the traversal does
/// not go and look for anything, so what it returns is what the graph
/// says, and it is the same every time.
pub fn blast_radius(relations: &[Edge], vulnerability: &Hrn) -> BlastRadius {
    let paths = affected_paths(relations, vulnerability);

    let mut affected_subjects: BTreeSet<String> = BTreeSet::new();
    let mut affected_deployments: BTreeSet<String> = BTreeSet::new();
    let mut affected_components: BTreeSet<String> = BTreeSet::new();
    let mut depths: Vec<SubjectDepth> = Vec::new();

    for path in &paths {
        let subject = path.artifact.to_string();
        affected_subjects.insert(subject.clone());
        affected_components.insert(path.component.to_string());
        for deployment in &path.deployments {
            affected_deployments.insert(deployment.to_string());
        }
        // The depth of a subject is the shortest path that reaches it:
        // the shortest one is the one that says how exposed it is.
        match depths.iter_mut().find(|entry| entry.subject == subject) {
            Some(entry) => {
                entry.depth = entry.depth.min(path.depth);
                for deployment in &path.deployments {
                    let deployment = deployment.to_string();
                    if !entry.deployments.contains(&deployment) {
                        entry.deployments.push(deployment);
                    }
                }
            }
            None => depths.push(SubjectDepth {
                subject,
                depth: path.depth,
                deployments: path
                    .deployments
                    .iter()
                    .map(|deployment| deployment.to_string())
                    .collect(),
            }),
        }
    }

    BlastRadius {
        affected_subjects: affected_subjects.into_iter().collect(),
        affected_deployments: affected_deployments.into_iter().collect(),
        affected_components: affected_components.into_iter().collect(),
        depths,
    }
}

/// The paths from the artefacts to the components the vulnerability is
/// about.
///
/// The walk goes from the artefact towards the dependency: the artefact
/// contains a component, that component depends on others, and the
/// vulnerability is about one of them. The depth is the number of steps
/// that took.
pub fn affected_paths(relations: &[Edge], vulnerability: &Hrn) -> Vec<AffectedPath> {
    // The components the vulnerability is about.
    let affected_components: BTreeSet<String> = relations
        .iter()
        .filter(|edge| edge.relation == RelationType::AffectedBy && &edge.to == vulnerability)
        .map(|edge| edge.from.to_string())
        .collect();

    if affected_components.is_empty() {
        return Vec::new();
    }

    // What each component depends on, what contains it, and where it runs.
    let mut dependencies: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut containers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut deployments: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for edge in relations {
        match edge.relation {
            RelationType::DependsOn => dependencies
                .entry(edge.from.to_string())
                .or_default()
                .push(edge.to.to_string()),
            RelationType::Contains => containers
                .entry(edge.to.to_string())
                .or_default()
                .push(edge.from.to_string()),
            RelationType::DeployedAs => deployments
                .entry(edge.from.to_string())
                .or_default()
                .push(edge.to.to_string()),
            _ => {}
        }
    }

    let mut paths = Vec::new();

    for (component, artifacts) in &containers {
        // The walk goes from the component the artefact contains towards
        // the ones it depends on. The breadth-first walk finds the
        // shortest path, and the shortest path is the one that says how
        // far the dependency is.
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        queue.push_back((component.clone(), 1));

        let mut seen: BTreeSet<String> = BTreeSet::new();

        while let Some((current, depth)) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                // The walk does not go in circles, and a component that
                // was already visited is not visited again with another
                // depth.
                continue;
            }

            if affected_components.contains(&current) {
                for artifact in artifacts {
                    let mut path = AffectedPath {
                        artifact: Hrn::from_string(artifact).expect("an HRN"),
                        component: Hrn::from_string(&current).expect("an HRN"),
                        depth,
                        deployments: deployments
                            .get(artifact)
                            .map(|list| {
                                list.iter()
                                    .map(|hrn| Hrn::from_string(hrn).expect("an HRN"))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    };
                    path.deployments
                        .sort_by_key(|deployment| deployment.to_string());
                    paths.push(path);
                }
            }

            if let Some(dependencies_of) = dependencies.get(&current) {
                for dependency in dependencies_of {
                    queue.push_back((dependency.clone(), depth + 1));
                }
            }
        }
    }

    paths.sort_by(|left, right| {
        left.artifact
            .to_string()
            .cmp(&right.artifact.to_string())
            .then(left.component.to_string().cmp(&right.component.to_string()))
            .then(left.depth.cmp(&right.depth))
    });
    paths.dedup();
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hrn(text: &str) -> Hrn {
        Hrn::from_string(text).expect("an HRN")
    }

    /// App A contains P1, P1 depends on P2, and the vulnerability is about
    /// P2. App B contains P3, which is not affected.
    fn the_graph() -> Vec<Edge> {
        vec![
            Edge {
                from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/a"),
                to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p1"),
                relation: RelationType::Contains,
            },
            Edge {
                from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p1"),
                to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p2"),
                relation: RelationType::DependsOn,
            },
            Edge {
                from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p2"),
                to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/v"),
                relation: RelationType::AffectedBy,
            },
            Edge {
                from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/a"),
                to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Deployment/prod"),
                relation: RelationType::DeployedAs,
            },
            Edge {
                from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/b"),
                to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p3"),
                relation: RelationType::Contains,
            },
        ]
    }

    #[test]
    fn the_walk_finds_the_transitive_dependency_and_the_deployment() {
        let paths = affected_paths(
            &the_graph(),
            &hrn("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/v"),
        );
        assert_eq!(paths.len(), 1, "only A is affected, and not B");
        let path = &paths[0];
        assert_eq!(
            path.artifact,
            hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/a")
        );
        assert_eq!(
            path.component,
            hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p2")
        );
        assert_eq!(path.depth, 2, "and it is a transitive dependency");
        assert!(path.is_transitive());
        assert!(!path.is_direct());
        assert_eq!(
            path.deployments,
            vec![hrn("hrn:hodei:artifacts:eu-west-1:acme:Deployment/prod")]
        );
    }

    #[test]
    fn the_radius_names_the_artefacts_the_components_and_the_deployments() {
        let radius = blast_radius(
            &the_graph(),
            &hrn("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/v"),
        );
        assert_eq!(radius.affected_subjects.len(), 1);
        assert_eq!(radius.affected_components.len(), 1);
        assert_eq!(radius.affected_deployments.len(), 1);
    }

    #[test]
    fn a_direct_dependency_is_one_step_and_a_transitive_one_is_more() {
        let mut graph = the_graph();
        // A contains P2 directly as well.
        graph.push(Edge {
            from: hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/c"),
            to: hrn("hrn:hodei:artifacts:eu-west-1:acme:Component/p2"),
            relation: RelationType::Contains,
        });
        let paths = affected_paths(
            &graph,
            &hrn("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/v"),
        );
        let direct = paths
            .iter()
            .find(|path| path.artifact == hrn("hrn:hodei:artifacts:eu-west-1:acme:Artifact/c"))
            .expect("the direct one is there");
        assert_eq!(direct.depth, 1);
        assert!(direct.is_direct());
    }
}
