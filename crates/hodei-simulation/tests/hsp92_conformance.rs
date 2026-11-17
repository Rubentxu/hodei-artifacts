//! La conformidad de la simulacion, en memoria.
//!
//! La suite de conformidad vive en el dominio, y esto la corre contra el
//! fixture en memoria: el grafo que no es un grafo de verdad, con las
//! mismas relaciones y las mismas reglas.
//!
//! La misma suite corre contra SurrealDB en el crate de almacen: un
//! escenario que pasa aqui y falla alla es un fallo del adaptador.

use hodei_simulation::memory::InMemoryConformance;
use hodei_simulation::run_suite;
use kernel::Hrn;

/// El tenant de la cuenta de la conformidad.
fn the_tenant() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/acme").expect("un HRN")
}

/// La suite entera pasa en memoria.
///
/// Y si un escenario falla, el informe dice WHICH: un informe que solo
/// dice "fallo" no dice nada.
#[tokio::test]
async fn the_conformance_suite_passes_in_memory() {
    let world = InMemoryConformance::new("acme", the_tenant());

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
