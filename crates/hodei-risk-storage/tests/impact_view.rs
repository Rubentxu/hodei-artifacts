//! La vista materializada del impacto, dentro de SurrealDB.
//!
//! La respuesta autoritativa de "a que alcanza esta vulnerabilidad" es
//! el recorrido del grafo, y la vista dice LO MISMO: los mismos
//! sujetos, las mismas profundidades, incluido un caso de 522 sujetos
//! del benchmark. Lo que cambia se invalida SOLO donde cambio, y la
//! version hasta la que la vista esta completa se expone: el lag no se
//! presenta como actualidad.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use hodei_evidence::domain::delta::{RelationDelta, RelationDeltaKind, RelationDeltaLedger};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal, NodeKind, RelationType,
};
use hodei_evidence::ports::graph::GraphWriter;
use hodei_evidence::{EvidenceId, RelationDeltaSubscription};
use hodei_evidence_storage::surreal::delta::SurrealRelationDeltaLedger;
use hodei_evidence_storage::surreal::graph::SurrealGraphStore;
use hodei_risk_storage::impact_view::{SurrealImpactView, IMPACT_TABLE};
use hodei_reactive::reactions::BlastRadiusSource;
use hodei_risk_storage::reactions::SurrealBlastRadius;
use kernel::Hrn;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

fn tenant() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/acme").expect("un HRN")
}

fn other_tenant() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:mallory:Artifact/mallory").expect("un HRN")
}

fn component() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-shared").expect("un HRN")
}

fn vulnerability() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-view-0001")
        .expect("un HRN")
}

fn artifact(index: usize) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-{index}"
    ))
    .expect("un HRN")
}

fn moment() -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800, 0).single().expect("un momento")
}

/// El changeset de un artefacto que contiene el componente vulnerable,
/// con su causa: la causa es lo que hace que el grafo emita deltas.
fn the_contains(cause: &str, index: usize) -> GraphChangeSet {
    let evidence_id = EvidenceId::new(format!("ev-{index}"));
    let shared = EvidenceId::new("ev-shared");
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_node(GraphNode::new(component(), tenant(), NodeKind::Component, moment()))
        .with_node(GraphNode::new(vulnerability(), tenant(), NodeKind::Vulnerability, moment()))
        .with_node(GraphNode::new(artifact(index), tenant(), NodeKind::Artifact, moment()))
        .with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(&tenant(), &evidence_id),
            tenant(),
            NodeKind::Evidence,
            moment(),
        ))
        .with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(&tenant(), &shared),
            tenant(),
            NodeKind::Evidence,
            moment(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            artifact(index),
            component(),
            tenant(),
            evidence_id,
            moment(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            component(),
            vulnerability(),
            tenant(),
            EvidenceId::new("ev-shared"),
            moment(),
        ))
}

/// La retirada del apoyo de UNA de las relaciones Contains, con su
/// causa.
fn the_withdrawal(cause: &str, index: usize) -> GraphChangeSet {
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_withdrawal(GraphRelationWithdrawal::new(
            RelationType::Contains,
            artifact(index),
            component(),
            tenant(),
            EvidenceId::new(format!("ev-{index}")),
            moment(),
        ))
}

/// El mundo de la vista: el grafo, el registro de deltas y la vista,
/// todos sobre la misma base.
async fn the_world() -> (
    tempfile::TempDir,
    Arc<SurrealGraphStore<Db>>,
    Arc<SurrealRelationDeltaLedger<Db>>,
    SurrealImpactView<Db>,
) {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let db = Arc::new(
        Surreal::new::<RocksDb>(tmp.path().join("db").to_string_lossy().to_string())
            .await
            .expect("la base abre"),
    );
    db.use_ns("hodei").use_db("evidence").await.expect("el espacio");
    let graph = Arc::new(SurrealGraphStore::new(db.clone()));
    let deltas = Arc::new(SurrealRelationDeltaLedger::new(db));
    let view = SurrealImpactView::new(graph.clone(), deltas.clone());
    (tmp, graph, deltas, view)
}

/// AC1: la vista materializa LO MISMO que el recorrido autoritativo,
/// incluido un caso de 522 sujetos.
#[tokio::test]
async fn the_view_says_what_the_walk_says_including_522_subjects() {
    let (_tmp, graph, _deltas, view) = the_world().await;

    // El caso del benchmark: el componente vulnerable esta en 521
    // artefactos, y el sujeto afectado de profundidad cero es el
    // componente. 521 + 1 = 522 sujetos.
    for index in 0..521 {
        graph
            .apply(the_contains("evt-load", index))
            .await
            .expect("la carga entra");
    }

    // El recorrido autoritativo: la respuesta de siempre.
    let radius = hodei_risk_storage::blast_radius(graph.as_ref(), &tenant(), &vulnerability())
        .await
        .expect("el recorrido corre");
    let walk_subjects: Vec<(String, usize)> = {
        let mut subjects: Vec<(String, usize)> = radius
            .affected_subjects
            .iter()
            .map(|subject| (subject.clone(), 1usize))
            .collect();
        for component in &radius.affected_components {
            subjects.push((component.clone(), 0usize));
        }
        subjects.sort();
        subjects
    };

    // La vista, materializada desde el MISMO recorrido.
    let snapshot = view.rebuild_advisory(&tenant(), &vulnerability()).await.expect("la vista corre");

    assert_eq!(
        snapshot.subjects.len(),
        522,
        "el caso del benchmark alcanza 522 sujetos: {}",
        snapshot.subjects.len()
    );
    assert_eq!(
        snapshot.subjects, walk_subjects,
        "la vista dice lo que el recorrido autoritativo dice"
    );

    // Y la lectura de la vista, por su puerta, dice lo mismo.
    let read = view.read(&tenant(), &vulnerability()).await.expect("la lectura corre").expect("la vista esta");
    assert_eq!(read.subjects, snapshot.subjects);
    assert_eq!(read.watermark, snapshot.watermark);
}

/// AC2: los deltas solo invalidan LAS CLAVES AFECTADAS, y la
/// reentrega del mismo trabajo no repite nada.
#[tokio::test]
async fn the_deltas_invalidate_only_the_keys_they_touch() {
    let (_tmp, graph, _deltas, view) = the_world().await;

    for index in 0..4 {
        graph
            .apply(the_contains("evt-load", index))
            .await
            .expect("la carga entra");
    }
    view.rebuild_advisory(&tenant(), &vulnerability())
        .await
        .expect("la vista corre");

    // El delta de una relacion que la ruta del advisory uso: el
    // advisory SE invalida.
    let route_delta = RelationDelta {
        kind: RelationDeltaKind::Removed,
        tenant: tenant(),
        relation: RelationType::Contains,
        from: artifact(2),
        to: component(),
        evidence: EvidenceId::new("ev-2"),
        cause: "evt-withdraw".to_string(),
        observed_at: moment(),
    };
    let mut meta = graph
        .db()
        .query("SELECT * FROM type::table($table)")
        .bind(("table", "impact_meta"))
        .await
        .expect("la lectura corre");
    let meta_rows: Vec<hodei_risk_storage::impact_view::ImpactRoute> =
        meta.take(0).expect("las filas meta");
    eprintln!("META={meta_rows:?}");
    let touched = view.apply_delta(&route_delta).await.expect("la invalidacion corre");
    assert_eq!(
        touched,
        vec![vulnerability()],
        "el advisory cuya ruta uso la relation se invalida: {touched:?}"
    );

    // El delta de una relacion que NINGUNA ruta uso: no invalida nada.
    let foreign_delta = RelationDelta {
        kind: RelationDeltaKind::Added,
        tenant: other_tenant(),
        relation: RelationType::Contains,
        from: Hrn::from_string("hrn:hodei:artifacts:eu-west-1:mallory:Artifact/app-0")
            .expect("un HRN"),
        to: component(),
        evidence: EvidenceId::new("ev-mallory"),
        cause: "evt-mallory".to_string(),
        observed_at: moment(),
    };
    let untouched = view.apply_delta(&foreign_delta).await.expect("la invalidacion corre");
    assert!(
        untouched.is_empty(),
        "lo de otra cuenta no invalida nada de esta vista: {untouched:?}"
    );

    // La reentrega: el mismo delta otra vez es el mismo trabajo una vez,
    // y la vista queda IGUAL, no doble.
    let before = view.read(&tenant(), &vulnerability()).await.expect("la lectura corre");
    let again = view.apply_delta(&route_delta).await.expect("la reentrega corre");
    assert_eq!(again, vec![vulnerability()], "la reentrega invalida lo mismo");
    let after = view.read(&tenant(), &vulnerability()).await.expect("la lectura corre");
    assert_eq!(before, after, "recomputar lo mismo deja lo mismo");
}

/// AC2 (retirada real): la evidencia retirada saca al sujeto de la
/// vista, y SOLO a el.
#[tokio::test]
async fn a_withdrawn_support_takes_only_its_subject_out_of_the_view() {
    let (_tmp, graph, deltas, view) = the_world().await;

    for index in 0..3 {
        graph
            .apply(the_contains("evt-load", index))
            .await
            .expect("la carga entra");
    }
    let before = view.rebuild_advisory(&tenant(), &vulnerability()).await.expect("la vista corre");
    assert_eq!(before.subjects.len(), 4, "tres artefactos y el componente");

    // La retirada REAL pasa por el grafo: el apoyo de app-2 se retira y
    // el grafo emite SU delta.
    graph.apply(the_withdrawal("evt-withdraw", 2)).await.expect("la retirada entra");
    let said = deltas.version_of(&tenant()).await.expect("la version corre");
    assert!(said > 0, "la retirada dejo su delta dicho: {said}");

    // La vista se invalida con lo que dijo el grafo.
    let affected = view.apply_delta(&RelationDelta {
        kind: RelationDeltaKind::Removed,
        tenant: tenant(),
        relation: RelationType::Contains,
        from: artifact(2),
        to: component(),
        evidence: EvidenceId::new("ev-2"),
        cause: "evt-withdraw".to_string(),
        observed_at: moment(),
    }).await.expect("la invalidacion corre");
    assert_eq!(affected, vec![vulnerability()]);

    let after = view.read(&tenant(), &vulnerability()).await.expect("la lectura corre").expect("la vista esta");
    assert_eq!(after.subjects.len(), 3, "app-2 salio de la vista");
    assert!(
        after.subjects.iter().all(|(subject, _)| subject.as_str() != artifact(2).to_string()),
        "el sujeto retirado no esta: {:?}",
        after.subjects
    );
}

/// AC3: el watermark dice hasta que version la vista esta completa, y
/// el lag NO se presenta como actualidad.
#[tokio::test]
async fn the_watermark_exposes_the_lag_instead_of_hiding_it() {
    let (_tmp, graph, deltas, view) = the_world().await;

    graph.apply(the_contains("evt-1", 0)).await.expect("la carga entra");
    view.rebuild_advisory(&tenant(), &vulnerability()).await.expect("la vista corre");

    // Al dia: la version de la vista alcanza la del registro.
    assert!(
        view.is_current(&tenant()).await.expect("la comprobacion corre"),
        "la vista esta al dia"
    );
    let snapshot = view.read(&tenant(), &vulnerability()).await.expect("la lectura corre").expect("la vista esta");
    let said = deltas.version_of(&tenant()).await.expect("la version corre");
    assert_eq!(snapshot.watermark, said, "la vista dice hasta donde llego");

    // Cambia el grafo: el registro avanza y la vista se queda atras.
    graph.apply(the_contains("evt-2", 1)).await.expect("el nuevo apoyo entra");
    let said = deltas.version_of(&tenant()).await.expect("la version corre");
    assert!(
        !view.is_current(&tenant()).await.expect("la comprobacion corre"),
        "hay lag, y la vista lo dice"
    );

    // La reconstruccion cierra el lag, y el watermark dice la version
    // nueva.
    view.rebuild_advisory(&tenant(), &vulnerability()).await.expect("la vista corre");
    assert!(
        view.is_current(&tenant()).await.expect("la comprobacion corre"),
        "reconstruida, la vista esta al dia"
    );
    let snapshot = view.read(&tenant(), &vulnerability()).await.expect("la lectura corre").expect("la vista esta");
    assert_eq!(snapshot.watermark, said, "el watermark alcanza la version del registro");
    assert_eq!(snapshot.subjects.len(), 3, "dos artefactos y el componente");

    // Y un tenant sin vista no se presenta como actual: no hay vista
    // que presentar.
    assert!(
        !view.is_current(&other_tenant()).await.expect("la comprobacion corre"),
        "sin vista no hay actualidad"
    );
    assert!(
        view.read(&other_tenant(), &vulnerability())
            .await
            .expect("la lectura corre")
            .is_none(),
        "sin vista no hay respuesta stale que disfrazar"
    );
}

/// La vista no se deja enganar por una fila cuya evidencia ya no la
/// apoya: la tabla existe y esta vacia cuando el advisory no tiene
/// vista.
#[tokio::test]
async fn the_view_tables_exist_and_answer_the_truth() {
    let (_tmp, _graph, _deltas, view) = the_world().await;
    let rows: Vec<serde_json::Value> = view
        .the_rows()
        .await
        .expect("las filas");
    assert!(rows.is_empty(), "una vista sin Advisory no tiene filas");
}

/// AC2 (edge nuevo): un artefacto NUEVO que contiene el componente
/// vulnerable no esta en ninguna ruta previa. La vista se invalida por
/// los EXTREMOS de la relacion, no solo por las relaciones que ya
/// conocia, y el sujeto nuevo entra.
#[tokio::test]
async fn a_new_edge_brings_its_subject_into_the_view() {
    let (_tmp, graph, deltas, view) = the_world().await;

    for index in 0..2 {
        graph
            .apply(the_contains("evt-load", index))
            .await
            .expect("la carga entra");
    }
    view.rebuild_advisory(&tenant(), &vulnerability())
        .await
        .expect("la vista corre");
    let before = view
        .read(&tenant(), &vulnerability())
        .await
        .expect("la lectura corre")
        .expect("la vista esta");
    assert_eq!(before.subjects.len(), 3, "dos artefactos y el componente");

    // El edge NUEVO: app-7 contiene el componente vulnerable, y ninguna
    // ruta lo conocia antes de existir.
    graph
        .apply(the_contains("evt-new-edge", 7))
        .await
        .expect("el edge nuevo entra");
    let said = deltas.version_of(&tenant()).await.expect("la version corre");
    assert!(said > 0, "el grafo dijo el delta del edge nuevo: {said}");

    let affected = view
        .apply_delta(&RelationDelta {
            kind: RelationDeltaKind::Added,
            tenant: tenant(),
            relation: RelationType::Contains,
            from: artifact(7),
            to: component(),
            evidence: EvidenceId::new("ev-7"),
            cause: "evt-new-edge".to_string(),
            observed_at: moment(),
        })
        .await
        .expect("la invalidacion corre");
    assert_eq!(
        affected,
        vec![vulnerability()],
        "el edge nuevo invalida el advisory: {affected:?}"
    );

    let after = view
        .read(&tenant(), &vulnerability())
        .await
        .expect("la lectura corre")
        .expect("la vista esta");
    assert_eq!(after.subjects.len(), 4, "el sujeto nuevo entro: {:?}", after.subjects);
    assert!(
        after
            .subjects
            .iter()
            .any(|(subject, _)| subject.as_str() == artifact(7).to_string()),
        "app-7 esta en la vista: {:?}",
        after.subjects
    );
}

/// HSP-83 AC1: la respuesta tipada dice el CAMINO que explica cada
/// sujeto -- las relaciones y la evidencia que lo trajo -- y la
/// frescura de donde sale.
#[tokio::test]
async fn the_answer_says_what_explains_each_subject_and_its_freshness() {
    let (_tmp, graph, deltas, view) = the_world().await;

    for index in 0..2 {
        graph
            .apply(the_contains("evt-load", index))
            .await
            .expect("la carga entra");
    }
    view.rebuild_advisory(&tenant(), &vulnerability())
        .await
        .expect("la vista corre");

    let answer = view
        .serve_impact(&tenant(), &vulnerability())
        .await
        .expect("la respuesta corre");

    assert_eq!(answer.freshness, hodei_risk_storage::impact_view::Freshness::Current,
        "la vista esta al dia y se presenta como tal");
    assert_eq!(answer.version, deltas.version_of(&tenant()).await.expect("la version corre"));

    // Cada sujeto trae su camino: app-0 con la Contains que lo trae y
    // su evidencia; el componente con la AffectedBy que lo declara.
    let artifact_path = answer
        .subjects
        .iter()
        .find(|one| one.subject == artifact(0).to_string())
        .expect("el artefacto esta en la respuesta");
    assert!(
        artifact_path
            .relations
            .iter()
            .any(|relation| relation.starts_with("contains|") && relation.contains(&component().to_string())),
        "el camino del artefacto nombra la Contains: {:?}",
        artifact_path.relations
    );
    assert!(
        artifact_path.evidences.iter().any(|one| one == "ev-0"),
        "el camino dice la evidencia que lo apoya: {:?}",
        artifact_path.evidences
    );
    let component_path = answer
        .subjects
        .iter()
        .find(|one| one.subject == component().to_string())
        .expect("el componente esta en la respuesta");
    assert_eq!(component_path.depth, 0);
    assert!(
        component_path
            .relations
            .iter()
            .any(|relation| relation.starts_with("affected_by|")),
        "el camino del componente nombra la AffectedBy: {:?}",
        component_path.relations
    );
    assert!(
        !answer.deployments.is_empty() || answer.subjects.len() == 3,
        "los despliegues que corren los sujetos se dicen"
    );
}

/// HSP-83 AC2: una vista ATRASADA no se presenta como actual. El
/// fallback dice lo que el recorrido autoritativo dice AHORA, y el lag
/// queda expuesto en la frescura, nunca escondido.
#[tokio::test]
async fn a_stale_view_falls_back_to_the_walk_and_exposes_the_lag() {
    let (_tmp, graph, deltas, view) = the_world().await;

    for index in 0..2 {
        graph
            .apply(the_contains("evt-load", index))
            .await
            .expect("la carga entra");
    }
    view.rebuild_advisory(&tenant(), &vulnerability())
        .await
        .expect("la vista corre");

    // El grafo avanza y la vista NO consume el delta: se queda atras.
    graph
        .apply(the_contains("evt-late", 7))
        .await
        .expect("el apoyo nuevo entra");
    let said = deltas.version_of(&tenant()).await.expect("la version corre");

    let answer = view
        .serve_impact(&tenant(), &vulnerability())
        .await
        .expect("la respuesta corre");

    match &answer.freshness {
        hodei_risk_storage::impact_view::Freshness::Authoritative { view_watermark } => {
            assert!(*view_watermark < said, "el lag queda dicho: {view_watermark} < {said}");
        }
        other => panic!("una vista atrasada no se presenta como actual: {other:?}"),
    }
    assert_eq!(answer.version, said, "la respuesta es completa hasta la version dicha");
    assert!(
        answer
            .subjects
            .iter()
            .any(|one| one.subject == artifact(7).to_string()),
        "el fallback dice lo que el recorrido dice AHORA, con el sujeto nuevo: {:?}",
        answer.subjects
    );
}

/// HSP-83 AC1 (fail-closed): el advisory de OTRA cuenta es una puerta
/// cerrada, no una respuesta vacia que parezca cero impacto.
#[tokio::test]
async fn an_advisory_of_another_account_is_a_closed_door() {
    let (_tmp, _graph, _deltas, view) = the_world().await;

    let foreign = Hrn::from_string(
        "hrn:hodei:artifacts:eu-west-1:mallory:Vulnerability/GHSA-view-0001",
    )
    .expect("un HRN");
    let error = view
        .serve_impact(&tenant(), &foreign)
        .await
        .expect_err("una puerta cerrada es un error");
    assert!(
        matches!(
            error,
            hodei_risk_storage::impact_view::ImpactReadError::TenantMismatch { .. }
        ),
        "el desajuste de cuenta se dice como tal: {error:?}"
    );
}

/// HSP-83 AC2 (sin vista): si la vista no esta, el calculo
/// autoritativo responde AHORA, y lo dice.
#[tokio::test]
async fn without_a_view_the_authoritative_walk_answers_now() {
    let (_tmp, graph, deltas, view) = the_world().await;

    for index in 0..2 {
        graph
            .apply(the_contains("evt-load", index))
            .await
            .expect("la carga entra");
    }

    let answer = view
        .serve_impact(&tenant(), &vulnerability())
        .await
        .expect("la respuesta corre");
    match &answer.freshness {
        hodei_risk_storage::impact_view::Freshness::Authoritative { view_watermark } => {
            assert_eq!(*view_watermark, 0, "no habia vista que presentar");
        }
        other => panic!("sin vista no hay actualidad que reclamar: {other:?}"),
    }
    assert_eq!(answer.version, deltas.version_of(&tenant()).await.expect("la version corre"));
    assert_eq!(answer.subjects.len(), 3, "dos artefactos y el componente");
}

/// HSP-83 AC2 (reactivo): el consumidor de riesgo sirve de la vista con
/// fallback seguro: responde LO MISMO que el recorrido con la vista al
/// dia, y sigue respondiendo lo que el recorrido dice con la vista
/// atrasada. Nunca silencio, nunca falsa certeza.
#[tokio::test]
async fn the_risk_consumer_serves_from_the_view_with_a_safe_fallback() {
    let (_tmp, graph, _deltas, view) = the_world().await;
    let view = Arc::new(view);

    for index in 0..2 {
        graph
            .apply(the_contains("evt-load", index))
            .await
            .expect("la carga entra");
    }
    view.rebuild_advisory(&tenant(), &vulnerability())
        .await
        .expect("la vista corre");

    let from_view = Arc::new(SurrealBlastRadius::with_view(
        graph.clone(),
        view.clone(),
        tenant(),
    ));
    let from_walk = SurrealBlastRadius::new(graph.clone(), tenant());

    // Con la vista al dia: LO MISMO que el recorrido.
    let served = from_view
        .impacted("acme", &vulnerability().to_string())
        .await
        .expect("el consumidor corre");
    let walked = from_walk
        .impacted("acme", &vulnerability().to_string())
        .await
        .expect("el recorrido corre");
    let served_set: std::collections::BTreeSet<_> = served
        .iter()
        .map(|one| (one.subject.clone(), one.depth))
        .collect();
    let walked_set: std::collections::BTreeSet<_> = walked
        .iter()
        .map(|one| (one.subject.clone(), one.depth))
        .collect();
    assert_eq!(served_set, walked_set, "servir de la vista es servir el recorrido");

    // Y el advisory de otra cuenta sigue siendo una puerta cerrada.
    let foreign = Hrn::from_string(
        "hrn:hodei:artifacts:eu-west-1:mallory:Vulnerability/GHSA-view-0001",
    )
    .expect("un HRN");
    assert!(
        from_view
            .impacted("mallory", &foreign.to_string())
            .await
            .is_err(),
        "el consumidor no cruza cuentas"
    );

    // La vista se queda atras: el consumidor responde lo que el
    // recorrido dice AHORA, no lo que la vista decia antes.
    graph
        .apply(the_contains("evt-late", 7))
        .await
        .expect("el apoyo nuevo entra");
    let served = from_view
        .impacted("acme", &vulnerability().to_string())
        .await
        .expect("el consumidor corre con la vista atrasada");
    assert!(
        served
            .iter()
            .any(|one| one.subject == artifact(7).to_string()),
        "el fallback alcanza el sujeto nuevo: {:?}",
        served
            .iter()
            .map(|one| one.subject.clone())
            .collect::<Vec<_>>()
    );
}
