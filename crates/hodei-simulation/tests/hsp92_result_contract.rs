//! El contrato del resultado de una simulacion.
//!
//! El resultado es lo que se entrega: un contrato **versionado** que
//! separa lo observado de lo hipotetico, dice que politicas
//! intervinieron, dice que se ha cubierto y que no, y declara los
//! conflictos del escenario en vez de esconderlos.
//!
//! Y lo hace sin arrastrar objetos de politica: lo que viaja es el
//! sello --identificador y version--, porque un contrato que lleva la
//! politica entera deja de ser portable.

use chrono::{DateTime, Utc};
use hodei_evidence::domain::graph::RelationType;
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use hodei_simulation::memory::{InMemoryDecisions, InMemoryGraph};
use hodei_simulation::result::{
    ConflictKind, HypothesisKind, SIMULATION_RESULT_VERSION, SimulationResult,
};
use hodei_simulation::service::SimulationService;
use hodei_simulation::{Checkpoint, SimulationFork, SimulationTag};
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

/// El instante de las pruebas: uno solo.
fn the_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
        .expect("un instante fijo")
        .with_timezone(&Utc)
}

/// El sujeto de la simulacion.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
}

/// El componente del que depende.
fn component_v1() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
}

/// El segundo componente.
fn component_v2() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
}

/// El componente al que se sube.
fn component_v3() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v3").expect("un HRN")
}

/// Un sujeto que la simulacion no toca: su relacion no se lee.
fn other() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/other-1.0.0").expect("un HRN")
}

/// Otro sujeto que la simulacion no toca.
fn another() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/another-1.0.0").expect("un HRN")
}

/// La politica de ahora: la que no prohibe nada.
fn the_policy() -> TrustPolicy {
    TrustPolicy::new("production", "1.0.0", Vec::new())
}

/// La politica hipotetica: la que prohibe el riesgo establecido.
fn the_strict_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "2.0.0",
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// El grafo del mundo: dos dependencias del sujeto y una que no es suya.
fn the_graph() -> std::sync::Arc<InMemoryGraph> {
    let graph = InMemoryGraph::new("acme");
    graph.set_the_world(
        vec![
            RelationFact::new(RelationType::DependsOn, subject(), component_v1())
                .with_evidence("evd-1"),
            RelationFact::new(RelationType::DependsOn, component_v1(), component_v2())
                .with_evidence("evd-2"),
            RelationFact::new(RelationType::DependsOn, other(), another()).with_evidence("evd-3"),
        ],
        7,
    );
    graph
}

/// El servicio de la simulacion sobre el grafo.
fn the_service(graph: std::sync::Arc<InMemoryGraph>) -> SimulationService {
    SimulationService::new(graph, std::sync::Arc::new(InMemoryDecisions))
}

/// El fork de la simulacion.
fn the_fork() -> SimulationFork {
    SimulationFork::new(
        "sim-1",
        Checkpoint::new("checkpoint-1", "acme", the_now(), Some(42), 7),
        the_policy(),
        the_now(),
    )
}

/// La etiqueta de la simulacion.
fn the_tag() -> SimulationTag {
    SimulationTag::new("sim-1", the_now())
}

/// La hipotetica del riesgo: el sujeto queda expuesto.
fn the_risk() -> Hypothetical {
    Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Risk {
            subject: subject(),
            advisory: "GHSA-hsp92-0001".to_string(),
            critical: true,
            reachable: true,
        },
    }
}

/// El resultado de una simulacion con el riesgo y la politica estricta.
async fn the_result() -> SimulationResult {
    let graph = the_graph();
    let service = the_service(graph);
    let fork = the_fork().with(the_risk()).with_policy(the_strict_policy());
    service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace")
}

// ───────────────────────────── la version ─────────────────────────────

/// El resultado declara la version del contrato.
///
/// Un contrato que no dice su version es un contrato que no se puede
/// evolve: el consumidor no sabe si lo que lee es lo que espera.
#[tokio::test]
async fn the_result_declares_its_version() {
    let result = the_result().await;

    assert_eq!(result.version, SIMULATION_RESULT_VERSION);
    assert_eq!(result.version, "1.0.0");

    // Y el JSON que se lleva dice lo mismo.
    let json = serde_json::to_string(&result).expect("el resultado se serializa");
    assert!(
        json.contains("\"version\":\"1.0.0\""),
        "el JSON declara la version: {json}"
    );
}

// ───────────────────────────── lo observado y lo hipotetico ─────────────────────────────

/// El resultado separa lo observado de lo hipotetico.
///
/// Y la hipotetica **no se publica como hecho del grafo**: las
/// relaciones observadas son las que estaban, y la hipotetica viaja en
/// su propio campo.
#[tokio::test]
async fn the_result_separates_the_observed_from_the_hypothetical() {
    let result = the_result().await;

    // Lo observado: las relaciones que estaban en el mundo.
    assert_eq!(
        result.observed.relations.len(),
        1,
        "la relacion del sujeto: {:?}",
        result.observed.relations
    );
    assert!(
        !result
            .observed
            .relations
            .iter()
            .any(|relation| relation.to == component_v3()),
        "y la relacion hipotetica NO esta entre las observadas"
    );

    // Lo hipotetico: los escenarios, marcados como tales. Son dos: el
    // riesgo y la politica hipotetica.
    assert_eq!(
        result.hypotheses.len(),
        2,
        "la hipotetica del riesgo y la politica: {:?}",
        result.hypotheses
    );
    let hypothesis = result
        .hypotheses
        .iter()
        .find(|hypothesis| matches!(hypothesis.kind, HypothesisKind::Risk { .. }))
        .expect("la hipotetica del riesgo esta declarada");
    assert_eq!(hypothesis.simulation, "sim-1");
    assert!(matches!(
        hypothesis.kind,
        HypothesisKind::Risk {
            ref advisory,
            critical: true,
            reachable: true,
            ..
        } if advisory == "GHSA-hsp92-0001"
    ));
    assert_eq!(hypothesis.subjects, vec![subject()]);

    // Y la relacion hipotetica, si la hay, tampoco esta entre las
    // observadas.
    let graph = the_graph();
    let service = the_service(graph);
    let fork = the_fork().with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: subject(),
            to: component_v3(),
            replaces: None,
        },
    });
    let result = service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace");
    assert!(
        !result
            .observed
            .relations
            .iter()
            .any(|relation| relation.to == component_v3()),
        "la relacion hipotetica no se publica como hecho del grafo"
    );
    assert!(
        result
            .hypotheses
            .iter()
            .any(|hypothesis| matches!(&hypothesis.kind, HypothesisKind::Relation { to, .. } if *to == component_v3())),
        "y si aparece, es como hipotetica: {:?}",
        result.hypotheses
    );
}

// ───────────────────────────── las politicas ─────────────────────────────

/// El resultado dice que politicas intervinieron.
///
/// La de antes, la de despues y la hipotetica: con su identificador y su
/// version, y sin el objeto entero.
#[tokio::test]
async fn the_result_says_which_policies_intervened() {
    let result = the_result().await;

    // La politica que estaba en vigor.
    assert_eq!(result.observed.policy.id, "production");
    assert_eq!(result.observed.policy.version, "1.0.0");

    // Y las dos versiones de cada decision que cambia.
    assert_eq!(result.decisions.len(), 1, "la decision que cambia");
    let change = &result.decisions[0];
    assert_eq!(change.policy_version_before, "1.0.0");
    assert_eq!(change.policy_version_after, "2.0.0");

    // Y la politica hipotetica viaja como hipotetica, con su sello.
    let hypothetical_policy = result
        .hypotheses
        .iter()
        .find(|hypothesis| matches!(hypothesis.kind, HypothesisKind::Policy { .. }))
        .expect("la politica hipotetica esta declarada");
    match &hypothetical_policy.kind {
        HypothesisKind::Policy { policy } => {
            assert_eq!(policy.id, "production");
            assert_eq!(policy.version, "2.0.0");
        }
        other => panic!("no es una politica: {other:?}"),
    }
}

/// Y el contrato NO lleva la politica entera: solo su sello.
///
/// Un contrato que arrastra el objeto de la politica deja de ser
/// portable, y lo que se necesita para saber que reglas se aplicaron
/// es la version.
#[tokio::test]
async fn the_result_does_not_carry_a_policy_object() {
    let result = the_result().await;

    let json = serde_json::to_string(&result).expect("el resultado se serializa");
    assert!(
        !json.contains("risk_established"),
        "el resultado no lleva los requisitos de la politica: {json}"
    );
    assert!(
        !json.contains("requirements"),
        "ni una lista de requisitos: {json}"
    );
}

// ───────────────────────────── la base ─────────────────────────────

/// El resultado lleva el sello de la base: sobre que mundo se calculo.
#[tokio::test]
async fn the_result_carries_the_stamp_of_the_base() {
    let result = the_result().await;

    assert_eq!(result.account, "acme");
    assert_eq!(result.simulation, "sim-1");
    assert_eq!(result.base.account, "acme");
    assert_eq!(result.base.evaluated_at, the_now());
    assert_eq!(result.base.generation, Some(7));
    assert_eq!(result.base.declared_ledger_position, Some(42));
    assert!(
        !result.base.seal.is_empty(),
        "el sello del contenido leido no esta vacio"
    );
    assert!(
        !result.base.fingerprint.is_empty(),
        "la huella del manifiesto no esta vacia"
    );
    assert_eq!(
        result.base.sources,
        vec![hodei_simulation::result::SourceStamp {
            source: "graph".to_string(),
            version: 7,
        }],
        "y la version de la fuente leida: {:?}",
        result.base.sources
    );
}

// ───────────────────────────── la cobertura ─────────────────────────────

/// El resultado dice que se ha cubierto y que no.
///
/// La relacion del otro sujeto no se lee, y el resultado lo dice: un
/// resultado que no dice lo que no ha visto parece completo.
#[tokio::test]
async fn the_result_carries_the_coverage() {
    let result = the_result().await;

    assert_eq!(result.coverage.evaluated_subjects, vec![subject()]);
    assert!(
        result.coverage.declared_subjects.is_empty(),
        "no se declararon sujetos: {:?}",
        result.coverage.declared_subjects
    );
    assert_eq!(
        result.coverage.relations_read, 1,
        "la relacion del sujeto, que es lo unico que toca"
    );
    assert_eq!(
        result.coverage.relations_not_read, 2,
        "y las otras dos de la cuenta, que no se leen"
    );
    assert_eq!(
        result.coverage.omitted.len(),
        1,
        "y se dice que no se ha leido: {:?}",
        result.coverage.omitted
    );
    assert!(
        result.coverage.omitted[0]
            .reason
            .contains("2 relations of the account were not read"),
        "con su motivo: {:?}",
        result.coverage.omitted[0]
    );
}

// ───────────────────────────── los conflictos ─────────────────────────────

/// El ciclo del overlay se declara.
///
/// Un overlay que cierra un ciclo y se presenta como un overlay normal
/// es un overlay que nadie puede leer.
#[tokio::test]
async fn the_cycle_of_the_overlay_is_declared() {
    let graph = the_graph();
    let service = the_service(graph);
    let fork = the_fork().with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: component_v2(),
            to: subject(),
            replaces: None,
        },
    });

    let result = service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace");

    let conflict = result
        .conflicts
        .iter()
        .find(|conflict| conflict.kind == ConflictKind::Cycle)
        .expect("el ciclo se declara");
    assert!(
        conflict.subjects.contains(&subject()),
        "y dice a quien le llega: {:?}",
        conflict.subjects
    );
}

/// El duplicado se declara: dos hipoteticas que dicen lo mismo.
#[tokio::test]
async fn the_duplicate_of_the_hypothesis_is_declared() {
    let graph = the_graph();
    let service = the_service(graph);
    let claim = HypotheticalClaim::Relation {
        relation: RelationType::DependsOn,
        from: subject(),
        to: component_v3(),
        replaces: None,
    };
    let fork = the_fork()
        .with(Hypothetical {
            tag: the_tag(),
            claim: claim.clone(),
        })
        .with(Hypothetical {
            tag: the_tag(),
            claim,
        });

    let result = service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace");

    assert!(
        result
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == ConflictKind::Duplicate),
        "el duplicado se declara: {:?}",
        result.conflicts
    );
}

/// La contradiccion se declara: una relacion que se anade y se
/// sustituye a la vez.
#[tokio::test]
async fn the_contradiction_of_the_hypotheses_is_declared() {
    let graph = the_graph();
    let service = the_service(graph);
    let fork = the_fork().with(Hypothetical {
        tag: the_tag(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: subject(),
            to: component_v3(),
            replaces: Some(RelationFact::new(
                RelationType::DependsOn,
                subject(),
                component_v3(),
            )),
        },
    });

    let result = service
        .simulate_result(&fork, the_now())
        .await
        .expect("la simulacion se hace");

    assert!(
        result
            .conflicts
            .iter()
            .any(|conflict| conflict.kind == ConflictKind::Contradiction),
        "la contradiccion se declara: {:?}",
        result.conflicts
    );
}

// ───────────────────────────── la reproducibilidad ─────────────────────────────

/// La misma simulacion da el mismo resultado.
///
/// El reloj es inyectado, el resultado es canonico y por eso dos
/// simulaciones del mismo mundo se pueden comparar.
#[tokio::test]
async fn the_same_simulation_gives_the_same_result() {
    let first = the_result().await;
    let second = the_result().await;

    assert_eq!(first, second, "el resultado es el mismo");
}
