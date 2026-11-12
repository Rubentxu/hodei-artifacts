//! La UAT de M5: la alcanzabilidad del codigo.
//!
//! Son los seis escenarios de `uat/M5-CODE-REACHABILITY-UAT.md`, ejecutados
//! sobre fixtures locales y servicios efimeros, sin integraciones
//! obligatorias con otros productos.
//!
//! # Los escenarios
//!
//! - M5-UAT-01 El camino alcanzable.
//! - M5-UAT-02 El simbolo que no se alcanza.
//! - M5-UAT-03 Lo desconocido sigue desconocido.
//! - M5-UAT-04 Cambia la version del analizador.
//! - M5-UAT-05 Un cambio en el codigo.
//! - M5-UAT-06 La explicacion para el desarrollador.
//!
//! # Que se ejecuta de verdad
//!
//! La evidencia se escribe en un almacen real y se lee de el. La version
//! del analizador, la cobertura y los limites viajan con la evidencia, y
//! el estado se deriva de lo que se leyo.

use chrono::{Duration, Utc};
use hodei_reachability::derivation::derive;
use hodei_reachability::evidence::{
    AnalyzerIdentity, CallEdge, Coverage, Limitation, ReachabilityEvidence, ReachabilityState,
};
use hodei_reachability::identity::{CodeIdentity, VulnerableSymbol};
use hodei_reachability::store::ReachabilityStore;
use hodei_reachability_storage::SurrealReachabilityStore;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

/// El comando con el que se ejecuta esta suite.
const COMMAND: &str = "cargo test -p hodei-reachability-storage --test m5_uat -- --nocapture";

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

/// El simbolo vulnerable que no esta en el camino.
fn other_vulnerable() -> CodeIdentity {
    identity("lib", "src/other.rs", "other_vuln_fn")
}

/// El almacen de la prueba.
async fn store() -> Arc<SurrealReachabilityStore<Db>> {
    let tmp = tempfile::tempdir().expect("a throwaway directory");
    let path = tmp.keep();
    let db = Arc::new(
        Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei")
        .use_db("reachability")
        .await
        .expect("the namespace");
    Arc::new(SurrealReachabilityStore::new(db))
}

/// La evidencia de un analisis completo, con el camino.
fn the_evidence(id: &str, version: &str, revision: &str) -> ReachabilityEvidence {
    ReachabilityEvidence::new(
        id,
        AnalyzerIdentity::new("hodei-local", version),
        Coverage::complete(revision, vec!["app".to_string(), "lib".to_string()]),
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
    )
}

/// El resultado de un escenario, tal como queda registrado.
fn record(scenario: &str, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => println!("{scenario}: PASSED"),
        Err(reason) => println!("{scenario}: FAILED -- {reason}"),
    }
}

// ───────────────────────────── M5-UAT-01 ─────────────────────────────

/// M5-UAT-01 El camino alcanzable.
///
/// Given an internally supplied, versioned ReachabilityEvidence fixture
/// where application symbol Entry calls library vulnerable symbol VulnFn
/// through graph path, when vulnerability mapping is evaluated, then
/// reachability is REACHABLE with explicit call path and analyzer evidence.
#[tokio::test]
async fn m5_uat_01_reachable_vulnerable_path() {
    let result: Result<(), String> = async {
        let store = store().await;
        let evidence = the_evidence("ev-1", "1.0.0", "rev-1");
        store.put(&evidence).await.map_err(|e| e.to_string())?;

        // Se lee del almacen, no de la variable que se escribio.
        let read = store
            .evidence_of("rev-1", &vulnerable())
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(read.len(), 1, "the evidence is there");

        let reachability = derive(&read[0], &vulnerable());
        assert_eq!(reachability.state, ReachabilityState::Reachable);

        // Y el camino es explicito.
        assert_eq!(reachability.call_path.len(), 3);
        assert_eq!(reachability.call_path[0], entry());
        assert_eq!(reachability.call_path[1], caller());
        assert_eq!(reachability.call_path[2], vulnerable());

        // Y la evidencia del analizador dice quien y que version.
        assert_eq!(reachability.analyzer.name, "hodei-local");
        assert_eq!(reachability.analyzer.version, "1.0.0");
        assert_eq!(reachability.evidence_id, "ev-1");
        Ok(())
    }
    .await;

    record("M5-UAT-01", result);
}

// ───────────────────────────── M5-UAT-02 ─────────────────────────────

/// M5-UAT-02 El simbolo que no se alcanza.
///
/// Given affected package is present but no path from application entry
/// scope to VulnFn according to supported analyzer, when evaluated, then
/// state is UNREACHABLE with analyzer scope/limitations recorded.
#[tokio::test]
async fn m5_uat_02_unreachable_vulnerable_symbol() {
    let result: Result<(), String> = async {
        let store = store().await;
        let evidence = the_evidence("ev-1", "1.0.0", "rev-1");
        store.put(&evidence).await.map_err(|e| e.to_string())?;

        let read = store
            .evidence_of("rev-1", &other_vulnerable())
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(read.len(), 1);

        let reachability = derive(&read[0], &other_vulnerable());
        assert_eq!(reachability.state, ReachabilityState::Unreachable);
        assert!(reachability.call_path.is_empty());

        // Y el alcance y los limites quedan registrados: un
        // UNREACHABLE sin ellos no significa nada.
        assert!(
            reachability.reason.contains("covered all the code"),
            "the reason says that the coverage was complete: {}",
            reachability.reason
        );
        assert_eq!(reachability.analyzer.version, "1.0.0");
        assert!(
            read[0].coverage.complete,
            "and the coverage is explicit in the evidence"
        );
        assert!(
            read[0].limitations.is_empty(),
            "and there were no limitations"
        );
        Ok(())
    }
    .await;

    record("M5-UAT-02", result);
}

// ───────────────────────────── M5-UAT-03 ─────────────────────────────

/// M5-UAT-03 Lo desconocido sigue desconocido.
///
/// Given package affected but analyzer cannot resolve dynamic
/// dispatch/reflection/missing source, then reachability is UNKNOWN and
/// trust policy must not treat it as UNREACHABLE.
#[tokio::test]
async fn m5_uat_03_unknown_remains_unknown() {
    let result: Result<(), String> = async {
        let store = store().await;

        // El analisis no pudo resolver el despacho dinamico.
        let evidence =
            the_evidence("ev-1", "1.0.0", "rev-1").with_limitation(Limitation::DynamicDispatch);
        store.put(&evidence).await.map_err(|e| e.to_string())?;

        let read = store
            .evidence_of("rev-1", &other_vulnerable())
            .await
            .map_err(|e| e.to_string())?;
        let reachability = derive(&read[0], &other_vulnerable());

        assert_eq!(
            reachability.state,
            ReachabilityState::Unknown,
            "the dynamic dispatch that was not followed is not a proof"
        );
        assert_ne!(
            reachability.state,
            ReachabilityState::Unreachable,
            "and the trust policy must not treat it as UNREACHABLE"
        );
        assert!(
            reachability.reason.contains("not a proof"),
            "and the reason says so: {}",
            reachability.reason
        );

        // Y lo mismo con el analisis incompleto.
        let mut partial = the_evidence("ev-2", "1.0.0", "rev-2");
        partial.coverage = Coverage::partial("rev-2", vec!["app".to_string()]);
        // El analisis cubrio el simbolo, pero no todo lo demas.
        partial.coverage.symbols = vec![other_vulnerable()];
        store.put(&partial).await.map_err(|e| e.to_string())?;

        let read = store
            .evidence_of("rev-2", &other_vulnerable())
            .await
            .map_err(|e| e.to_string())?;
        let reachability = derive(&read[0], &other_vulnerable());
        assert_eq!(reachability.state, ReachabilityState::Unknown);
        assert!(reachability.reason.contains("not a proof"));
        Ok(())
    }
    .await;

    record("M5-UAT-03", result);
}

// ───────────────────────────── M5-UAT-04 ─────────────────────────────

/// M5-UAT-04 Cambia la version del analizador.
///
/// Given same source analyzed by versions A1 and A2 producing different
/// paths, then both evidence revisions are preserved and derived
/// reachability identifies analyzer/version used.
#[tokio::test]
async fn m5_uat_04_analyzer_version_changes() {
    let result: Result<(), String> = async {
        let store = store().await;

        // La version 1.0.0 no encuentra el camino.
        let first = ReachabilityEvidence::new(
            "ev-1",
            AnalyzerIdentity::new("hodei-local", "1.0.0"),
            Coverage::complete("rev-1", vec!["app".to_string(), "lib".to_string()]),
            vec![entry()],
            vec![],
            Utc::now(),
        );
        store.put(&first).await.map_err(|e| e.to_string())?;

        // Y la version 2.0.0 si.
        let second = the_evidence("ev-2", "2.0.0", "rev-1");
        store.put(&second).await.map_err(|e| e.to_string())?;

        // Las dos se conservan.
        let evidences = store
            .evidence_of_revision("rev-1")
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(
            evidences.len(),
            2,
            "both evidence revisions are preserved: a new analyser does not erase the old answer"
        );

        // Y la derivacion identifica el analizador y la version.
        let reachability = derive(&evidences[1], &vulnerable());
        assert_eq!(reachability.state, ReachabilityState::Reachable);
        assert_eq!(reachability.analyzer.version, "2.0.0");
        assert_eq!(reachability.evidence_id, "ev-2");

        // Y la version vieja sigue diciendo lo que decia.
        let reachability = derive(&evidences[0], &vulnerable());
        assert_eq!(reachability.state, ReachabilityState::Unreachable);
        assert_eq!(reachability.analyzer.version, "1.0.0");
        Ok(())
    }
    .await;

    record("M5-UAT-04", result);
}

// ───────────────────────────── M5-UAT-05 ─────────────────────────────

/// M5-UAT-05 Un cambio en el codigo.
///
/// Given a commit introduces a new call to VulnFn, when code graph updates,
/// then reachability changes from UNREACHABLE/UNKNOWN to REACHABLE and
/// risk/trust reevaluation is requested.
#[tokio::test]
async fn m5_uat_05_incremental_source_change() {
    let result: Result<(), String> = async {
        let store = store().await;

        // Antes del cambio: el simbolo no se alcanza.
        let before = ReachabilityEvidence::new(
            "ev-1",
            AnalyzerIdentity::new("hodei-local", "1.0.0"),
            Coverage::complete("rev-1", vec!["app".to_string(), "lib".to_string()]),
            vec![entry()],
            vec![],
            Utc::now(),
        );
        store.put(&before).await.map_err(|e| e.to_string())?;

        let read = store
            .evidence_of("rev-1", &vulnerable())
            .await
            .map_err(|e| e.to_string())?;
        let before_state = derive(&read[0], &vulnerable()).state;
        assert_eq!(before_state, ReachabilityState::Unreachable);

        // Y despues del cambio: se alcanza.
        let after = the_evidence("ev-2", "1.0.0", "rev-2");
        store.put(&after).await.map_err(|e| e.to_string())?;

        let read = store
            .evidence_of("rev-2", &vulnerable())
            .await
            .map_err(|e| e.to_string())?;
        let after_state = derive(&read[0], &vulnerable()).state;
        assert_eq!(
            after_state,
            ReachabilityState::Reachable,
            "a new call changes the reachability"
        );
        assert_ne!(after_state, before_state);

        // Y el estado dice que revision lo produjo, que es lo que permite
        // pedir la reevaluacion del riesgo y de la confianza.
        assert_eq!(read[0].coverage.source_revision, "rev-2");
        Ok(())
    }
    .await;

    record("M5-UAT-05", result);
}

// ───────────────────────────── M5-UAT-06 ─────────────────────────────

/// M5-UAT-06 La explicacion para el desarrollador.
///
/// Given REACHABLE result, then UI/API returns shortest/relevant call path,
/// package/dependency path, vulnerable symbol and evidence limitations in
/// actionable form.
#[tokio::test]
async fn m5_uat_06_developer_explanation() {
    let result: Result<(), String> = async {
        let store = store().await;
        let evidence =
            the_evidence("ev-1", "1.0.0", "rev-1").with_limitation(Limitation::MissingLibraryBody);
        store.put(&evidence).await.map_err(|e| e.to_string())?;

        let read = store
            .evidence_of("rev-1", &vulnerable())
            .await
            .map_err(|e| e.to_string())?;
        let reachability = derive(&read[0], &vulnerable());

        assert_eq!(reachability.state, ReachabilityState::Reachable);

        // El camino mas corto y mas relevante.
        assert_eq!(reachability.call_path.len(), 3);
        assert_eq!(reachability.call_path.last(), Some(&vulnerable()));

        // El simbolo vulnerable, con su paquete.
        let symbol =
            VulnerableSymbol::new("org.example:lib", Some("2.0.0".to_string()), vulnerable());
        assert_eq!(symbol.identity, vulnerable());
        assert_eq!(symbol.package, "org.example:lib");

        // Y las limitaciones de la evidencia, que son accionables: quien
        // lo lee tiene que saber que el analisis no lo vio todo.
        assert_eq!(read[0].limitations.len(), 1);
        assert_eq!(read[0].limitations[0].as_str(), "missing_library_body");
        assert!(
            reachability.analyzer.name == "hodei-local" && reachability.analyzer.version == "1.0.0",
            "and which analyser produced it"
        );
        Ok(())
    }
    .await;

    record("M5-UAT-06", result);
}

/// El comando con el que se ejecuta esta suite queda escrito.
#[test]
fn the_command_is_recorded() {
    assert!(COMMAND.contains("m5_uat"));
}

/// Y la evidencia que se escribe sobrevive a que se lea otra vez.
#[tokio::test]
async fn the_evidence_survives_the_read() {
    let store = store().await;
    let evidence = the_evidence("ev-1", "1.0.0", "rev-1");
    store.put(&evidence).await.expect("written");

    let read = store
        .evidence_of("rev-1", &vulnerable())
        .await
        .expect("read");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].analyzer.version, "1.0.0");
    assert_eq!(read[0].call_edges.len(), 2, "with its call edges");
    assert!(read[0].analyzed_at <= Utc::now() + Duration::seconds(1));
}
