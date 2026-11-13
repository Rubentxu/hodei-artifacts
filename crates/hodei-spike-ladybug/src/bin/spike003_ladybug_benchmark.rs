//! El prototipo de la proyeccion analitica: la otra mitad de la
//! comparacion del SPIKE-003.
//!
//! El binario de la medicion (`spike003_benchmark`) construye el grafo en
//! SurrealDB, lo mide y lo EXPORTA. Este binario coge ese mismo grafo, lo
//! mete en LadybugDB y hace **la misma pregunta**.
//!
//! La comparacion es de los mismos datos y de las mismas preguntas, que es
//! lo que el spike pide. Y el coste de la importacion tambien se mide,
//! porque una proyeccion secundaria hay que reconstruirla cada vez que la
//! verdad cambia.
//!
//! # El comando
//!
//! ```text
//! HODEI_SPIKE003_EXPORT=/tmp/dataset.jsonl \
//!   cargo run --release -p hodei-risk-storage --bin spike003_benchmark
//!
//! cargo run --release -p hodei-spike-ladybug --bin spike003_ladybug_benchmark \
//!   --features ladybug -- /tmp/dataset.jsonl
//! ```

use std::collections::BTreeSet;
use std::time::Instant;

use grust::prelude::*;

use grust::prelude::Result as _GrustResult;
use grust_ladybug::{LadybugConfig, LadybugGraphStore, LadybugPath};

/// Cuantas veces se mide cada consulta.
const ROUNDS: usize = 50;

/// La linea del dataset.
#[derive(Debug, serde::Deserialize)]
struct Line {
    /// Que es la linea.
    kind: String,
    /// La etiqueta, si es un nodo o una relacion.
    #[serde(default)]
    label: String,
    /// El identificador, si es un nodo.
    #[serde(default)]
    id: String,
    /// El origen, si es una relacion.
    #[serde(default)]
    from: String,
    /// El destino, si es una relacion.
    #[serde(default)]
    to: String,
    /// La vulnerabilidad que se consulta.
    #[serde(default)]
    vulnerability: String,
    /// Las medidas del almacen transaccional.
    #[serde(default)]
    surrealdb: Measurements,
}

/// Las medidas del almacen transaccional.
#[derive(Debug, Default, serde::Deserialize)]
struct Measurements {
    /// El p50 del radio de impacto.
    blast_p50_ms: u128,
    /// El p95 del radio de impacto.
    blast_p95_ms: u128,
    /// El maximo del radio de impacto.
    blast_max_ms: u128,
    /// El p50 del linaje.
    lineage_p50_ms: u128,
    /// El p95 del linaje.
    lineage_p95_ms: u128,
    /// Cuantos sujetos alcanzó el radio de impacto.
    subjects: usize,
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("the export file is needed: the dataset and the measurements")?;
    let directory = std::env::var("HODEI_SPIKE003_LADYBUG_DIR")
        .unwrap_or_else(|_| "/tmp/spike003-ladybug".to_string());

    // ── The dataset ───────────────────────────────────────────────────────
    let content = std::fs::read_to_string(&path)?;
    let mut metadata = Measurements::default();
    let mut vulnerability = String::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();

    let started = Instant::now();
    for line in content.lines() {
        let Ok(parsed) = serde_json::from_str::<Line>(line) else {
            continue;
        };
        match parsed.kind.as_str() {
            "metadata" => {
                metadata = parsed.surrealdb;
                vulnerability = parsed.vulnerability;
            }
            "node" => nodes.push(Node::new(
                parsed.label.clone(),
                parsed.id.clone(),
                Props::new(),
            )),
            "edge" => edges.push(Edge::new(
                parsed.label.clone(),
                parsed.from.clone(),
                parsed.to.clone(),
                Props::new(),
            )),
            _ => {}
        }
    }
    println!("=== The dataset ===");
    println!(
        "nodes: {}, edges: {} (read in {:.1} s)",
        nodes.len(),
        edges.len(),
        started.elapsed().as_secs_f64()
    );
    if vulnerability.is_empty() {
        return Err(
            "the export has no metadata: the dataset is not the one of the measurement".into(),
        );
    }

    // ── The import ────────────────────────────────────────────────────────
    let mut config = LadybugConfig::untyped();
    config.path = LadybugPath::Directory(std::path::PathBuf::from(&directory));
    let store = LadybugGraphStore::new(config)?;
    store.bootstrap().await?;

    let graph = Graph::new(nodes, edges);
    let started = Instant::now();
    let report = store.put_graph(&graph).await?;
    let import = started.elapsed();
    println!("\n=== The import ===");
    println!("the import: {:.1} s ({report:?})", import.as_secs_f64());

    // ── The same question ─────────────────────────────────────────────────
    let mut samples = Vec::new();
    let mut reached = 0usize;
    for _ in 0..ROUNDS {
        let at = Instant::now();
        let (subjects, _deployments) = the_blast_radius(&store, &vulnerability).await?;
        reached = subjects;
        samples.push(at.elapsed());
    }
    assert!(
        reached > 0,
        "the analytic projection reached nothing: the measurement would be of the empty"
    );
    samples.sort_unstable();
    let p50 = samples[samples.len() / 2].as_millis();
    let p95 = samples[(samples.len() as f64 * 0.95).ceil() as usize - 1].as_millis();

    println!("\n=== The comparison ===");
    println!("the same dataset, the same walk, two engines\n");
    println!("                           | transaccional | analitica");
    println!(
        "radio de impacto p50        | {:>12} ms | {:>9} ms",
        metadata.blast_p50_ms, p50
    );
    println!(
        "radio de impacto p95        | {:>12} ms | {:>9} ms",
        metadata.blast_p95_ms, p95
    );
    println!(
        "radio de impacto max        | {:>12} ms | {:>9} ms",
        metadata.blast_max_ms,
        samples.last().unwrap().as_millis()
    );
    println!(
        "linaje p95                 | {:>12} ms |        n/a",
        metadata.lineage_p95_ms
    );
    println!(
        "sujetos alcanzados          | {:>12} | {:>9}",
        metadata.subjects, reached
    );
    println!(
        "el coste de la importacion  |       n/a    | {:.1} s",
        import.as_secs_f64()
    );

    println!("\n=== The verdict ===");
    if p95 <= metadata.blast_p95_ms {
        println!(
            "the analytic projection answers the walk {}-times faster, and the import costs {:.1} s",
            metadata.blast_p95_ms / p95.max(1),
            import.as_secs_f64()
        );
    } else {
        println!(
            "the analytic projection is NOT faster: {} ms against {} ms",
            p95, metadata.blast_p95_ms
        );
    }
    Ok(())
}

/// El radio de impacto en la proyeccion analitica.
///
/// Es el MISMO recorrido: los componentes de los que trata la
/// vulnerabilidad, los artefactos que los contienen y los despliegues que
/// los ejecutan.
async fn the_blast_radius(
    store: &LadybugGraphStore,
    vulnerability: &str,
) -> std::result::Result<(usize, usize), Box<dyn std::error::Error>> {
    let affected = store
        .get_edges(EdgeQuery {
            from: None,
            to: Some(NodeId::from(vulnerability)),
            label: Some(Label::from("AffectedBy")),
        })
        .await?;

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut level: Vec<String> = affected.iter().map(|edge| edge.from.to_string()).collect();
    let mut subjects: BTreeSet<String> = BTreeSet::new();
    let mut deployments: BTreeSet<String> = BTreeSet::new();

    while !level.is_empty() {
        let mut next_level: Vec<String> = Vec::new();
        for node in &level {
            let containers = store
                .get_edges(EdgeQuery {
                    from: None,
                    to: Some(NodeId::from(node.clone())),
                    label: Some(Label::from("Contains")),
                })
                .await?;
            for container in containers {
                subjects.insert(container.from.to_string());
                let deployed = store
                    .get_edges(EdgeQuery {
                        from: Some(container.from.clone()),
                        to: None,
                        label: Some(Label::from("DeployedAs")),
                    })
                    .await?;
                for deployment in deployed {
                    deployments.insert(deployment.to.to_string());
                }
            }

            let dependents = store
                .get_edges(EdgeQuery {
                    from: None,
                    to: Some(NodeId::from(node.clone())),
                    label: Some(Label::from("DependsOn")),
                })
                .await?;
            for dependent in dependents {
                let key = dependent.from.to_string();
                if seen.insert(key.clone()) {
                    next_level.push(key);
                }
            }
        }
        level = next_level;
    }

    Ok((subjects.len(), deployments.len()))
}
