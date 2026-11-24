//! HSP-220: el drenaje del outbox del servidor de laboratorio compartido.
//!
//! El servidor Dgraph compartido (h186) acumula los hechos de auditoría
//! pendientes de TODAS las corridas: ninguna suite los marca entregados y
//! no hay worker desplegado que los consuma. El censo global `pending()`
//! del puerto los devuelve todos con su `AuditEvent` completo, y cuando el
//! conjunto cruza el límite de mensaje gRPC (4 MiB) la respuesta muere con
//! `OutOfRange`: el fallo probado por HSP-194 que bloqueaba las suites
//! 187/190/191/193 (estado compartido del servidor, no código).
//!
//! Esta IT es la utilidad de drenaje del laboratorio: reclama cada hecho
//! pendiente y lo marca entregado con LA MISMA mecánica que promete el
//! puerto (`claim` atómico + `mark_delivered`, hsp187), por lotes, para
//! que el servidor no guarde un conjunto pendiente global que desborde.
//! No borra ni reescribe nada: la entrega es la semántica de drenaje para
//! hechos que ningún consumidor va a releer; el historial queda en el
//! grafo como `delivered`.
//!
//! La raíz de la consulta de claves va acotada (`first:`), porque el
//! drenaje arranca justamente cuando el censo sin acotar ya no cabe. Los
//! conteos van a stdout y a `target/uat/hsp220/`; nunca los payloads.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT`, como todos los IT del stack. Una
//! sola prueba: el drenaje es una operación del servidor, no por cuenta.
//!
//! El censo por tenant (`eq(hodei.catalog.outbox.tenant, cuenta)`, índice
//! exacto) es la prevención para las suites; este drenaje es la cura del
//! servidor ya desbordado. El consumidor real del puerto
//! (`AuditDispatcher::dispatch_pending`) sigue llamando a `pending()` sin
//! acotar: si se despliega contra un servidor compartido, necesita
//! paginación — la recomendación de seguimiento vive en la evidencia.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Duration as ChronoDuration, Utc};
use hodei_audit::AuditOutbox;
use hodei_catalog_storage::DgraphAuditOutbox;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use tokio::task::JoinSet;

/// El lote por ronda: la respuesta de claves debe ser pequeña aunque el
/// servidor esté desbordado (de ahí `first:`).
const KEYS_PER_ROUND: usize = 500;

/// La concurrencia del drenaje: cada hecho cuesta varias llamadas gRPC
/// (claim + marca + lecturas) y 8 pares en vuelo no ahogan al alpha.
const DRAIN_CONCURRENCY: usize = 8;

/// Tope de rondas: un drenaje que no termina no es un drenaje.
const MAX_ROUNDS: usize = 200;

/// La ejecución: un worker distinto por corrida.
fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

/// El directorio de la evidencia: `target/uat/hsp220/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp220");
    std::fs::create_dir_all(&dir).expect("the evidence directory");
    dir
}

/// Cuántos hechos esperan entrega AHORA: los `pending` y los `claimed`
/// cuyo lease expiró, la MISMA definición que el censo global del puerto.
/// El conteo viaja acotado (un número), nunca el payload.
async fn the_waiting_count(client: &GraphClient) -> usize {
    let now = Utc::now();
    let response = client
        .execute_dql(&format!(
            "{{ p(func: eq(hodei.catalog.outbox.state, \"pending\")) {{ count(uid) }} \
             c(func: eq(hodei.catalog.outbox.state, \"claimed\")) \
             @filter(le(hodei.catalog.outbox.lease_until, \"{now}\")) {{ count(uid) }} }}"
        ))
        .await
        .expect("the waiting count answers");
    let pending = response
        .json
        .pointer("/p/0/count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let expired = response
        .json
        .pointer("/c/0/count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    (pending + expired) as usize
}

/// Las claves del siguiente lote que espera entrega, con la misma
/// definición que `pending()`. La raíz va acotada (`first:`) para que la
/// respuesta sea pequeña aunque el servidor esté desbordado.
async fn the_next_waiting_keys(client: &GraphClient) -> Vec<String> {
    let now = Utc::now();
    let response = client
        .execute_dql(&format!(
            "{{ p(func: eq(hodei.catalog.outbox.state, \"pending\"), first: {KEYS_PER_ROUND}) \
             {{ hodei.catalog.outbox.key }} \
             c(func: eq(hodei.catalog.outbox.state, \"claimed\")) \
             @filter(le(hodei.catalog.outbox.lease_until, \"{now}\")) \
             {{ hodei.catalog.outbox.key }} }}"
        ))
        .await
        .expect("the waiting keys answer");
    let mut keys = Vec::new();
    for root in ["p", "c"] {
        if let Some(nodes) = response
            .json
            .get(root)
            .and_then(serde_json::Value::as_array)
        {
            for node in nodes {
                if let Some(key) = node
                    .get("hodei.catalog.outbox.key")
                    .and_then(serde_json::Value::as_str)
                {
                    keys.push(key.to_string());
                }
            }
        }
    }
    keys
}

/// Un lote se drena en paralelo acotado: reclamar y marcar es el par del
/// worker; lo que otro se llevó no es un fallo, es el claim haciendo su
/// trabajo.
async fn drain_batch(client: &Arc<GraphClient>, worker: &str, keys: Vec<String>) -> (usize, usize) {
    let mut rounds = JoinSet::new();
    let in_flight = Arc::new(tokio::sync::Semaphore::new(DRAIN_CONCURRENCY));
    for key in keys {
        let permit = in_flight.clone();
        let outbox = DgraphAuditOutbox::new(client.clone());
        let worker = worker.to_string();
        rounds.spawn(async move {
            let _permit = permit.acquire().await.expect("the semaphore lives");
            let now = Utc::now();
            let lease_until = now + ChronoDuration::seconds(30);
            let claimed = outbox
                .claim(&key, &worker, lease_until, now)
                .await
                .expect("the claim answers");
            if claimed.is_none() {
                return false;
            }
            outbox
                .mark_delivered(&key, &worker, Utc::now())
                .await
                .expect("the mark answers");
            true
        });
    }
    let mut delivered = 0;
    let mut skipped = 0;
    while let Some(outcome) = rounds.join_next().await {
        match outcome.expect("the drain task does not panic") {
            true => delivered += 1,
            false => skipped += 1,
        }
    }
    (delivered, skipped)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drains_the_shared_outbox_waiting_facts_with_claim_and_mark() {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let client = Arc::new(client);
    let worker = format!("hsp220-drain-{}", nonce());

    let before = the_waiting_count(&client).await;
    println!("DRAIN before: {before} waiting facts");
    let mut rounds = 0;
    let mut total_delivered = 0;
    let mut total_skipped = 0;
    while rounds < MAX_ROUNDS {
        rounds += 1;
        let keys = the_next_waiting_keys(&client).await;
        if keys.is_empty() {
            break;
        }
        let (delivered, skipped) = drain_batch(&client, &worker, keys).await;
        total_delivered += delivered;
        total_skipped += skipped;
        println!("DRAIN round {rounds}: {delivered} delivered, {skipped} skipped");
    }
    let after = the_waiting_count(&client).await;
    println!(
        "DRAIN after: {after} waiting facts ({total_delivered} delivered by {worker} \
         in {rounds} rounds, {total_skipped} lost to other workers)"
    );

    // El drenaje cumple cuando el servidor ya no guarda un conjunto
    // pendiente global que desborde el mensaje gRPC.
    assert_eq!(
        after, 0,
        "the shared outbox must drain to zero waiting facts, not {after}"
    );

    let evidence = the_evidence_dir().join("drenaje.log");
    let line = format!(
        "drenaje {worker}: antes={before} despues={after} entregados={total_delivered} \
         rondas={rounds} saltados={total_skipped}\n"
    );
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&evidence)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()))
        .expect("the drain evidence line is written");
}
