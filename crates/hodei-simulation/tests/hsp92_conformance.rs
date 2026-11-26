//! The conformance of the simulation, in memory.
//!
//! The conformance suite lives in the domain, and this runs it against the
//! in-memory fixture: the graph that is not a real graph, with the
//! same relations and the same rules.
//!
//! The same suite runs against SurrealDB in the store crate: a
//! scenario that passes here and fails there is an adapter failure.

use hodei_simulation::memory::InMemoryConformance;
use hodei_simulation::run_suite;
use kernel::Hrn;

/// The tenant of the conformance account.
fn the_tenant() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/acme").expect("un HRN")
}

/// The whole suite passes in memory.
///
/// And if a scenario fails, the report says WHICH: a report that only
/// says "failure" says nothing.
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
