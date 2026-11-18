//! The risk facts of ONE subject, read from the materialized impact view.
//!
//! The promotion decision asks a question the capsule cannot answer: what
//! does the risk graph say reaches THIS artifact, as of which version of
//! the ledger? The impact view (HSP-42..46) already materializes exactly
//! that relation -- advisory to subject, with the path that proves it --
//! and this module reads it from the subject's side without inventing a
//! second evaluator.
//!
//! The contract that makes the reading honest:
//!
//! * A fact is `established` only when the account's view is complete up
//!   to the version the ledger has said AND every advisory reaching the
//!   subject has a known severity. An empty answer under a current view
//!   is a VERIFIED ZERO; anything else is UNKNOWN, and UNKNOWN is not
//!   zero.
//! * The reading is scoped to one account: a subject that does not belong
//!   to the tenant asked for produces no facts and leaks no information.
//! * The version the answer is complete up to travels WITH the facts: a
//!   decision that does not say which version of the risk it measured
//!   cannot be reproduced against the feed it actually saw.

use std::collections::BTreeMap;
use std::sync::Arc;

use kernel::Hrn;
use surrealdb::Connection;

use crate::impact_view::{ImpactRow, SurrealImpactView};

/// How bad an advisory is, as the catalog says.
///
/// The severity is not computed here: it is a fact about the advisory that
/// a feed provides, and this crate only asks for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvisorySeverity {
    /// The severity that blocks a promotion by itself.
    Critical,
    /// Any other severity the catalog knows.
    Other,
}

/// What the catalog knows about the advisories of the graph.
///
/// `None` is an answer and must not be smoothed away: an advisory whose
/// severity nobody knows makes the risk of the subject INCOMPLETE, and a
/// policy that requires the risk must not be granted on it.
pub trait AdvisoryCatalog: Send + Sync {
    /// The severity of one advisory, when the catalog has it.
    fn severity_of(&self, advisory: &Hrn) -> Option<AdvisorySeverity>;
}

/// A catalog that knows nothing: the shape of the wiring is real, and
/// every answer is honestly UNKNOWN until a feed fills it.
pub struct EmptyCatalog;

impl AdvisoryCatalog for EmptyCatalog {
    fn severity_of(&self, _advisory: &Hrn) -> Option<AdvisorySeverity> {
        None
    }
}

/// A catalog held in memory: what a feed or a test has said, keyed by the
/// canonical HRN of the advisory.
#[derive(Debug, Default, Clone)]
pub struct MemoryCatalog {
    severities: BTreeMap<String, AdvisorySeverity>,
}

impl MemoryCatalog {
    /// A catalog with the given severities.
    pub fn new(severities: impl IntoIterator<Item = (String, AdvisorySeverity)>) -> Self {
        Self {
            severities: severities.into_iter().collect(),
        }
    }
}

impl AdvisoryCatalog for MemoryCatalog {
    fn severity_of(&self, advisory: &Hrn) -> Option<AdvisorySeverity> {
        self.severities.get(&advisory.to_string()).copied()
    }
}

/// The risk facts of one subject, at one version of the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectFacts {
    /// Critical advisories the graph says the subject has, whether or not
    /// a path reaches them from the artifact itself.
    pub critical_known: u32,
    /// Critical advisories that REACH the subject through a supported
    /// path: the rows of the impact view are exactly the reachability.
    pub critical_reachable: u32,
    /// How many advisories reach the subject at all.
    pub advisories_reaching: u32,
    /// The version of the ledger the answer is complete up to.
    pub version: u64,
    /// Whether these facts were established: a current view and a known
    /// severity for everything that reaches the subject.
    pub established: bool,
}

impl SubjectFacts {
    /// The facts of a subject nobody could measure: nothing is known, and
    /// the version says so by being zero.
    pub fn unknown() -> Self {
        Self {
            critical_known: 0,
            critical_reachable: 0,
            advisories_reaching: 0,
            version: 0,
            established: false,
        }
    }
}

/// Why reading the facts failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskFactsError {
    /// The store could not answer.
    Unavailable(String),
}

impl std::fmt::Display for RiskFactsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(reason) => {
                write!(formatter, "the risk facts are unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for RiskFactsError {}

/// The read-only port the promotion decision uses to ask for risk.
#[async_trait::async_trait]
pub trait RiskFactsReader: Send + Sync {
    /// The facts of one subject of one account.
    ///
    /// A reader that cannot establish the facts says so; it never answers
    /// with a zero it did not verify.
    async fn risk_facts(&self, tenant: &Hrn, subject: &Hrn)
    -> Result<SubjectFacts, RiskFactsError>;
}

/// The facts built from what the view answered, with no database around.
///
/// Split from the reader so the judgement -- when is a zero verified, and
/// when is everything UNKNOWN -- is testable as the pure function it is.
pub fn facts_from(
    rows: &[ImpactRow],
    view_is_current: bool,
    said: u64,
    catalog: &dyn AdvisoryCatalog,
) -> SubjectFacts {
    let mut critical_known = 0;
    let mut critical_reachable = 0;
    let mut incomplete = false;
    for row in rows {
        let Some(advisory) = Hrn::from_string(&row.advisory) else {
            // A row whose advisory is not a canonical HRN is a row whose
            // severity can never be looked up: the answer is incomplete.
            incomplete = true;
            continue;
        };
        match catalog.severity_of(&advisory) {
            Some(AdvisorySeverity::Critical) => {
                critical_known += 1;
                critical_reachable += 1;
            }
            Some(AdvisorySeverity::Other) => {}
            None => incomplete = true,
        }
    }
    SubjectFacts {
        critical_known,
        critical_reachable,
        advisories_reaching: rows.len() as u32,
        version: said,
        established: view_is_current && !incomplete,
    }
}

/// The facts of a subject, read from the materialized impact view.
pub struct SurrealRiskFacts<C: Connection> {
    view: SurrealImpactView<C>,
    catalog: Arc<dyn AdvisoryCatalog>,
}

impl<C: Connection> SurrealRiskFacts<C> {
    /// The reader over the view and the catalog of severities.
    pub fn new(view: SurrealImpactView<C>, catalog: Arc<dyn AdvisoryCatalog>) -> Self {
        Self { view, catalog }
    }
}

#[async_trait::async_trait]
impl<C: Connection> RiskFactsReader for SurrealRiskFacts<C> {
    async fn risk_facts(
        &self,
        tenant: &Hrn,
        subject: &Hrn,
    ) -> Result<SubjectFacts, RiskFactsError> {
        // A subject of another account produces no facts and no
        // information: the answer is the same unknown any stranger gets.
        if subject.account_id() != tenant.account_id() {
            return Ok(SubjectFacts::unknown());
        }
        let said = self
            .view
            .said_version(tenant)
            .await
            .map_err(RiskFactsError::Unavailable)?;
        let rows = self
            .view
            .rows_reaching(tenant, subject)
            .await
            .map_err(RiskFactsError::Unavailable)?;
        let view_is_current = self
            .view
            .is_current(tenant)
            .await
            .map_err(RiskFactsError::Unavailable)?;
        Ok(facts_from(
            &rows,
            view_is_current,
            said,
            self.catalog.as_ref(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn tenant() -> String {
        "acme".to_string()
    }

    fn row(advisory: &str, subject: &str, depth: usize) -> ImpactRow {
        ImpactRow {
            tenant: tenant(),
            advisory: advisory.to_string(),
            subject: subject.to_string(),
            kind: "artifact".to_string(),
            depth,
            updated_at: Utc::now(),
        }
    }

    fn advisory(id: &str) -> String {
        format!("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/{id}")
    }

    fn artifact(id: &str) -> String {
        format!("hrn:hodei:artifacts:eu-west-1:acme:Artifact/{id}")
    }

    #[test]
    fn an_empty_answer_under_a_current_view_is_a_verified_zero() {
        let facts = facts_from(&[], true, 7, &EmptyCatalog);
        assert!(
            facts.established,
            "a current view with nothing reaching is zero, not UNKNOWN"
        );
        assert_eq!(facts.critical_reachable, 0);
        assert_eq!(facts.version, 7);
    }

    #[test]
    fn a_lagging_view_establishes_nothing_even_with_rows() {
        let rows = [row(&advisory("GHSA-x"), &artifact("app"), 1)];
        let facts = facts_from(&rows, false, 9, &EmptyCatalog);
        assert!(!facts.established);
        assert_eq!(
            facts.version, 9,
            "the lagging version still travels with the facts"
        );
    }

    #[test]
    fn a_reaching_critical_is_counted_as_reachable() {
        let catalog = MemoryCatalog::new([(advisory("GHSA-c"), AdvisorySeverity::Critical)]);
        let rows = [row(&advisory("GHSA-c"), &artifact("app"), 0)];
        let facts = facts_from(&rows, true, 3, &catalog);
        assert!(facts.established);
        assert_eq!(facts.critical_known, 1);
        assert_eq!(facts.critical_reachable, 1);
    }

    #[test]
    fn an_advisory_without_severity_makes_the_answer_incomplete() {
        let catalog = MemoryCatalog::new([(advisory("GHSA-known"), AdvisorySeverity::Other)]);
        let rows = [
            row(&advisory("GHSA-known"), &artifact("app"), 2),
            row(&advisory("GHSA-unknown"), &artifact("app"), 1),
        ];
        let facts = facts_from(&rows, true, 4, &catalog);
        assert!(
            !facts.established,
            "UNKNOWN severity is not smoothed into a zero"
        );
        assert_eq!(facts.critical_reachable, 0);
        assert_eq!(facts.advisories_reaching, 2);
    }

    #[test]
    fn a_row_with_a_non_canonical_advisory_is_incomplete_not_silently_zero() {
        let rows = [row("GHSA-raw-id", &artifact("app"), 0)];
        let facts = facts_from(&rows, true, 2, &EmptyCatalog);
        assert!(!facts.established);
    }

    #[test]
    fn the_unknown_facts_carry_no_version_claim() {
        let facts = SubjectFacts::unknown();
        assert!(!facts.established);
        assert_eq!(facts.version, 0);
    }

    #[tokio::test]
    async fn a_subject_of_another_account_produces_no_facts_and_no_information() {
        use hodei_evidence_storage::SurrealGraphStore;
        use hodei_evidence_storage::surreal::delta::SurrealRelationDeltaLedger;

        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .expect("the in-memory database opens");
        db.use_ns("hodei")
            .use_db("risk")
            .await
            .expect("the namespace");
        let db = Arc::new(db);
        let reader = SurrealRiskFacts::new(
            SurrealImpactView::new(
                Arc::new(SurrealGraphStore::new(db.clone())),
                Arc::new(SurrealRelationDeltaLedger::new(db)),
            ),
            Arc::new(EmptyCatalog),
        );
        let tenant =
            Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:artifact/acme/app-sha256-aa")
                .expect("the tenant hrn parses");
        let foreign =
            Hrn::from_string("hrn:hodei:artifacts:eu-west-1:other:artifact/other/app-sha256-bb")
                .expect("the foreign hrn parses");
        let facts = reader
            .risk_facts(&tenant, &foreign)
            .await
            .expect("the stranger is answered");
        assert!(
            !facts.established,
            "a stranger is answered with the same unknown anyone else gets"
        );
        assert_eq!(
            facts.version, 0,
            "no version of the tenant's feed is claimed"
        );
        assert_eq!(facts.advisories_reaching, 0, "no row of the tenant leaks");
    }
}
