//! El SLA de ingesta de la inteligencia de vulnerabilidades (HSP-155 AC4).
//!
//! La tarea pide medir con numeros el retardo entre que el feed entrega
//! una pagina y que el caso de seguridad queda persistido, y que la
//! reevaluacion es incremental: que no se abre un caso global cuando solo
//! cambio un advisory. Se mide aqui, en una base de datos local efimera
//! (RocksDb desechable), sin tocar el alpha Dgraph compartido de la stack.
//!
//! El numero que importa no es el absolutode una maquina concreta, sino la
//! forma de la curva: una sincronizacion que reevalua lo que cambio abre
//! SOLO los casos de los advisories modificados, no uno por cada advisory
//! del feed. Eso es lo que el test puede fallar.

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use hodei_reactive::reactions::{ImpactedSubject, SecurityCaseSink};
use hodei_risk::affected::DependencyKind;
use hodei_risk::feed::{FeedPage, RiskFeed, WatchedComponent, reassess};
use hodei_risk_storage::reactions::SurrealSecurityCaseStore;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

fn clock() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
        .expect("a clock")
        .with_timezone(&Utc)
}

/// Un advisory de un paquete Maven, con el id y el paquete pedidos.
fn advisory(id: &str, package: &str) -> String {
    format!(
        r#"{{"id": "{id}", "modified": "2026-10-01T00:00:00Z", "affected": [
          {{"package": {{"ecosystem": "Maven", "name": "{package}"}},
           "ranges": [{{"type": "ECOSYSTEM", "events": [{{"introduced": "0"}}, {{"fixed": "9.0.0"}}]}}]
          }}] }}"#
    )
}

async fn cases() -> (tempfile::TempDir, SurrealSecurityCaseStore<Db>) {
    let tmp = tempfile::tempdir().expect("a disposable directory");
    let db = Arc::new(
        Surreal::new::<RocksDb>(tmp.path().join("db").to_string_lossy().to_string())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei")
        .use_db("intelligence")
        .await
        .expect("the namespace");
    (tmp, SurrealSecurityCaseStore::new(db))
}

/// El ingesta de una pagina grande deja el feed lleno y nada de casos, y
/// el retardo entre la pagina y el primer caso se mide en milisegundos.
#[tokio::test]
async fn the_ingest_lag_is_measured_and_the_cases_are_persisted_incrementally() {
    let (_tmp, store) = cases().await;
    let mut feed = RiskFeed::new(chrono::Duration::hours(6));

    // La carga inicial: cien advisories. Nada de esto debe abrir casos
    // todavia porque nadie miro el inventario.
    let documents: Vec<String> = (0..100)
        .map(|index| {
            advisory(
                &format!("GHSA-hodei-{index:04}.1"),
                &format!("org.example:p-{index}"),
            )
        })
        .collect();
    let bulk_started = Instant::now();
    let bulk = feed.apply_page(
        Ok(FeedPage {
            source: "osv".to_string(),
            fetched_at: clock(),
            documents,
        }),
        clock(),
    );
    let bulk_seconds = bulk_started.elapsed();
    assert_eq!(bulk.inserted, 100);

    // El inventario watched: los mismos cien paquetes, uno cada uno.
    let components: Vec<WatchedComponent> = (0..100)
        .map(|index| WatchedComponent {
            package: format!("org.example:p-{index}"),
            version: Some("1.0.0".to_string()),
            dependency: DependencyKind::Direct,
        })
        .collect();

    // Un advisory NUEVO llega. La reevaluacion es incremental: solo ese
    // advisory se compara.
    let new_id = "GHSA-hodei-new.1";
    let report = feed.apply_page(
        Ok(FeedPage {
            source: "osv".to_string(),
            fetched_at: clock() + chrono::Duration::seconds(1),
            documents: vec![advisory(new_id, "org.example:p-50")],
        }),
        clock() + chrono::Duration::seconds(1),
    );
    assert_eq!(report.inserted, 1);
    assert_eq!(report.changed_ids, vec![new_id.to_string()]);

    let reassess_started = Instant::now();
    let round = reassess(&feed, &report.changed_ids, &components);
    let reassess_time = reassess_started.elapsed();
    assert_eq!(
        round.advisories_matched, 1,
        "the incremental rule: one advisory"
    );
    assert_eq!(round.advisories_held, 101);
    assert!(round.is_incremental());

    // Los casos se persisten SOLO de los advisories tocados por esta
    // pagina. Un caso por cada advisory changed, no uno por cada advisory
    // del feed.
    let persist_started = Instant::now();
    let mut opened = Vec::new();
    for advisory_id in round.touched_advisories() {
        let impacted = vec![ImpactedSubject {
            subject: "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0".to_string(),
            deployment: None,
            depth: 1,
        }];
        let id = store
            .open_or_update(&advisory_id, &impacted, clock())
            .await
            .expect("the case opens");
        opened.push(id);
    }
    let persist_time = persist_started.elapsed();

    assert_eq!(opened.len(), 1, "one changed advisory opens one case");

    // La re-ingesta del mismo advisory no abre un segundo caso.
    let again = store
        .open_or_update(new_id, &[], clock())
        .await
        .expect("the idempotent open");
    assert_eq!(again, opened[0], "a case opened twice is the same case");

    // Y el numero pedido por la tarea: el retardo total entre la pagina y
    // el caso persistido.
    let get = store.get(&opened[0]).await.expect("read");
    assert!(get.is_some(), "the case is persisted");

    println!(
        "HSP-155 AC4 storage measured: bulk_sync={bulk_seconds:?} reassess={reassess_time:?} persist_one_case={persist_time:?} opened_cases={}",
        opened.len()
    );
}
