//! The contract of the evidence of reachability.
//!
//! The contract does **not** presuppose any analyser. It says what an
//! evidence of reachability has to carry so that a reader can judge it:
//! who produced it, on which revision of the source, **what it covered**
//! and **what it could not resolve**.
//!
//! Those last two are the whole point. An evidence that says "not
//! reachable" is only worth something if the analysis covered
//! everything that could have reached it. An evidence that says
//! "unreachable" without saying what it covered is a guess, and Hodei
//! treats a guess as **unknown**.
//!
//! And the rule is absolute: **anything the analyser could not resolve
//! is UNKNOWN, never UNREACHABLE**. A dynamic dispatch that the analyser
//! did not follow is not a proof that the code is not reached; it is a
//! proof that nobody knows.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::identity::CodeIdentity;

/// The state of the reachability of a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ReachabilityState {
    /// There is a path from the entry point to the symbol.
    Reachable,
    /// There is no path, and the analysis covered everything that could
    /// have had one.
    Unreachable,
    /// Nobody knows.
    ///
    /// Either the analysis did not cover everything, or it found something
    /// it could not resolve. It is not a weaker "unreachable": it is a
    /// different answer.
    Unknown,
}

impl ReachabilityState {
    /// The name of the state.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reachable => "REACHABLE",
            Self::Unreachable => "UNREACHABLE",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// Whether the state is a proof that the symbol is not reached.
    pub fn is_proof_of_absence(&self) -> bool {
        matches!(self, Self::Unreachable)
    }
}

/// Who produced the evidence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnalyzerIdentity {
    /// The name of the analyser.
    pub name: String,
    /// The version of the analyser.
    ///
    /// It is part of the identity because the same source analysed by two
    /// versions can give two answers, and a reachability that does not
    /// name the version cannot be reproduced.
    pub version: String,
}

impl AnalyzerIdentity {
    /// The identity of an analyser.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// What the analysis covered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// The revision of the source that was analysed.
    pub source_revision: String,
    /// The modules the analysis covered.
    pub modules: Vec<String>,
    /// The symbols the analysis covered.
    pub symbols: Vec<CodeIdentity>,
    /// Whether the analysis covered **all** the code of the application.
    ///
    /// It is the difference between an `UNREACHABLE` that means something
    /// and one that does not.
    pub complete: bool,
}

impl Coverage {
    /// A coverage that is complete.
    pub fn complete(source_revision: impl Into<String>, modules: Vec<String>) -> Self {
        Self {
            source_revision: source_revision.into(),
            modules,
            symbols: Vec::new(),
            complete: true,
        }
    }

    /// A coverage that is partial.
    pub fn partial(source_revision: impl Into<String>, modules: Vec<String>) -> Self {
        Self {
            source_revision: source_revision.into(),
            modules,
            symbols: Vec::new(),
            complete: false,
        }
    }

    /// Whether a symbol was covered by the analysis.
    ///
    /// A complete coverage that names no symbols covered all of them: the
    /// document does not have to enumerate everything it looked at. A
    /// coverage that names symbols covered exactly those.
    pub fn covers(&self, symbol: &CodeIdentity) -> bool {
        if self.symbols.is_empty() {
            return self.complete;
        }
        self.symbols.iter().any(|covered| covered == symbol)
    }
}

/// What the analysis could not resolve.
///
/// Every one of these is a reason for the answer to be **UNKNOWN** and
/// not `UNREACHABLE`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Limitation {
    /// A call that is resolved at run time.
    DynamicDispatch,
    /// A symbol that is loaded at run time.
    Reflection,
    /// Code that the analysis could not read.
    UnparsedSource,
    /// A library whose body the analysis did not have.
    MissingLibraryBody,
    /// Something else, said in words.
    Other(String),
}

impl Limitation {
    /// The name of the limitation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DynamicDispatch => "dynamic_dispatch",
            Self::Reflection => "reflection",
            Self::UnparsedSource => "unparsed_source",
            Self::MissingLibraryBody => "missing_library_body",
            Self::Other(_) => "other",
        }
    }
}

/// One edge of the call graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallEdge {
    /// The symbol that calls.
    pub from: CodeIdentity,
    /// The symbol that is called.
    pub to: CodeIdentity,
}

/// The evidence of reachability.
///
/// It is the whole thing: who analysed what, on which revision, with which
/// coverage, with which limits, and with which call edges. The reachability
/// is **derived** from it, and it is derived the same way every time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityEvidence {
    /// The identifier of the evidence.
    pub id: String,
    /// The analyser that produced it.
    pub analyzer: AnalyzerIdentity,
    /// What it covered.
    pub coverage: Coverage,
    /// What it could not resolve.
    pub limitations: Vec<Limitation>,
    /// The entry points of the application.
    pub entry_points: Vec<CodeIdentity>,
    /// The edges of the call graph.
    pub call_edges: Vec<CallEdge>,
    /// When the analysis was made.
    pub analyzed_at: DateTime<Utc>,
}

impl ReachabilityEvidence {
    /// The evidence of an analysis.
    pub fn new(
        id: impl Into<String>,
        analyzer: AnalyzerIdentity,
        coverage: Coverage,
        entry_points: Vec<CodeIdentity>,
        call_edges: Vec<CallEdge>,
        analyzed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            analyzer,
            coverage,
            limitations: Vec::new(),
            entry_points,
            call_edges,
            analyzed_at,
        }
    }

    /// The evidence has a limitation.
    pub fn with_limitation(mut self, limitation: Limitation) -> Self {
        self.limitations.push(limitation);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_that_is_not_a_proof_is_not_a_proof() {
        assert!(ReachabilityState::Unreachable.is_proof_of_absence());
        assert!(!ReachabilityState::Unknown.is_proof_of_absence());
        assert!(!ReachabilityState::Reachable.is_proof_of_absence());
    }
}
