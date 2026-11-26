//! The contract of the reachability.
//!
//! Five things are promised and five things are checked:
//!
//! - There is a `ReachabilityEvidence` contract with its coverage and its
//!   limits, and it does not presuppose any analyser.
//! - The identity of the code tells module, file and symbol apart, and it
//!   can be matched with a vulnerable symbol.
//! - The evidence of reachability is ingested with its call edges and it
//!   can be consulted by symbol.
//! - The state of reachability tells reachable, unreachable and unknown
//!   apart, and unknown is never confused with unreachable.
//! - A local, bounded provider demonstrates the whole chain, and the
//!   conclusion of SPIKE-007 is written down.

use chrono::{Duration, Utc};
use hodei_reachability::{
    AnalysisRequest, AnalyzerIdentity, CallEdge, CodeIdentity, Coverage, InMemoryReachabilityStore,
    Limitation, LocalProvider, ReachabilityEvidence, ReachabilityProvider, ReachabilityState,
    ReachabilityStore, VulnerableSymbol, derive, evidence_of,
};

fn identity(module: &str, file: &str, symbol: &str) -> CodeIdentity {
    CodeIdentity::new(module, file, symbol)
}

/// The application's entry point.
fn entry() -> CodeIdentity {
    identity("app", "src/main.rs", "main")
}

/// The application function that calls.
fn caller() -> CodeIdentity {
    identity("app", "src/app.rs", "run")
}

/// The library's vulnerable symbol.
fn vulnerable() -> CodeIdentity {
    identity("lib", "src/vuln.rs", "vuln_fn")
}

/// A symbol that does not exist on the path.
fn absent() -> CodeIdentity {
    identity("lib", "src/vuln.rs", "other_fn")
}

// ───────────────────────────── AC1 ─────────────────────────────

/// There is a `ReachabilityEvidence` contract with its coverage and its
/// limits, and it does not presuppose any concrete analyser.
#[test]
fn the_contract_carries_its_coverage_and_its_limits() {
    let evidence = ReachabilityEvidence::new(
        "ev-1",
        AnalyzerIdentity::new("hodei-local", "1.0.0"),
        Coverage::complete("rev-1", vec!["app".to_string(), "lib".to_string()]),
        vec![entry()],
        vec![CallEdge {
            from: entry(),
            to: vulnerable(),
        }],
        Utc::now(),
    )
    .with_limitation(Limitation::DynamicDispatch);

    // The coverage says WHAT was covered.
    assert_eq!(evidence.coverage.source_revision, "rev-1");
    assert!(evidence.coverage.complete);
    assert_eq!(evidence.coverage.modules.len(), 2);

    // And the limits say WHAT could NOT be resolved.
    assert_eq!(evidence.limitations.len(), 1);
    assert_eq!(evidence.limitations[0].as_str(), "dynamic_dispatch");

    // And the contract does NOT presuppose any analyser: the analyser is a
    // piece of data of the evidence, not a part of the contract.
    assert_eq!(evidence.analyzer.name, "hodei-local");
    assert_eq!(evidence.analyzer.version, "1.0.0");

    // And the contract text says so, where the code is.
    let source = include_str!("../src/lib.rs");
    assert!(
        source.contains("no presupone ningun analizador"),
        "the contract says that it does not presuppose an analyser"
    );
}

// ───────────────────────────── AC2 ─────────────────────────────

/// The identity of the code tells module, file and symbol apart, and can be
/// matched with a vulnerable symbol.
#[test]
fn the_code_identity_tells_module_file_and_symbol_apart() {
    let one = identity("app", "src/main.rs", "parse");
    let other_module = identity("other", "src/main.rs", "parse");
    let other_file = identity("app", "src/other.rs", "parse");
    let other_symbol = identity("app", "src/main.rs", "other");

    assert_ne!(one, other_module);
    assert_ne!(one, other_file);
    assert_ne!(one, other_symbol);

    // And it can be matched with a vulnerable symbol.
    let symbol = VulnerableSymbol::new(
        "org.example:lib",
        Some("2.0.0".to_string()),
        identity("lib", "src/vuln.rs", "vuln_fn"),
    );
    assert_eq!(symbol.identity, vulnerable());
    assert_eq!(symbol.package, "org.example:lib");
    assert_eq!(symbol.version.as_deref(), Some("2.0.0"));
}

// ───────────────────────────── AC3 ─────────────────────────────

/// The evidence of reachability is ingested with its call edges and
/// can be consulted by symbol.
#[tokio::test]
async fn the_evidence_is_ingested_with_its_call_edges_and_consulted_by_symbol() {
    let store = InMemoryReachabilityStore::empty();
    assert!(store.is_empty());

    let evidence = ReachabilityEvidence::new(
        "ev-1",
        AnalyzerIdentity::new("hodei-local", "1.0.0"),
        Coverage::complete("rev-1", vec!["app".to_string(), "lib".to_string()]),
        vec![entry()],
        vec![
            CallEdge {
                from: entry(),
                to: caller(),
            },
            CallEdge {
                from: caller(),
                to: vulnerable(),
            },
        ],
        Utc::now(),
    );
    store.put(&evidence).await.expect("the evidence is written");
    assert_eq!(store.len(), 1, "and it is there with its call edges");

    // And it is consulted by symbol.
    let found = store
        .evidence_of("rev-1", &vulnerable())
        .await
        .expect("the evidence is read");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].call_edges.len(), 2, "with its call edges");

    // And the state is derived from what is there.
    let reachability = store
        .reachability_of(
            "rev-1",
            &VulnerableSymbol::new("org.example:lib", Some("2.0.0".to_string()), vulnerable()),
        )
        .expect("the reachability is derived")
        .expect("there is one");
    assert_eq!(reachability.state, ReachabilityState::Reachable);
    assert_eq!(reachability.call_path.len(), 3);
    assert_eq!(reachability.analyzer.version, "1.0.0");
}

// ───────────────────────────── AC4 ─────────────────────────────

/// The state of reachability tells reachable, unreachable and
/// unknown apart, and `unknown` is not confused with `unreachable`.
#[test]
fn the_state_tells_the_three_apart_and_unknown_is_not_unreachable() {
    let complete = ReachabilityEvidence::new(
        "ev-1",
        AnalyzerIdentity::new("hodei-local", "1.0.0"),
        Coverage::complete("rev-1", vec!["app".to_string()]),
        vec![entry()],
        vec![CallEdge {
            from: entry(),
            to: vulnerable(),
        }],
        Utc::now(),
    );

    // Reachable: there is a path.
    let reachability = derive(&complete, &vulnerable());
    assert_eq!(reachability.state, ReachabilityState::Reachable);
    assert!(reachability.is_reachable());

    // Unreachable: the analysis was complete and there is no path.
    let reachability = derive(&complete, &absent());
    assert_eq!(reachability.state, ReachabilityState::Unreachable);
    assert!(reachability.state.is_proof_of_absence());

    // Unknown: the analysis was NOT complete.
    let mut partial = complete.clone();
    partial.coverage = Coverage::partial("rev-1", vec!["app".to_string()]);
    let reachability = derive(&partial, &absent());
    assert_eq!(reachability.state, ReachabilityState::Unknown);
    assert!(
        !reachability.state.is_proof_of_absence(),
        "and unknown is never confused with unreachable"
    );
    assert!(reachability.reason.contains("not a proof"));

    // And unknown also when the analyser could not resolve something.
    let limited = complete.with_limitation(Limitation::DynamicDispatch);
    let reachability = derive(&limited, &absent());
    assert_eq!(reachability.state, ReachabilityState::Unknown);
    assert!(reachability.reason.contains("not a proof"));

    // And the path wins over the limitation: the symbol is reached.
    let reachability = derive(&limited, &vulnerable());
    assert_eq!(reachability.state, ReachabilityState::Reachable);
}

// ───────────────────────────── AC5 ─────────────────────────────

/// A local, bounded provider demonstrates the whole chain, and
/// SPIKE-007 leaves its conclusion written down.
#[test]
fn a_local_bounded_provider_demonstrates_the_whole_chain() {
    let provider = LocalProvider::new(
        AnalyzerIdentity::new("hodei-local", "1.0.0"),
        Coverage::complete("rev-1", vec!["app".to_string(), "lib".to_string()]),
        vec![entry()],
        vec![
            CallEdge {
                from: entry(),
                to: caller(),
            },
            CallEdge {
                from: caller(),
                to: vulnerable(),
            },
        ],
    );

    let request = AnalysisRequest {
        source_revision: "rev-2".to_string(),
        symbols: vec![vulnerable()],
    };
    let result = provider.analyze(&request);
    let evidence = evidence_of("ev-2", result, Utc::now());

    assert_eq!(evidence.coverage.source_revision, "rev-2");
    assert_eq!(evidence.coverage.symbols.len(), 1);
    assert_eq!(evidence.analyzer.name, "hodei-local");

    // And the whole chain: from the request to the response.
    let reachability = derive(&evidence, &vulnerable());
    assert_eq!(reachability.state, ReachabilityState::Reachable);
    assert_eq!(reachability.call_path.len(), 3);
    assert_eq!(reachability.evidence_id, "ev-2");

    // And the conclusion of SPIKE-007 is written down, with its rule.
    let source = include_str!("../src/lib.rs");
    assert!(
        source.contains("UNKNOWN"),
        "the rule is written where the code is"
    );
}

/// And two versions of the analyser give two pieces of evidence, and both
/// are kept.
#[tokio::test]
async fn two_analyser_versions_are_both_preserved() {
    let store = InMemoryReachabilityStore::empty();

    let first = ReachabilityEvidence::new(
        "ev-1",
        AnalyzerIdentity::new("hodei-local", "1.0.0"),
        Coverage::complete("rev-1", vec!["app".to_string()]),
        vec![entry()],
        vec![CallEdge {
            from: entry(),
            to: vulnerable(),
        }],
        Utc::now(),
    );
    store.put(&first).await.expect("written");

    let second = ReachabilityEvidence::new(
        "ev-2",
        AnalyzerIdentity::new("hodei-local", "2.0.0"),
        Coverage::complete("rev-1", vec!["app".to_string()]),
        vec![entry()],
        vec![],
        Utc::now() + Duration::hours(1),
    );
    store.put(&second).await.expect("written");

    let evidences = store.evidence_of_revision("rev-1").await.expect("read");
    assert_eq!(
        evidences.len(),
        2,
        "both revisions are preserved: a new analyser does not erase the old answer"
    );

    // And the derivation says WHICH analyser and WHICH version produced the
    // state.
    let reachability = store
        .reachability_of(
            "rev-1",
            &VulnerableSymbol::new("org.example:lib", None, vulnerable()),
        )
        .expect("derived")
        .expect("there is one");
    assert_eq!(
        reachability.analyzer.version, "2.0.0",
        "the newest one wins"
    );
    assert_eq!(
        reachability.state,
        ReachabilityState::Unreachable,
        "and the newest one is the one that says the state"
    );
}
