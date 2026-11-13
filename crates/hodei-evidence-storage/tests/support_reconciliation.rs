//! La reconciliacion de los apoyos del grafo.
//!
//! Una relacion del grafo la AFIRMA la evidencia que la respalda, y dos
//! evidencias distintas sobre la misma relacion son DOS apoyos de UNA
//! relacion, no dos relaciones. Cuando una evidencia se supersedida, su
//! apoyo se retira solo, y los demas se quedan. Cuando se pierde el
//! ultimo apoyo, la relacion deja de ser una afirmacion: lo que queda es
//! UNKNOWN, nunca falsa seguridad.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal, NodeKind, RelationType,
};
use hodei_evidence::ports::graph::GraphWriter;
use hodei_evidence::{
    EventEnvelope, EventId, EventType, EvidenceGraphProjector, EvidenceId, EvidencePayload,
    GraphProjector, RelationClaim,
};
use hodei_evidence_storage::surreal::graph::{RelationRow, SurrealGraphStore};
use kernel::Hrn;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

fn tenant() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/acme").expect("un HRN")
}

fn component() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-vuln").expect("un HRN")
}

fn vulnerability() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-recon-0001")
        .expect("un HRN")
}

fn moment() -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800, 0).single().expect("un momento")
}

/// El changeset de una evidencia que afirma que el componente esta
/// afectado por la vulnerabilidad: la relacion que el radio de impacto
/// recorre.
fn the_claim(evidence: &str) -> GraphChangeSet {
    let evidence_id = EvidenceId::new(evidence);
    GraphChangeSet::empty()
        .with_node(GraphNode::new(component(), tenant(), NodeKind::Component, moment()))
        .with_node(GraphNode::new(vulnerability(), tenant(), NodeKind::Vulnerability, moment()))
        .with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(&tenant(), &evidence_id),
            tenant(),
            NodeKind::Evidence,
            moment(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            component(),
            vulnerability(),
            tenant(),
            evidence_id,
            moment(),
        ))
}

/// La retirada del apoyo de una evidencia sobre la misma relacion.
fn the_withdrawal(evidence: &str) -> GraphChangeSet {
    GraphChangeSet::empty().with_withdrawal(GraphRelationWithdrawal::new(
        RelationType::AffectedBy,
        component(),
        vulnerability(),
        tenant(),
        EvidenceId::new(evidence),
        moment(),
    ))
}

/// La identidad de la relacion que afirman los changesets.
fn the_identity() -> String {
    format!(
        "{}|{}|{}",
        RelationType::AffectedBy.as_str(),
        component(),
        vulnerability()
    )
}

/// El store sobre una base desechable.
async fn the_store() -> (tempfile::TempDir, SurrealGraphStore<Db>) {
    let tmp = tempfile::tempdir().expect("un directorio de usar y tirar");
    let db = Arc::new(
        Surreal::new::<RocksDb>(tmp.path().join("db").to_string_lossy().to_string())
            .await
            .expect("la base de datos abre"),
    );
    db.use_ns("hodei").use_db("evidence").await.expect("el espacio");
    (tmp, SurrealGraphStore::new(db))
}

/// AC1: dos evidencias sobre la misma relacion son dos apoyos
/// verificables de UNA relacion, sin duplicar la relacion.
#[tokio::test]
async fn two_evidences_are_two_supports_of_one_relation() {
    let (_tmp, store) = the_store().await;

    store.apply(the_claim("evd-1")).await.expect("el primer apoyo entra");
    store.apply(the_claim("evd-2")).await.expect("el segundo apoyo entra");

    // UNA fila de relacion: la identidad de la relacion no se duplica.
    let mut response = store
        .db()
        .query("SELECT * FROM type::table($table) WHERE tenant_hrn = $tenant")
        .bind(("table", store.relation_table_name()))
        .bind(("tenant", tenant().to_string()))
        .await
        .expect("la lectura corre");
    let rows: Vec<RelationRow> = response.take(0).expect("las filas");
    assert_eq!(rows.len(), 1, "una relacion, no dos: {rows:?}");

    // Y DOS apoyos verificables, con su procedencia.
    let supports = store.supports_of(&the_identity()).await.expect("los apoyos");
    let evidence: Vec<&str> = supports.iter().map(|one| one.0.as_str()).collect();
    assert!(
        evidence.contains(&"evd-1") && evidence.contains(&"evd-2"),
        "los dos apoyos estan: {evidence:?}"
    );
}

/// AC2: al retirarse el apoyo de la evidencia supersedida, el otro apoyo
/// vigente se queda, y la relacion sigue consultable.
#[tokio::test]
async fn a_superseded_evidence_withdraws_only_its_support() {
    let (_tmp, store) = the_store().await;

    store.apply(the_claim("evd-1")).await.expect("el primer apoyo entra");
    store.apply(the_claim("evd-2")).await.expect("el segundo apoyo entra");
    store.apply(the_withdrawal("evd-1")).await.expect("la retirada entra");

    let supports = store.supports_of(&the_identity()).await.expect("los apoyos");
    let evidence: Vec<&str> = supports.iter().map(|one| one.0.as_str()).collect();
    assert_eq!(evidence, vec!["evd-2"], "solo queda el apoyo vigente: {evidence:?}");

    // La relacion SIGUE siendo una afirmacion valida, y el radio de
    // impacto la sigue viendo.
    let radius = hodei_risk_storage::blast_radius(&store, &tenant(), &vulnerability())
        .await
        .expect("el radio corre");
    assert!(
        radius
            .affected_components
            .contains(&component().to_string()),
        "la relacion sigue consultable con el apoyo que queda: {:?}",
        radius.affected_components
    );
}

/// AC3: al perder el ultimo apoyo, la relacion deja de ser una
/// afirmacion valida y lo que queda es UNKNOWN, nunca falsa seguridad.
#[tokio::test]
async fn losing_the_last_support_exposes_unknown() {
    let (_tmp, store) = the_store().await;

    store.apply(the_claim("evd-1")).await.expect("el apoyo entra");
    store.apply(the_withdrawal("evd-1")).await.expect("la retirada entra");

    let supports = store.supports_of(&the_identity()).await.expect("los apoyos");
    assert!(supports.is_empty(), "no queda apoyo alguno: {supports:?}");

    // La fila SIGUE en el grafo, con su linaje, y dice UNKNOWN: el
    // vacio de apoyos no se disfraza de afirmacion.
    let mut response = store
        .db()
        .query("SELECT * FROM type::table($table) WHERE tenant_hrn = $tenant")
        .bind(("table", store.relation_table_name()))
        .bind(("tenant", tenant().to_string()))
        .await
        .expect("la lectura corre");
    let rows: Vec<RelationRow> = response.take(0).expect("las filas");
    assert_eq!(rows.len(), 1, "la fila queda, con su linaje");
    assert_eq!(rows[0].status, "unknown", "lo que queda es UNKNOWN: {rows:?}");

    // Y el radio de impacto NO la usa: una relacion sin apoyo no es una
    // afirmacion.
    let radius = hodei_risk_storage::blast_radius(&store, &tenant(), &vulnerability())
        .await
        .expect("el radio corre");
    assert!(
        !radius
            .affected_components
            .contains(&component().to_string()),
        "sin apoyo no hay afirmacion: {:?}",
        radius.affected_components
    );
}

/// AC4: el replay del ledger por el proyector reconstruye el mismo
/// estado que la aplicacion directa de los changesets: los mismos
/// apoyos, el mismo linaje.
#[tokio::test]
async fn the_replay_of_the_ledger_rebuilds_the_same_state() {
    let projector = EvidenceGraphProjector::new();

    let claim = RelationClaim {
        relation: RelationType::AffectedBy,
        from: component(),
        to: vulnerability(),
        from_kind: NodeKind::Component,
        to_kind: NodeKind::Vulnerability,
    };
    let accepted = |id: &str, ev: u64| {
        EventEnvelope::builder(EventId::new(format!("evt-{ev}")), tenant(), EventType::EvidenceAccepted)
            .stream("tenant-acme", ev)
            .occurred_at(moment())
            .recorded_at(moment())
            .typed_payload(EvidencePayload {
                evidence_id: EvidenceId::new(id),
                tenant_hrn: tenant(),
                content_digest: "sha256:aa".to_string(),
                supersedes: None,
                relation_claim: Some(claim.clone()),
            })
            .expect("el payload serializa")
            .build()
    };
    let superseded = EventEnvelope::builder(EventId::new("evt-3"), tenant(), EventType::EvidenceSuperseded)
        .stream("tenant-acme", 3)
        .occurred_at(moment())
        .recorded_at(moment())
        .typed_payload(EvidencePayload {
            evidence_id: EvidenceId::new("evd-2"),
            tenant_hrn: tenant(),
            content_digest: "sha256:bb".to_string(),
            supersedes: Some(EvidenceId::new("evd-1")),
            relation_claim: Some(claim.clone()),
        })
        .expect("el payload serializa")
        .build();

    // La REPRODUCCION: los eventos del ledger, por el proyector, al grafo.
    let (_tmp, replayed) = the_store().await;
    for event in [accepted("evd-1", 1), accepted("evd-2", 2), superseded] {
        let changes = projector.project(&event).expect("el evento proyecta");
        replayed.apply(changes).await.expect("el changeset entra");
    }

    // La APLICACION directa: los mismos hechos, dichos a mano.
    let (_tmp2, direct) = the_store().await;
    direct.apply(the_claim("evd-1")).await.expect("entra");
    direct.apply(the_claim("evd-2")).await.expect("entra");
    direct.apply(the_withdrawal("evd-1")).await.expect("entra");

    // El MISMO estado: los mismos apoyos, el mismo linaje.
    let replayed_supports = replayed.supports_of(&the_identity()).await.expect("los apoyos");
    let direct_supports = direct.supports_of(&the_identity()).await.expect("los apoyos");
    assert_eq!(
        replayed_supports, direct_supports,
        "el replay reconstruye los mismos apoyos"
    );

    let mut response = replayed
        .db()
        .query("SELECT * FROM type::table($table) WHERE tenant_hrn = $tenant")
        .bind(("table", replayed.relation_table_name()))
        .bind(("tenant", tenant().to_string()))
        .await
        .expect("la lectura corre");
    let rows: Vec<RelationRow> = response.take(0).expect("las filas");
    assert_eq!(rows[0].status, "asserted", "la relacion sigue afirmada");
}

/// AC4 (compatibilidad): una fila escrita por el formato ANTERIOR -- sin
/// status y sin apoyos -- sigue siendo una relacion afirmada y sigue
/// siendo traversable. Los datos ya persistidos no se rompen.
#[tokio::test]
async fn an_old_row_without_status_is_still_an_asserted_relation() {
    let (_tmp, store) = the_store().await;

    // La fila del formato anterior: sin status, sin apoyos, con su
    // evidence_id de siempre.
    let identity = the_identity();
    let old_row = serde_json::json!({
        "relation_id": identity,
        "tenant_hrn": tenant().to_string(),
        "relation_type": RelationType::AffectedBy.as_str(),
        "from": component().to_string(),
        "to": vulnerability().to_string(),
        "evidence_id": "evd-old",
        "observed_at": moment().to_rfc3339(),
    });
    store
        .db()
        .query("UPSERT $id CONTENT $row")
        .bind((
            "id",
            surrealdb::sql::Thing::from((
                store.relation_table_name(),
                hodei_evidence_storage::surreal::graph::escape_id(&identity),
            )),
        ))
        .bind(("row", old_row))
        .await
        .expect("la fila vieja entra")
        .check()
        .expect("la fila vieja se escribe");

    // Se lee como una relacion afirmada: la compatibilidad no es una
    // migracion silenciosa, es una lectura que funciona.
    let mut response = store
        .db()
        .query("SELECT * FROM type::table($table) WHERE tenant_hrn = $tenant")
        .bind(("table", store.relation_table_name()))
        .bind(("tenant", tenant().to_string()))
        .await
        .expect("la lectura corre");
    let rows: Vec<RelationRow> = response.take(0).expect("las filas");
    assert_eq!(rows.len(), 1, "la fila vieja esta");

    // Y el radio de impacto la traversa: los datos ya persistidos
    // siguen respondiendo las preguntas de siempre.
    let radius = hodei_risk_storage::blast_radius(&store, &tenant(), &vulnerability())
        .await
        .expect("el radio corre");
    assert!(
        radius
            .affected_components
            .contains(&component().to_string()),
        "la fila vieja sigue siendo una afirmacion: {:?}",
        radius.affected_components
    );
}
