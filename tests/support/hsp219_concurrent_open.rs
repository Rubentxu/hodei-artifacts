// HSP-219: the conditioned `open` shared by the gated suites of the
// Dgraph catalog (hsp187, hsp190, hsp191) via `include!`.
//
// The proven defect: `DgraphCatalogStore::open` starts the schema guard
// and declares its additive DDL with two `alter`s on every opening. Under
// the suite's default threads -- and more so with two gated binaries against
// the same alpha -- the concurrent `alter`s fall into the reindexing window
// that Dgraph opens after accepting a DDL: the server answers
// `errIndexingInProgress. Please retry` and the open blows up
// intermittently (measured 21-11: 5/8 runs fail in hsp187 and 1/8 in hsp190
// launching both suites in parallel with default threads; sequential or
// `--test-threads=1`: 0). The window lasts ~1 s and is transient by
// contract: the message itself says *retry*.
//
// The stabilization is twofold and lives entirely in the test lane (production
// opens once per process and does not need the retry):
//
// 1. **Intra-process serialization**: a global async gate per
//    test binary so that, within the suite, the opens share
//    the schema preparation one at a time -- the first pays the alter and
//    the following ones see it idempotent, already indexed.
// 2. **Bounded retry with backoff** (HSP-191's proven recipe, extended
//    to 8 attempts with growing breathers): covers the window when the DDL
//    is triggered by ANOTHER process -- two concurrent gated suites against the
//    same alpha, which are not coordinated here. Only `Unavailable`
//    is retried, the transient class the rejected alter returns;
//    a `Conflict`, `Denied` or `Corrupt` is a real failure and panics
//    immediately, without masking it.
//
// Recommended lane: suites that alter the same type are run one at a
// time (`cargo test --test <suite>`, as HSP-190 prescribes), and
// within each binary the default thread lane is enough. If two
// suites are launched at once, the bounded retry absorbs the window.

/// The opening gate: one catalog open at a time per test process.
fn hsp219_open_gate() -> &'static tokio::sync::Mutex<()> {
    static GATE: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// The catalog's Dgraph store, with the opening conditioned on the alpha's
/// concurrent DDL (see the module). Uses `AllowAllAuthorizer` like
/// all the stack's suites; the `with_identity`/`with_probe` adapters
/// are chained onto the already-opened store (the helper panics before
/// returning if the open does not hold).
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
