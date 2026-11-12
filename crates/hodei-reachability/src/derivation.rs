//! The derivation of the reachability.
//!
//! The derivation is a **pure function** of the evidence, and it is the
//! same every time: the same evidence gives the same answer, and the
//! answer says why.
//!
//! The rules, in order:
//!
//! 1. If there is a path from an entry point to the symbol, the symbol
//!    is **REACHABLE**, and the path is in the answer.
//! 2. If there is no path, but the analysis did not cover everything, or
//!    it found something it could not resolve, the symbol is **UNKNOWN**.
//!    Nobody knows, and nobody knowing is not a proof.
//! 3. Only when there is no path, the coverage was complete and there
//!    were no limitations is the symbol **UNREACHABLE**.
//!
//! The order matters. The first rule wins over the second, and the second
//! wins over the third: a symbol that is reachable is reachable even if
//! the analysis also found something it could not resolve elsewhere, and a
//! symbol whose analysis was incomplete is unknown even when no path was
//! found.

use serde::{Deserialize, Serialize};

use crate::evidence::{ReachabilityEvidence, ReachabilityState};
use crate::identity::CodeIdentity;

/// The reachability of a symbol, with the reason and the path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reachability {
    /// The state.
    pub state: ReachabilityState,
    /// The shortest path from an entry point to the symbol, when there is
    /// one.
    pub call_path: Vec<CodeIdentity>,
    /// Why the answer is what it is.
    pub reason: String,
    /// The identifier of the evidence the answer comes from.
    pub evidence_id: String,
    /// The analyser that produced the evidence.
    pub analyzer: crate::evidence::AnalyzerIdentity,
}

impl Reachability {
    /// Whether the symbol is reached.
    pub fn is_reachable(&self) -> bool {
        self.state == ReachabilityState::Reachable
    }
}

/// Derives the reachability of a symbol from the evidence.
///
/// The derivation reads the evidence and nothing else: it does not query,
/// it does not look for more, and it does not guess.
pub fn derive(evidence: &ReachabilityEvidence, symbol: &CodeIdentity) -> Reachability {
    // 1. The path.
    if let Some(path) = shortest_path(evidence, symbol) {
        return Reachability {
            state: ReachabilityState::Reachable,
            call_path: path,
            reason: format!(
                "there is a path from an entry point to {symbol} in the call graph of the analysis"
            ),
            evidence_id: evidence.id.clone(),
            analyzer: evidence.analyzer.clone(),
        };
    }

    // 2. Nobody knows, because the analysis did not cover everything or
    // it found something it could not resolve.
    if !evidence.coverage.complete {
        return Reachability {
            state: ReachabilityState::Unknown,
            call_path: Vec::new(),
            reason: format!(
                "the analysis of {} did not cover all the code, and no path was found; that is not a proof that the symbol is not reached",
                evidence.analyzer.name
            ),
            evidence_id: evidence.id.clone(),
            analyzer: evidence.analyzer.clone(),
        };
    }

    if let Some(limitation) = evidence.limitations.first() {
        return Reachability {
            state: ReachabilityState::Unknown,
            call_path: Vec::new(),
            reason: format!(
                "the analysis of {} could not resolve {}; that is not a proof that the symbol is not reached",
                evidence.analyzer.name,
                limitation.as_str()
            ),
            evidence_id: evidence.id.clone(),
            analyzer: evidence.analyzer.clone(),
        };
    }

    // 3. The analysis was complete, it resolved everything it saw, and
    // there is no path.
    Reachability {
        state: ReachabilityState::Unreachable,
        call_path: Vec::new(),
        reason: format!(
            "the analysis of {} covered all the code and resolved everything it saw, and there is no path from an entry point to {symbol}",
            evidence.analyzer.name
        ),
        evidence_id: evidence.id.clone(),
        analyzer: evidence.analyzer.clone(),
    }
}

/// The shortest path from an entry point to a symbol.
///
/// The walk is in breadth, so the first path that arrives is the shortest
/// one, and it is the same one every time.
pub fn shortest_path(
    evidence: &ReachabilityEvidence,
    symbol: &CodeIdentity,
) -> Option<Vec<CodeIdentity>> {
    use std::collections::{BTreeSet, VecDeque};

    let mut queue: VecDeque<(CodeIdentity, Vec<CodeIdentity>)> = VecDeque::new();
    for entry in &evidence.entry_points {
        queue.push_back((entry.clone(), vec![entry.clone()]));
    }

    let mut seen: BTreeSet<CodeIdentity> = BTreeSet::new();

    while let Some((current, path)) = queue.pop_front() {
        if !seen.insert(current.clone()) {
            continue;
        }
        if &current == symbol {
            return Some(path);
        }
        for edge in evidence
            .call_edges
            .iter()
            .filter(|edge| edge.from == current)
        {
            let mut next = path.clone();
            next.push(edge.to.clone());
            queue.push_back((edge.to.clone(), next));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{AnalyzerIdentity, Coverage, Limitation};
    use chrono::Utc;

    fn identity(module: &str, file: &str, symbol: &str) -> CodeIdentity {
        CodeIdentity::new(module, file, symbol)
    }

    fn the_evidence() -> ReachabilityEvidence {
        let entry = identity("app", "src/main.rs", "main");
        let caller = identity("app", "src/app.rs", "run");
        let vulnerable = identity("lib", "src/vuln.rs", "vuln_fn");
        ReachabilityEvidence::new(
            "ev-1",
            AnalyzerIdentity::new("hodei-local", "1.0.0"),
            Coverage::complete("rev-1", vec!["app".to_string(), "lib".to_string()]),
            vec![entry.clone()],
            vec![
                crate::evidence::CallEdge {
                    from: entry.clone(),
                    to: caller.clone(),
                },
                crate::evidence::CallEdge {
                    from: caller,
                    to: vulnerable,
                },
            ],
            Utc::now(),
        )
    }

    #[test]
    fn a_path_makes_the_symbol_reachable() {
        let evidence = the_evidence();
        let vulnerable = identity("lib", "src/vuln.rs", "vuln_fn");
        let reachability = derive(&evidence, &vulnerable);
        assert_eq!(reachability.state, ReachabilityState::Reachable);
        assert_eq!(reachability.call_path.len(), 3);
    }

    #[test]
    fn a_complete_analysis_with_no_path_proves_the_absence() {
        let evidence = the_evidence();
        let absent = identity("lib", "src/vuln.rs", "other_fn");
        let reachability = derive(&evidence, &absent);
        assert_eq!(reachability.state, ReachabilityState::Unreachable);
        assert!(reachability.call_path.is_empty());
    }

    #[test]
    fn an_incomplete_analysis_leaves_it_unknown_and_not_unreachable() {
        let mut evidence = the_evidence();
        evidence.coverage = Coverage::partial("rev-1", vec!["app".to_string()]);
        let absent = identity("lib", "src/vuln.rs", "other_fn");
        let reachability = derive(&evidence, &absent);
        assert_eq!(
            reachability.state,
            ReachabilityState::Unknown,
            "an incomplete analysis does not prove anything"
        );
    }

    #[test]
    fn a_limitation_leaves_it_unknown_and_not_unreachable() {
        let evidence = the_evidence().with_limitation(Limitation::DynamicDispatch);
        let absent = identity("lib", "src/vuln.rs", "other_fn");
        let reachability = derive(&evidence, &absent);
        assert_eq!(
            reachability.state,
            ReachabilityState::Unknown,
            "a dynamic dispatch that was not followed is not a proof"
        );
    }

    #[test]
    fn a_path_wins_over_a_limitation() {
        let evidence = the_evidence().with_limitation(Limitation::Reflection);
        let vulnerable = identity("lib", "src/vuln.rs", "vuln_fn");
        let reachability = derive(&evidence, &vulnerable);
        assert_eq!(
            reachability.state,
            ReachabilityState::Reachable,
            "the symbol is reached, whatever else the analysis could not resolve"
        );
    }
}
