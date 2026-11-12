//! The scale model of reference.
//!
//! A measurement without a load says nothing: the same query can be fast on
//! a toy graph and unusable on a real one, and without knowing which one is
//! being measured, the number cannot be compared with anything.
//!
//! So the load comes first. The model says how many accounts there are, how
//! many repositories each one has, how many revisions and builds each
//! repository has, and how many artefacts each build produces -- and, more
//! importantly, the **shape**: a graph where everything hangs from one
//! repository does not look like a graph that anybody has.
//!
//! The shape here is:
//!
//! - the accounts have their sizes spread out, not all the same;
//! - the repositories of an account are spread out too;
//! - the builds of a revision are spread out;
//! - every artefact belongs to a build, and every deployment to an
//!   artefact, so the graph is connected the way a real one is.
//!
//! # One source of truth
//!
//! The size of an account is computed once, in [`ScaleModel::shape`], and
//! both the count of the evidence and the journey of the account are read
//! from that shape. A model that computed the sizes twice would eventually
//! compute them differently, and the count would stop being the count.

use hodei_evidence::{EvidenceType, NodeKind, RelationClaim, RelationType};

/// The distribution of the sizes of the model.
///
/// It is a logarithmic spread on purpose: most accounts are small, a few
/// are large, and a model where everybody is the same size hides exactly
/// the case that breaks things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleModel {
    /// How many accounts there are.
    pub accounts: usize,
    /// The smallest account: repositories.
    pub min_repositories: usize,
    /// The largest account: repositories.
    pub max_repositories: usize,
    /// The smallest repository: revisions.
    pub min_revisions: usize,
    /// The largest repository: revisions.
    pub max_revisions: usize,
    /// The smallest revision: builds.
    pub min_builds: usize,
    /// The largest revision: builds.
    pub max_builds: usize,
    /// How many artefacts each build produces.
    pub artifacts_per_build: usize,
    /// How many deployments each artefact has.
    pub deployments_per_artifact: usize,
}

/// The shape of one account.
///
/// It says how many repositories the account has, how many revisions each
/// repository has, and how many builds each revision has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountShape {
    /// How many repositories the account has.
    pub repositories: usize,
    /// How many revisions each repository has, in order.
    pub revisions: Vec<usize>,
    /// How many builds each revision has, by repository and by revision.
    pub builds: Vec<Vec<usize>>,
}

impl AccountShape {
    /// How many pieces of evidence the account holds.
    pub fn evidence(&self, model: &ScaleModel) -> usize {
        let mut total = 0;
        for builds in &self.builds {
            for count in builds {
                // The piece of the revision itself.
                total += 1;
                // A piece per build and per artefact it produces.
                total += count * model.artifacts_per_build;
                // And a piece per deployment of every artefact.
                total += count * model.artifacts_per_build * model.deployments_per_artifact;
            }
        }
        total
    }
}

impl ScaleModel {
    /// A model small enough to run in a test, big enough to say something.
    pub fn small() -> Self {
        Self {
            accounts: 3,
            min_repositories: 2,
            max_repositories: 4,
            min_revisions: 2,
            max_revisions: 4,
            min_builds: 1,
            max_builds: 3,
            artifacts_per_build: 1,
            deployments_per_artifact: 1,
        }
    }

    /// The model of reference: the shape of a deployment that has been
    /// running for a while.
    pub fn reference() -> Self {
        Self {
            accounts: 20,
            min_repositories: 5,
            max_repositories: 60,
            min_revisions: 10,
            max_revisions: 120,
            min_builds: 1,
            max_builds: 12,
            artifacts_per_build: 2,
            deployments_per_artifact: 3,
        }
    }

    /// A model of the same shape but of another size.
    ///
    /// The shape is what makes the model representative, and the size is
    /// what makes it cheap or expensive to measure. A model that can only be
    /// measured at one size says nothing about how the cost grows, and the
    /// growth is the question the spike answers.
    pub fn of_size(accounts: usize, size: usize) -> Self {
        let model = Self::reference();
        // The size scales the accounts and the volumes of the shape, but
        // not the minimums: the shape of the smallest account is the shape
        // of every account.
        Self {
            accounts,
            min_repositories: model.min_repositories,
            max_repositories: model.max_repositories * size / 10,
            min_revisions: model.min_revisions,
            max_revisions: model.max_revisions * size / 10,
            min_builds: model.min_builds,
            max_builds: model.max_builds * size / 10,
            artifacts_per_build: model.artifacts_per_build,
            deployments_per_artifact: model.deployments_per_artifact,
        }
    }

    /// The shape of the account `index`.
    ///
    /// The shape is computed once and read by everything else, so the count
    /// of the evidence and the journey of the account cannot disagree.
    pub fn shape(&self, index: usize) -> AccountShape {
        let repositories = self.spread(
            self.min_repositories,
            self.max_repositories,
            index,
            self.accounts,
        );

        let mut revisions = Vec::with_capacity(repositories);
        let mut builds = Vec::with_capacity(repositories);
        for repository in 0..repositories {
            let count = self.spread(
                self.min_revisions,
                self.max_revisions,
                repository,
                repositories,
            );
            revisions.push(count);
            let mut of_repository = Vec::with_capacity(count);
            for revision in 0..count {
                of_repository.push(self.spread(self.min_builds, self.max_builds, revision, count));
            }
            builds.push(of_repository);
        }

        AccountShape {
            repositories,
            revisions,
            builds,
        }
    }

    /// How many pieces of evidence the model holds.
    pub fn expected_events(&self) -> usize {
        (0..self.accounts)
            .map(|index| self.shape(index).evidence(self))
            .sum()
    }

    /// The size of the item `index`, spread out over `count` items.
    ///
    /// The spread is logarithmic: the first item is the smallest, the last
    /// one is the largest, and the ones in between grow slowly. A model
    /// where the sizes grow in a straight line is a model where the biggest
    /// case is the only case anybody looks at.
    ///
    /// The count of the items is a parameter because the spread is applied
    /// at three levels -- accounts, repositories and revisions -- and each
    /// level has its own count.
    pub fn spread(&self, min: usize, max: usize, index: usize, count: usize) -> usize {
        if max <= min {
            return min;
        }
        if index >= count {
            return max;
        }
        // The spread goes from the smallest to the largest, and the ones in
        // between grow slowly: a geometric interpolation, so that the small
        // sizes are many and the big ones are few.
        let fraction = if count <= 1 {
            0.0
        } else {
            index as f64 / (count - 1) as f64
        };
        let value = (min as f64) * (max as f64 / min as f64).powf(fraction);
        (value.round() as usize).clamp(min, max)
    }

    /// The journey of one account, piece by piece.
    ///
    /// It is the same shape the UAT uses, with the same claims: the
    /// repository has revisions, the revision triggers the build, the build
    /// produces the artefact and the artefact is deployed.
    pub fn journey(
        &self,
        account: &str,
        account_index: usize,
    ) -> Vec<(String, Vec<u8>, EvidenceType, Option<RelationClaim>)> {
        let shape = self.shape(account_index);
        let mut pieces = Vec::new();

        // The identity of a piece carries the account it belongs to: the
        // accounts share the store, and a piece of one account that
        // carried the identity of a piece of another account would be
        // taken for a repetition of it.
        for repository_index in 0..shape.repositories {
            let repository = format!("repo-{account_index}-{repository_index}");
            let revisions = shape.revisions[repository_index];

            for revision_index in 0..revisions {
                let revision = format!("rev-{account_index}-{repository_index}-{revision_index}");
                pieces.push((
                    revision.clone(),
                    format!("the revision {revision}").into_bytes(),
                    EvidenceType::SourceMetadata,
                    Some(RelationClaim {
                        relation: RelationType::HasRevision,
                        from: hrn(account, "Repository", &repository),
                        to: hrn(account, "SourceRevision", &revision),
                        from_kind: NodeKind::Repository,
                        to_kind: NodeKind::SourceRevision,
                    }),
                ));

                let builds = shape.builds[repository_index][revision_index];
                for build_index in 0..builds {
                    let build = format!(
                        "build-{account_index}-{repository_index}-{revision_index}-{build_index}"
                    );

                    for artifact_index in 0..self.artifacts_per_build {
                        let artifact = format!("artifact-{build}-{artifact_index}");
                        // The identity of the piece names the artefact it
                        // speaks of: two pieces of the same build that
                        // claim different artefacts are two pieces, and
                        // the store keeps the one that arrives twice as
                        // the same piece. The measure of the scale model
                        // found it: 858 pieces of the reference load were
                        // taken for a repetition and never became facts.
                        pieces.push((
                            format!("build-{build}-artefact-{artifact_index}"),
                            format!("the build {build} produced {artifact}").into_bytes(),
                            EvidenceType::BuildMetadata,
                            Some(RelationClaim {
                                relation: RelationType::Produced,
                                from: hrn(account, "Build", &build),
                                to: hrn(account, "Artifact", &artifact),
                                from_kind: NodeKind::Build,
                                to_kind: NodeKind::Artifact,
                            }),
                        ));

                        for deployment_index in 0..self.deployments_per_artifact {
                            let deployment = format!("deployment-{artifact}-{deployment_index}");
                            pieces.push((
                                format!("deployment-{deployment}"),
                                format!("the deployment {deployment}").into_bytes(),
                                EvidenceType::DeploymentObservation,
                                Some(RelationClaim {
                                    relation: RelationType::DeployedAs,
                                    from: hrn(account, "Artifact", &artifact),
                                    to: hrn(account, "Deployment", &deployment),
                                    from_kind: NodeKind::Artifact,
                                    to_kind: NodeKind::Deployment,
                                }),
                            ));
                        }
                    }
                }
            }
        }

        pieces
    }
}

/// The identity of a node of the model.
fn hrn(account: &str, kind: &str, id: &str) -> kernel::Hrn {
    kernel::Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:{kind}/{id}"
    ))
    .expect("the identity of a node of the model is a valid HRN")
}
