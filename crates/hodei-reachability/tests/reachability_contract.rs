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

/// El punto de entrada de la aplicacion.
fn entry() -> CodeIdentity {
    identity("app", "src/main.rs", "main")
}

/// La funcion de la aplicacion que llama.
fn caller() -> CodeIdentity {
    identity("app", "src/app.rs", "run")
}

/// El simbolo vulnerable de la libreria.
fn vulnerable() -> CodeIdentity {
    identity("lib", "src/vuln.rs", "vuln_fn")
}

/// Un simbolo que no existe en el camino.
fn absent() -> CodeIdentity {
    identity("lib", "src/vuln.rs", "other_fn")
}

// ───────────────────────────── AC1 ─────────────────────────────

/// Existe un contrato `ReachabilityEvidence` con su cobertura y sus
/// limites, y no presupone ningun analizador concreto.
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

    // La cobertura dice QUE se cubrio.
    assert_eq!(evidence.coverage.source_revision, "rev-1");
    assert!(evidence.coverage.complete);
    assert_eq!(evidence.coverage.modules.len(), 2);

    // Y los limites dicen QUE NO se pudo resolver.
    assert_eq!(evidence.limitations.len(), 1);
    assert_eq!(evidence.limitations[0].as_str(), "dynamic_dispatch");

    // Y el contrato NO presupone ningun analizador: el analizador es un
    // dato de la evidencia, no una parte del contrato.
    assert_eq!(evidence.analyzer.name, "hodei-local");
    assert_eq!(evidence.analyzer.version, "1.0.0");

    // Y el texto del contrato lo dice, donde el codigo esta.
    let source = include_str!("../src/lib.rs");
    assert!(
        source.contains("no presupone ningun analizador"),
        "the contract says that it does not presuppose an analyser"
    );
}

// ───────────────────────────── AC2 ─────────────────────────────

/// La identidad del codigo distingue modulo, fichero y simbolo, y se puede
/// emparejar con un simbolo vulnerable.
#[test]
fn the_code_identity_tells_module_file_and_symbol_apart() {
    let one = identity("app", "src/main.rs", "parse");
    let other_module = identity("other", "src/main.rs", "parse");
    let other_file = identity("app", "src/other.rs", "parse");
    let other_symbol = identity("app", "src/main.rs", "other");

    assert_ne!(one, other_module);
    assert_ne!(one, other_file);
    assert_ne!(one, other_symbol);

    // Y se puede emparejar con un simbolo vulnerable.
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

/// La evidencia de alcanzabilidad se ingiere con sus aristas de llamada y
/// se puede consultar por simbolo.
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

    // Y se consulta por simbolo.
    let found = store
        .evidence_of("rev-1", &vulnerable())
        .await
        .expect("the evidence is read");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].call_edges.len(), 2, "with its call edges");

    // Y el estado se deriva de lo que hay.
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

/// El estado de alcanzabilidad distingue alcanzable, no alcanzable y
/// desconocido, y `unknown` no se confunde con `no alcanzable`.
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

    // Alcanzable: hay un camino.
    let reachability = derive(&complete, &vulnerable());
    assert_eq!(reachability.state, ReachabilityState::Reachable);
    assert!(reachability.is_reachable());

    // No alcanzable: el analisis fue completo y no hay camino.
    let reachability = derive(&complete, &absent());
    assert_eq!(reachability.state, ReachabilityState::Unreachable);
    assert!(reachability.state.is_proof_of_absence());

    // Desconocido: el analisis NO fue completo.
    let mut partial = complete.clone();
    partial.coverage = Coverage::partial("rev-1", vec!["app".to_string()]);
    let reachability = derive(&partial, &absent());
    assert_eq!(reachability.state, ReachabilityState::Unknown);
    assert!(
        !reachability.state.is_proof_of_absence(),
        "and unknown is never confused with unreachable"
    );
    assert!(reachability.reason.contains("not a proof"));

    // Y desconocido tambien cuando el analizador no pudo resolver algo.
    let limited = complete.with_limitation(Limitation::DynamicDispatch);
    let reachability = derive(&limited, &absent());
    assert_eq!(reachability.state, ReachabilityState::Unknown);
    assert!(reachability.reason.contains("not a proof"));

    // Y el camino gana sobre la limitacion: el simbolo se alcanza.
    let reachability = derive(&limited, &vulnerable());
    assert_eq!(reachability.state, ReachabilityState::Reachable);
}

// ───────────────────────────── AC5 ─────────────────────────────

/// Un proveedor local acotado demuestra el recorrido completo, y el
/// SPIKE-007 deja escrita su conclusion.
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

    // Y el recorrido completo: de la peticion a la respuesta.
    let reachability = derive(&evidence, &vulnerable());
    assert_eq!(reachability.state, ReachabilityState::Reachable);
    assert_eq!(reachability.call_path.len(), 3);
    assert_eq!(reachability.evidence_id, "ev-2");

    // Y la conclusion del SPIKE-007 esta escrita, con su regla.
    let source = include_str!("../src/lib.rs");
    assert!(
        source.contains("UNKNOWN"),
        "the rule is written where the code is"
    );
}

/// Y dos versiones del analizador dan dos evidencias, y las dos se
/// conservan.
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

    // Y la derivacion dice QUE analizador y QUE version produjo el
    // estado.
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
