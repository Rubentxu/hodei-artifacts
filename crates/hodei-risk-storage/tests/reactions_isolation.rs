//! The isolation of the reactions by account and canonical HRN.
//!
//! A vulnerability or a reaction of ANOTHER account never produces impact
//! over the current one, and an identity error is detected, not turned
//! into an empty answer. The scenarios are adversarial: they ask for what
//! is not theirs and they expect a closed door, not silence.

use std::sync::Arc;

use chrono::Utc;
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType,
};
use hodei_evidence::GraphWriter;
use hodei_evidence::EvidenceId;
use hodei_evidence_storage::SurrealGraphStore;
use hodei_reactive::reactions::BlastRadiusSource;
use hodei_risk_storage::reactions::SurrealBlastRadius;
use kernel::Hrn;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

/// La cuenta propia: la del adaptador, la que su tenant lleva.
const ACCOUNT: &str = "acme";

/// La cuenta ajena que intenta mirar lo que no es suyo.
const FOREIGN: &str = "other";

/// El advisory propio, el de la cuenta de la UAT.
const ADVISORY: &str = "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-iso-0001";

/// El advisory que pertenece a la cuenta ajena.
const FOREIGN_ADVISORY: &str = "hrn:hodei:artifacts:eu-west-1:other:Vulnerability/GHSA-iso-0002";

/// El sujeto que la vulnerabilidad alcanza.
const SUBJECT: &str = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0";

/// El tenant del adaptador, como HRN canonico.
fn tenant() -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Artifact/{ACCOUNT}"
    ))
    .expect("un HRN")
}

/// El mundo: una base de datos desechable con la capa de riesgo de UNA
/// cuenta, y el radio de impacto montado sobre ella.
async fn world() -> (tempfile::TempDir, SurrealBlastRadius<Db>) {
    let tmp = tempfile::tempdir().expect("un directorio desechable");
    let db = Arc::new(
        Surreal::new::<RocksDb>(tmp.path().join("db").to_string_lossy().to_string())
            .await
            .expect("la base de datos abre"),
    );
    db.use_ns("hodei")
        .use_db("evidence")
        .await
        .expect("el espacio de nombres");

    let graph = Arc::new(SurrealGraphStore::new(db.clone()));
    seed_the_risk_layer(&graph).await;

    (tmp, SurrealBlastRadius::new(graph, tenant()))
}

/// La capa de riesgo de la cuenta propia: el artefacto, su componente, la
/// vulnerabilidad y el despliegue. Es la misma forma que la UAT de M6.
async fn seed_the_risk_layer(graph: &SurrealGraphStore<Db>) {
    let evidence = EvidenceId::new("ev-iso-1");
    let now = Utc::now();
    let account = tenant();
    let subject = Hrn::from_string(SUBJECT).expect("un HRN");
    let component = Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Component/lib-vuln"
    ))
    .expect("un HRN");
    let deployment = Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Deployment/production"
    ))
    .expect("un HRN");
    let advisory = Hrn::from_string(ADVISORY).expect("un HRN");

    let changes = GraphChangeSet::empty()
        .with_node(GraphNode::new(subject.clone(), account.clone(), NodeKind::Artifact, now))
        .with_node(GraphNode::new(component.clone(), account.clone(), NodeKind::Component, now))
        .with_node(GraphNode::new(advisory.clone(), account.clone(), NodeKind::Vulnerability, now))
        .with_node(GraphNode::new(deployment.clone(), account.clone(), NodeKind::Deployment, now))
        .with_node(
            GraphNode::new(
                hodei_evidence::application::projector::evidence_hrn(&account, &evidence),
                account.clone(),
                NodeKind::Evidence,
                now,
            ),
        )
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            subject.clone(),
            component,
            account.clone(),
            evidence.clone(),
            now,
        ))
        .with_relation(GraphRelation::new(
            RelationType::AffectedBy,
            Hrn::from_string(&format!(
                "hrn:hodei:artifacts:eu-west-1:{ACCOUNT}:Component/lib-vuln"
            ))
            .expect("un HRN"),
            advisory,
            account.clone(),
            evidence.clone(),
            now,
        ))
        .with_relation(GraphRelation::new(
            RelationType::DeployedAs,
            subject,
            deployment,
            account,
            evidence,
            now,
        ));

    graph.apply(changes).await.expect("la capa de riesgo se escribe");
}

/// La misma CVE en la cuenta autorizada conserva el resultado: el sujeto
/// de la cuenta sale, con su profundidad. Es la garantia de M6.
#[tokio::test]
async fn the_own_account_keeps_the_result() {
    let (_tmp, radius) = world().await;

    let subjects = radius
        .impacted(ACCOUNT, ADVISORY)
        .await
        .expect("la cuenta propia consulta su propia vulnerabilidad");

    assert!(
        subjects.iter().any(|one| one.subject == SUBJECT),
        "el sujeto de la cuenta tiene que estar en el radio: {subjects:?}"
    );
}

/// La misma solicitud desde una cuenta ajena es una puerta cerrada: un
/// error explicito, y no un vec vacio ni un resultado.
#[tokio::test]
async fn a_foreign_account_is_a_closed_door() {
    let (_tmp, radius) = world().await;

    let outcome = radius.impacted(FOREIGN, ADVISORY).await;

    let error = outcome.expect_err("una cuenta ajena no consulta lo que no es suyo");
    assert!(
        error.contains(FOREIGN) && error.contains(ACCOUNT),
        "el error nombra las dos cuentas: {error}"
    );
}

/// Una solicitud sin cuenta no se interpreta como nadie ni como todos:
/// se rechaza.
#[tokio::test]
async fn an_empty_account_is_rejected() {
    let (_tmp, radius) = world().await;

    let outcome = radius.impacted("", ADVISORY).await;

    assert!(
        outcome.is_err(),
        "sin cuenta no hay consulta: {outcome:?}"
    );
}

/// Un advisory malformado es un ERROR, no un impacto cero: un vec vacio
/// y un error se leen igual desde fuera y solo uno de los dos pide
///mirar de nuevo.
#[tokio::test]
async fn a_malformed_advisory_is_an_error_not_silence() {
    let (_tmp, radius) = world().await;

    let outcome = radius.impacted(ACCOUNT, "GHSA-m6-0001").await;

    let error = outcome.expect_err("un advisory malformado no es impacto cero");
    assert!(
        error.contains("canonical") || error.contains("HRN") || error.contains("hrn"),
        "el error dice que el advisory no es un HRN canonico: {error}"
    );
}

/// Un advisory que pertenece a OTRA cuenta se rechaza aunque la cuenta
/// pedida sea la propia: el HRN del advisory y el tenant tienen que
/// coincidir, y no hay nada que mirar si no coinciden.
#[tokio::test]
async fn an_advisory_of_another_account_is_rejected() {
    let (_tmp, radius) = world().await;

    let outcome = radius.impacted(ACCOUNT, FOREIGN_ADVISORY).await;

    let error = outcome.expect_err("el advisory de otra cuenta no se consulta");
    assert!(
        error.contains("other"),
        "el error nombra la cuenta del advisory: {error}"
    );
}

/// Y la cuenta ajena con su propio advisory tambien: ninguna combinacion
/// de cuenta ajena abre el grafo del tenant.
#[tokio::test]
async fn a_foreign_account_with_its_own_advisory_is_rejected_too() {
    let (_tmp, radius) = world().await;

    let outcome = radius.impacted(FOREIGN, FOREIGN_ADVISORY).await;

    assert!(
        outcome.is_err(),
        "el grafo del tenant no se abre a otra cuenta: {outcome:?}"
    );
}
