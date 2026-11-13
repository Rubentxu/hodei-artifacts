//! The atomicity of the projection: a changeset goes in WHOLE or not at
//! all.
//!
//! A projection that fails halfway leaves a graph that looks valid and is
//! not: nodes without their relations, a checkpoint that would hide the
//! pending event. The scenarios here are the ones the contract asks for:
//! the injected failure is a changeset whose relation is refused, and the
//! store has to answer for what it wrote before saying no.

use std::sync::Arc;

use chrono::Utc;
use hodei_evidence::domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationType,
};
use hodei_evidence::{GraphQuery, GraphWriter};
use hodei_evidence_storage::SurrealGraphStore;
use kernel::Hrn;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

/// La cuenta de la UAT.
fn account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/acme").expect("un HRN")
}

/// Un HRN de la cuenta, del tipo que se pide.
fn hrn(kind: &str, id: &str) -> Hrn {
    Hrn::from_string(&format!("hrn:hodei:artifacts:eu-west-1:acme:{kind}/{id}"))
        .expect("un HRN")
}

/// El almacén sobre una base desechable.
async fn store() -> (tempfile::TempDir, SurrealGraphStore<Db>) {
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
    (tmp, SurrealGraphStore::new(db))
}

/// El changeset de la UAT: un artefacto, su componente y una relación
/// Contains que dice qué evidencia la sostiene. Con `forged_evidence`,
/// la relación dice una evidencia que no está y tiene que ser rechazada.
fn the_changeset(forged_evidence: bool) -> GraphChangeSet {
    let evidence = if forged_evidence {
        hodei_evidence::EvidenceId::new("evd-does-not-exist")
    } else {
        hodei_evidence::EvidenceId::new("evd-1")
    };
    let now = Utc::now();
    let artifact = hrn("Artifact", "app-1.0.0");
    let component = hrn("Component", "lib-vuln");
    let mut changes = GraphChangeSet::empty()
        .with_node(GraphNode::new(artifact.clone(), account(), NodeKind::Artifact, now))
        .with_node(GraphNode::new(component.clone(), account(), NodeKind::Component, now))
        .with_relation(GraphRelation::new(
            RelationType::Contains,
            artifact,
            component,
            account(),
            evidence,
            now,
        ));
    if !forged_evidence {
        changes = changes.with_node(GraphNode::new(
            hodei_evidence::evidence_hrn(&account(), &hodei_evidence::EvidenceId::new("evd-1")),
            account(),
            NodeKind::Evidence,
            now,
        ));
    }
    changes
}

/// El conjunto de nodos que hay, dicho de forma comparable.
async fn the_nodes(graph: &SurrealGraphStore<Db>) -> Vec<String> {
    let mut seen = Vec::new();
    for kind in [NodeKind::Artifact, NodeKind::Component, NodeKind::Evidence] {
        let name = kind.as_str().to_string();
        for node in graph.nodes(&account(), kind).await.expect("los nodos se leen") {
            seen.push(format!("{}|{}", name, node.id));
        }
    }
    seen.sort();
    seen
}/// Un changeset rechazado no deja NADA escrito: ni el primer nodo, ni el
/// segundo, ni una parte de lo que venia. Un grafo con la mitad de un
/// changeset es un grafo que parece valido y no lo es.
#[tokio::test]
async fn a_rejected_changeset_leaves_nothing_written() {
    let (_tmp, graph) = store().await;

    let outcome = graph.apply(the_changeset(true)).await;
    assert!(
        outcome.is_err(),
        "la relacion con evidencia forjada se rechaza: {outcome:?}"
    );

    let nodes = the_nodes(&graph).await;
    assert!(
        nodes.is_empty(),
        "un changeset rechazado no deja nodos: {nodes:?}"
    );
}

/// El mismo changeset aplica entero cuando la evidencia esta: los nodos,
/// la relacion y el soporte quedan.
#[tokio::test]
async fn the_whole_changeset_applies_when_the_evidence_is_there() {
    let (_tmp, graph) = store().await;

    graph
        .apply(the_changeset(false))
        .await
        .expect("el changeset con su evidencia aplica");

    let nodes = the_nodes(&graph).await;
    assert_eq!(nodes.len(), 3, "dos nodos y su evidencia: {nodes:?}");

    let relations = graph
        .relations_from(&account(), &hrn("Artifact", "app-1.0.0"))
        .await
        .expect("las relaciones se leen");
    assert_eq!(relations.len(), 1, "la Contains queda");
}

/// Un fallo entre aplicar un changeset y avanzar el checkpoint deja el
/// mismo estado semantico al reintentar: el reintento re-proyecta el
/// mismo changeset y el grafo acaba igual, sin duplicados y sin saltarse
/// nada. Es lo que hace el projector cuando el checkpoint no se movio.
#[tokio::test]
async fn a_reprojection_after_a_crash_converges_to_the_same_state() {
    let (_tmp, graph) = store().await;

    graph
        .apply(the_changeset(false))
        .await
        .expect("el primer intento aplica");
    let before = the_nodes(&graph).await;

    // El reintento: el mismo changeset, porque el checkpoint no se movio.
    graph
        .apply(the_changeset(false))
        .await
        .expect("el reintento re-proyecta lo mismo");
    let after = the_nodes(&graph).await;

    assert_eq!(before, after, "el reintento no cambia el estado semantico");
}

/// Dos changesets aplicados a la vez no se pierden filas entre ellos:
/// la union es exacta y ninguna escritura desaparece.
#[tokio::test]
async fn two_concurrent_applies_never_lose_a_row() {
    let (_tmp, graph) = store().await;
    let graph = Arc::new(graph);

    let first = {
        let evidence = hodei_evidence::EvidenceId::new("evd-a");
        let now = Utc::now();
        let component = hrn("Component", "lib-a");
        GraphChangeSet::empty()
            .with_node(GraphNode::new(component.clone(), account(), NodeKind::Component, now))
            .with_node(
                GraphNode::new(
                    hodei_evidence::evidence_hrn(&account(), &evidence),
                    account(),
                    NodeKind::Evidence,
                    now,
                ),
            )
            .with_relation(GraphRelation::new(
                RelationType::Contains,
                hrn("Artifact", "app-1"),
                component,
                account(),
                evidence,
                now,
            ))
    };
    let second = {
        let evidence = hodei_evidence::EvidenceId::new("evd-b");
        let now = Utc::now();
        let component = hrn("Component", "lib-b");
        GraphChangeSet::empty()
            .with_node(GraphNode::new(component.clone(), account(), NodeKind::Component, now))
            .with_node(
                GraphNode::new(
                    hodei_evidence::evidence_hrn(&account(), &evidence),
                    account(),
                    NodeKind::Evidence,
                    now,
                ),
            )
            .with_relation(GraphRelation::new(
                RelationType::Contains,
                hrn("Artifact", "app-2"),
                component,
                account(),
                evidence,
                now,
            ))
    };

    let (left, right) = tokio::join!(
        graph.apply(first),
        graph.apply(second),
    );
    left.expect("el primero aplica");
    right.expect("el segundo aplica");

    let components = graph
        .nodes(&account(), NodeKind::Component)
        .await
        .expect("los componentes se leen");
    assert_eq!(components.len(), 2, "ningun componente se pierde");
}
