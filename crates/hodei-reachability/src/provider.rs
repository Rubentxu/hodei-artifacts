//! The provider of the reachability: the port and a local, bounded one.
//!
//! Hodei does not build a universal analyser. It **understands and uses**
//! an evidence of reachability that somebody else produced, and the port
//! is what makes that possible: any analyser enters by an adapter.
//!
//! The local provider that lives here is **bounded on purpose**. It reads
//! a description of a small program --its entry points, its calls and its
//! limitations-- and it produces the evidence from it. It is not an
//! analyser of code; it is the demonstration that the chain works, and it
//! is honest about what it did.

use serde::{Deserialize, Serialize};

use crate::evidence::{AnalyzerIdentity, CallEdge, Coverage, Limitation, ReachabilityEvidence};
use crate::identity::CodeIdentity;

/// What a provider is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRequest {
    /// The revision of the source to analyse.
    pub source_revision: String,
    /// The symbols the analysis is about.
    pub symbols: Vec<CodeIdentity>,
}

/// What a provider answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisResult {
    /// The identity of the analyser.
    pub analyzer: AnalyzerIdentity,
    /// What it covered.
    pub coverage: Coverage,
    /// What it could not resolve.
    pub limitations: Vec<Limitation>,
    /// The entry points it found.
    pub entry_points: Vec<CodeIdentity>,
    /// The edges of the call graph.
    pub call_edges: Vec<CallEdge>,
}

/// The provider of the reachability.
///
/// The port is the whole of the contract with an analyser: it asks for an
/// analysis of a revision and it gets an evidence. It does not ask what
/// kind of analyser it is, and it does not care.
pub trait ReachabilityProvider {
    /// The identity of the provider.
    fn analyzer(&self) -> AnalyzerIdentity;

    /// Analyses a revision.
    fn analyze(&self, request: &AnalysisRequest) -> AnalysisResult;
}

/// The local, bounded provider.
///
/// It reads a program that was described to it and it produces the
/// evidence from that description. It is the demonstration that the chain
/// works from the source to the answer, and it does not pretend to be
/// anything more: what it cannot resolve is a limitation, and a limitation
/// makes the answer **unknown**.
#[derive(Debug, Clone)]
pub struct LocalProvider {
    analyzer: AnalyzerIdentity,
    coverage: Coverage,
    limitations: Vec<Limitation>,
    entry_points: Vec<CodeIdentity>,
    call_edges: Vec<CallEdge>,
}

impl LocalProvider {
    /// A provider over a described program.
    pub fn new(
        analyzer: AnalyzerIdentity,
        coverage: Coverage,
        entry_points: Vec<CodeIdentity>,
        call_edges: Vec<CallEdge>,
    ) -> Self {
        Self {
            analyzer,
            coverage,
            limitations: Vec::new(),
            entry_points,
            call_edges,
        }
    }

    /// The provider found something it could not resolve.
    pub fn with_limitation(mut self, limitation: Limitation) -> Self {
        self.limitations.push(limitation);
        self
    }
}

impl ReachabilityProvider for LocalProvider {
    fn analyzer(&self) -> AnalyzerIdentity {
        self.analyzer.clone()
    }

    fn analyze(&self, request: &AnalysisRequest) -> AnalysisResult {
        let mut coverage = self.coverage.clone();
        let _ = &request;
        coverage.source_revision = request.source_revision.clone();
        coverage.symbols = request.symbols.clone();

        AnalysisResult {
            analyzer: self.analyzer.clone(),
            coverage,
            limitations: self.limitations.clone(),
            entry_points: self.entry_points.clone(),
            call_edges: self.call_edges.clone(),
        }
    }
}

/// The evidence of an analysis, with the identifier it is kept under.
pub fn evidence_of(
    id: impl Into<String>,
    result: AnalysisResult,
    analyzed_at: chrono::DateTime<chrono::Utc>,
) -> ReachabilityEvidence {
    ReachabilityEvidence {
        id: id.into(),
        analyzer: result.analyzer,
        coverage: result.coverage,
        limitations: result.limitations,
        entry_points: result.entry_points,
        call_edges: result.call_edges,
        analyzed_at,
    }
}
