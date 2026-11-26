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

use crate::impact_view::ImpactRow;
use kernel::Hrn;

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

/// The facts of a subject, read from the Dgraph impact view.
///
/// The same judgement as the embedded reader (it reuses [`facts_from`]):
/// facts are `established` only when the account's view is complete up to
/// the version the ledger has said and every advisory reaching the
/// subject has a known severity. Anything else is UNKNOWN, never zero.
pub struct DgraphRiskFacts {
    view: std::sync::Arc<crate::dgraph_view::DgraphImpactView>,
    catalog: std::sync::Arc<dyn AdvisoryCatalog>,
}

impl DgraphRiskFacts {
    /// The reader over the view and the catalog of severities.
    pub fn new(
        view: std::sync::Arc<crate::dgraph_view::DgraphImpactView>,
        catalog: std::sync::Arc<dyn AdvisoryCatalog>,
    ) -> Self {
        Self { view, catalog }
    }
}

#[async_trait::async_trait]
impl RiskFactsReader for DgraphRiskFacts {
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
