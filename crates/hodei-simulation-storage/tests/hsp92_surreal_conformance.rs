//! La conformidad de la simulacion, sobre el grafo de verdad.
//!
//! La MISMA suite que corre en memoria, aqui sobre SurrealDB: el mismo
//! fixture, la misma cuenta y los mismos escenarios. El adaptador es lo
//! unico que cambia, y por eso la comparacion de los dos informes es la
//! prueba de que la simulacion se comporta igual con cualquier mundo.

use std::sync::Arc;

use hodei_evidence_storage::{SurrealEvidenceStore, SurrealGraphStore};
use hodei_simulation::ports::DecisionEvaluator;
use hodei_simulation::run_suite;
use hodei_simulation_storage::{CapsuleDecisionEvaluator, SurrealConformance};
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;

/// La cuenta de la conformidad.
const ACCOUNT: &str = "acme";

/// El tenant de la cuenta.
fn the_tenant() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Artifact/{ACCOUNT}"
    ))
    .expect("un HRN")
}

/// La suite entera pasa sobre el grafo real.
///
/// Los mismos cinco escenarios que en memoria, con el grafo de verdad
/// debajo: el fixture se escribe en el grafo y el escenario ve lo que ve
/// el servicio.
#[tokio::test]
async fn the_conformance_suite_passes_over_surrealdb() {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let path = tmp.keep();

    let db = Arc::new(
        Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
            .await
            .expect("la base de datos abre"),
    );
    db.use_ns("hodei")
        .use_db("evidence")
        .await
        .expect("el namespace");

    let store = Arc::new(SurrealGraphStore::new(db.clone()));
    let evidence = Arc::new(SurrealEvidenceStore::new(db.clone()));
    let decisions: Arc<dyn DecisionEvaluator> =
        Arc::new(CapsuleDecisionEvaluator::new(evidence, ACCOUNT));
    let world = SurrealConformance::new(store, the_tenant(), decisions);

    let report = run_suite(&world).await;

    for scenario in &report.scenarios {
        assert!(scenario.passed, "{}: {:?}", scenario.name, scenario.reason);
    }
    assert!(
        report.passed(),
        "todos los escenarios pasan: {:?}",
        report.failures()
    );
    assert_eq!(
        report.scenarios.len(),
        5,
        "los cinco escenarios de la suite: {:?}",
        report.scenarios
    );
}
