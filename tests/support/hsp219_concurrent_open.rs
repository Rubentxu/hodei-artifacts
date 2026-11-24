// HSP-219: el `open` condicionado que comparten las suites gated del
// catálogo Dgraph (hsp187, hsp190, hsp191) vía `include!`.
//
// El defecto probado: `DgraphCatalogStore::open` arranca la guarda de
// esquema y declara su DDL aditivo con dos `alter` en cada apertura. Bajo
// los threads por defecto de la suite —y más con dos binarios gated contra
// el mismo alpha— los `alter` concurrentes caen en la ventana de reindexado
// que Dgraph abre tras aceptar un DDL: el servidor responde
// `errIndexingInProgress. Please retry` y el open revienta de forma
// intermitente (medido 21-11: 5/8 pasadas fallan en hsp187 y 1/8 en hsp190
// lanzando ambas suites en paralelo con threads por defecto; secuencial o
// `--test-threads=1`: 0). La ventana dura ~1 s y es transitoria por
// contrato: el propio mensaje dice *retry*.
//
// La estabilización es doble y vive toda en el carril de tests (la
// producción abre una vez por proceso y no necesita el retry):
//
// 1. **Serialización intra-proceso**: una puerta asíncrona global por
//    binario de test de modo que, dentro de la suite, los opens comparten
//    la preparación del schema de uno en uno — el primero paga el alter y
//    los siguientes lo ven idempotente ya indexado.
// 2. **Retry acotado con backoff** (la receta probada de HSP-191, ampliada
//    a 8 intentos con respiro creciente): cubre la ventana cuando el DDL
//    lo dispara OTRO proceso — dos suites gated concurrentes contra el
//    mismo alpha, que aquí no se coordinan. Sólo se reintentan los
//    `Unavailable`, la clase transitoria que devuelve el alter rechazado;
//    un `Conflict`, `Denied` o `Corrupt` es un fallo real y panica de
//    inmediato, sin enmascararlo.
//
// Carril recomendado: las suites que alter-ean el mismo tipo se corren de
// una en una (`cargo test --test <suite>`, como prescribe HSP-190), y
// dentro de cada binario basta el carril por defecto de threads. Si dos
// suites se lanzan a la vez, el retry acotado absorbe la ventana.

/// La puerta de apertura: un open del catálogo a la vez por proceso de test.
fn hsp219_open_gate() -> &'static tokio::sync::Mutex<()> {
    static GATE: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// El almacén Dgraph del catálogo, con la apertura condicionada al DDL
/// concurrente del alpha (véase el módulo). Usa `AllowAllAuthorizer` como
/// todas las suites del stack; los adaptadores `with_identity`/`with_probe`
/// se encadenan sobre el almacén ya abierto (el helper panic antes de
/// devolver si el open no cuaja).
async fn open_store(
    client: std::sync::Arc<hodei_graph_client::GraphClient>,
) -> hodei_catalog_storage::DgraphCatalogStore {
    let _serial = hsp219_open_gate().lock().await;
    let mut last = String::new();
    for attempt in 0..8u64 {
        match hodei_catalog_storage::DgraphCatalogStore::open(
            client.clone(),
            std::sync::Arc::new(hodei_catalog::AllowAllAuthorizer),
        )
        .await
        {
            Ok(store) => return store,
            Err(error @ hodei_catalog::CatalogError::Unavailable { .. }) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(200 * (attempt + 1))).await;
            }
            Err(error) => panic!("the dgraph catalog store does not open: {error}"),
        }
    }
    panic!("the dgraph catalog store does not open after 8 attempts: {last}");
}
