//! Los deltas de las relaciones, y quien los consume.
//!
//! La conformidad del registro corre LOS MISMOS escenarios contra el
//! registro en memoria y contra el de SurrealDB: un puerto, dos
//! materiales. Y la emision por el grafo dice el delta correcto para
//! cada apoyo que se anade, se retira o se repite -- lo que no cambia el
//! significado de una relacion no produce trabajo.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use hodei_evidence::domain::delta::{
    RelationDelta, RelationDeltaKind, RelationDeltaLedger, RelationDeltaSubscription,
};
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal, NodeKind, RelationType,
};
use hodei_evidence::ports::graph::GraphWriter;
use hodei_evidence::{EvidenceId, InMemoryRelationDeltaLedger};
use hodei_evidence_storage::surreal::delta::SurrealRelationDeltaLedger;
use hodei_evidence_storage::surreal::graph::SurrealGraphStore;
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;

fn tenant_a() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/acme").expect("un HRN")
}

fn tenant_b() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:other:Artifact/other").expect("un HRN")
}

fn component() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-vuln").expect("un HRN")
}

fn vulnerability() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-delta-0001")
        .expect("un HRN")
}

fn moment() -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800, 0)
        .single()
        .expect("un momento")
}

/// El delta de un apoyo anadido.
fn an_added_delta(
    tenant: &Hrn,
    evidence: &str,
    cause: &str,
    relation: RelationType,
) -> RelationDelta {
    RelationDelta {
        kind: RelationDeltaKind::Added,
        tenant: tenant.clone(),
        relation,
        from: component(),
        to: vulnerability(),
        evidence: EvidenceId::new(evidence),
        cause: cause.to_string(),
        observed_at: moment(),
    }
}

/// La conformidad del registro: los mismos hechos, en el mismo orden,
/// con las mismas claves, en el material que sea.
async fn the_ledger_conforms(ledger: Arc<dyn RelationDeltaLedger>) {
    // Dos cambios, dos causas, dos versiones.
    let first = an_added_delta(&tenant_a(), "evd-1", "evt-1", RelationType::AffectedBy);
    let second = an_added_delta(&tenant_a(), "evd-2", "evt-2", RelationType::AffectedBy);
    let version = ledger
        .append(&[first.clone(), second.clone()])
        .await
        .expect("el append corre");
    assert_eq!(version, Some(2), "el ultimo delta llevo la version 2");

    // El mismo cause, dicho otra vez: UN cambio, no dos.
    let again = ledger
        .append(&[first.clone(), second.clone()])
        .await
        .expect("el append corre");
    assert_eq!(again, None, "lo ya dicho no consume version: {again:?}");
    let (pending, stopped) = ledger
        .pending(
            &RelationDeltaSubscription::new(tenant_a(), [RelationType::AffectedBy]),
            0,
            10,
        )
        .await
        .expect("la lectura corre");
    assert_eq!(pending.len(), 2, "dos deltas, ni uno mas: {pending:?}");
    assert_eq!(stopped, 2, "el lector paro donde estaba el ultimo");

    // La version de la historia del tenant.
    let version = ledger
        .version_of(&tenant_a())
        .await
        .expect("la version corre");
    assert_eq!(version, 2);

    // La suscripcion cierra: otro tenant y otro tipo de relacion no son
    // suyos.
    let foreign = an_added_delta(&tenant_b(), "evd-3", "evt-3", RelationType::AffectedBy);
    let other_kind = an_added_delta(&tenant_a(), "evd-4", "evt-4", RelationType::Contains);
    ledger
        .append(&[foreign, other_kind])
        .await
        .expect("el append corre");

    let (mine, _) = ledger
        .pending(
            &RelationDeltaSubscription::new(tenant_a(), [RelationType::AffectedBy]),
            0,
            10,
        )
        .await
        .expect("la lectura corre");
    assert!(
        mine.iter()
            .all(|(_version, delta)| delta.tenant == tenant_a()
                && delta.relation == RelationType::AffectedBy),
        "solo lo suscrito entra: {mine:?}"
    );

    // Y la cuota corta: una suscripcion con cuota 1 lee UN delta y dice
    // donde paro.
    let (limited, stopped) = ledger
        .pending(
            &RelationDeltaSubscription::new(tenant_a(), [RelationType::AffectedBy]),
            0,
            1,
        )
        .await
        .expect("la lectura corre");
    assert_eq!(limited.len(), 1, "la cuota es la cuota");
    assert_eq!(stopped, limited[0].0, "y el lector sabe donde paro");

    // La suscripcion que no nombra tipos no invalida nada.
    let (nothing, stopped) = ledger
        .pending(&RelationDeltaSubscription::new(tenant_a(), []), 0, 10)
        .await
        .expect("la lectura corre");
    assert!(nothing.is_empty(), "sin tipos no hay trabajo: {nothing:?}");
    assert_eq!(stopped, 0);
}

/// La conformidad en memoria.
#[tokio::test]
async fn the_ledger_conforms_in_memory() {
    the_ledger_conforms(Arc::new(InMemoryRelationDeltaLedger::empty())).await;
}

/// La conformidad en SurrealDB: las mismas respuestas, y sobreviven al
/// reinicio.
#[tokio::test]
async fn the_ledger_conforms_in_surreal_and_survives_a_restart() {
    let tmp = tempfile::tempdir().expect("un directorio desechable");
    let path = tmp.path().join("db").to_string_lossy().to_string();

    {
        let db = Arc::new(
            Surreal::new::<RocksDb>(path.clone())
                .await
                .expect("la base abre"),
        );
        db.use_ns("hodei")
            .use_db("evidence")
            .await
            .expect("el espacio");
        the_ledger_conforms(Arc::new(SurrealRelationDeltaLedger::new(db))).await;
        // El proceso "cae": la base se deja caer aqui, y el lock del
        // fichero se suelta con ella.
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // El proceso vuelve: los deltas estan donde estaban, con su version.
    let db = Arc::new(Surreal::new::<RocksDb>(path).await.expect("la base vuelve"));
    db.use_ns("hodei")
        .use_db("evidence")
        .await
        .expect("el espacio");
    let ledger = SurrealRelationDeltaLedger::new(db);
    let (pending, _stopped) = ledger
        .pending(
            &RelationDeltaSubscription::new(tenant_a(), [RelationType::AffectedBy]),
            0,
            10,
        )
        .await
        .expect("la lectura tras el reinicio corre");
    assert_eq!(pending.len(), 2, "los deltas sobreviven al reinicio");
    assert_eq!(
        ledger
            .version_of(&tenant_a())
            .await
            .expect("la version corre"),
        2,
        "la version tambien"
    );
}

/// El changeset de una evidencia que afirma la relacion, con su causa.
fn the_claim(cause: &str, evidence: &str) -> GraphChangeSet {
    let evidence_id = EvidenceId::new(evidence);
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_node(GraphNode::new(
            component(),
            tenant_a(),
            NodeKind::Component,
            moment(),
        ))
        .with_node(GraphNode::new(
            vulnerability(),
            tenant_a(),
            NodeKind::Vulnerability,
            moment(),
        ))
        .with_node(GraphNode::new(
            hodei_evidence::application::projector::evidence_hrn(&tenant_a(), &evidence_id),
            tenant_a(),
            NodeKind::Evidence,
            moment(),
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            component(),
            vulnerability(),
            tenant_a(),
            evidence_id,
            moment(),
        ))
}

/// La retirada del apoyo de una evidencia, con su causa.
fn the_withdrawal(cause: &str, evidence: &str) -> GraphChangeSet {
    GraphChangeSet::empty()
        .with_cause(cause)
        .with_withdrawal(GraphRelationWithdrawal::new(
            RelationType::AffectedBy,
            component(),
            vulnerability(),
            tenant_a(),
            EvidenceId::new(evidence),
            moment(),
        ))
}

/// AC1 + AC3: cada cambio que altera el significado de una relacion
/// dice SU delta -- anadir, ampliar, retirar -- y lo que no altera el
/// significado no dice NADA.
#[tokio::test]
async fn the_graph_says_the_delta_of_what_really_changed() {
    let tmp = tempfile::tempdir().expect("un directorio desechable");
    let db = Arc::new(
        Surreal::new::<RocksDb>(tmp.path().join("db").to_string_lossy().to_string())
            .await
            .expect("la base abre"),
    );
    db.use_ns("hodei")
        .use_db("evidence")
        .await
        .expect("el espacio");
    let store = SurrealGraphStore::new(db.clone());
    let ledger = SurrealRelationDeltaLedger::new(db);

    // El primer apoyo: la relacion se ANADE.
    store
        .apply(the_claim("evt-1", "evd-1"))
        .await
        .expect("el primer apoyo entra");
    // El segundo apoyo sobre la misma relacion: el apoyo CAMBIA.
    store
        .apply(the_claim("evt-2", "evd-2"))
        .await
        .expect("el segundo apoyo entra");

    let (deltas, _stopped) = ledger
        .pending(
            &RelationDeltaSubscription::new(tenant_a(), [RelationType::AffectedBy]),
            0,
            10,
        )
        .await
        .expect("la lectura corre");
    let kinds: Vec<&str> = deltas.iter().map(|(_v, d)| d.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec!["added", "support_changed"],
        "el primer apoyo anade, el segundo amplia: {deltas:?}"
    );

    // El MISMO apoyo, dicho otra vez con otra causa: NO cambia el
    // significado de la relacion, y NO dice delta.
    store
        .apply(the_claim("evt-3", "evd-2"))
        .await
        .expect("la repeticion entra");
    let (deltas, _stopped) = ledger
        .pending(
            &RelationDeltaSubscription::new(tenant_a(), [RelationType::AffectedBy]),
            0,
            10,
        )
        .await
        .expect("la lectura corre");
    assert_eq!(
        deltas.len(),
        2,
        "la repeticion no produce trabajo: {deltas:?}"
    );

    // La retirada de un apoyo que estaba: se RETIRA.
    store
        .apply(the_withdrawal("evt-4", "evd-1"))
        .await
        .expect("la retirada entra");
    // Y la retirada de un apoyo que NO estaba: no cambia nada.
    store
        .apply(the_withdrawal("evt-5", "evd-9"))
        .await
        .expect("la retirada vacia entra");

    let (deltas, _stopped) = ledger
        .pending(
            &RelationDeltaSubscription::new(tenant_a(), [RelationType::AffectedBy]),
            0,
            10,
        )
        .await
        .expect("la lectura corre");
    let kinds: Vec<&str> = deltas.iter().map(|(_v, d)| d.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec!["added", "support_changed", "removed"],
        "la retirada real se dice; la vacia no: {deltas:?}"
    );
    assert_eq!(deltas[2].1.evidence, EvidenceId::new("evd-1"));
}

/// AC1 (reinicio): los deltas que el grafo dijo estan en el registro
/// despues de reabrir la base, y el consumidor sigue desde donde paro.
#[tokio::test]
async fn the_deltas_survive_a_restart_and_the_consumer_resumes() {
    let tmp = tempfile::tempdir().expect("un directorio desechable");
    let path = tmp.path().join("db").to_string_lossy().to_string();

    {
        let db = Arc::new(
            Surreal::new::<RocksDb>(path.clone())
                .await
                .expect("la base abre"),
        );
        db.use_ns("hodei")
            .use_db("evidence")
            .await
            .expect("el espacio");
        let store = SurrealGraphStore::new(db);
        store
            .apply(the_claim("evt-1", "evd-1"))
            .await
            .expect("el apoyo entra");
        store
            .apply(the_claim("evt-2", "evd-2"))
            .await
            .expect("el segundo entra");
        // El proceso "cae" al salir del bloque.
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // El proceso vuelve y el consumidor lee con su cuota: UN delta por
    // lectura, y el cursor avanza.
    let db = Arc::new(Surreal::new::<RocksDb>(path).await.expect("la base vuelve"));
    db.use_ns("hodei")
        .use_db("evidence")
        .await
        .expect("el espacio");
    let ledger = SurrealRelationDeltaLedger::new(db.clone());

    let subscription = RelationDeltaSubscription::new(tenant_a(), [RelationType::AffectedBy]);
    let (first, cursor) = ledger
        .pending(&subscription, 0, 1)
        .await
        .expect("la lectura corre");
    assert_eq!(first.len(), 1, "la cuota manda");
    let (second, cursor) = ledger
        .pending(&subscription, cursor, 1)
        .await
        .expect("la lectura corre");
    assert_eq!(second.len(), 1, "la siguiente lectura sigue donde paro");
    assert_ne!(first[0].0, second[0].0, "y no relee lo mismo");

    let (third, cursor) = ledger
        .pending(&subscription, cursor, 10)
        .await
        .expect("la lectura corre");
    assert!(third.is_empty(), "ya no queda nada por invalidar");
    assert_eq!(cursor, second[0].0, "el cursor se queda donde estaba");
}
