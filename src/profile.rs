//! The boot profile over the single authority of the graph (HSP-209).
//!
//! This module is the seam that was missing between the pieces already built:
//! each vertical store of the graph (`DgraphCatalogStore`, `DgraphIamAdapter`,
//! `DgraphTrustStore`, `DgraphDecisionStore`, `DgraphProvenanceLedger`,
//! `DgraphJournal`) exists and is tested in isolation against a real alpha.
//! Here **all of them are assembled against the same connected client**, in
//! the order the dependencies impose, and the production deployment's
//! `AppState` is obtained: Dgraph for metadata, S3 for bytes, Cedar
//! for decisions. There is no second metadata store anywhere:
//! the graph is the only profile (configuration validation requires
//! `[graph]`, [`crate::config::AppConfig::validate`]) -- if the profile cannot be
//! assembled, boot dies with the error of the specific piece instead of
//! degrading.
//!
//! The composition is intentionally **fail-before-degrade**: the versioned-schema
//! guard (`ensure_ready`, v7) is the first gate
//! and is run by the IAM adapter --no store exists over a server the
//! binary does not understand--, and each later open propagates its typed
//! error. An unreachable alpha, an incompatible schema or a bucket
//! declared unreachable produce the same result: the service does not
//! start.
//!
//! Surfaces deliberately **not wired** in this profile (blockers
//! recorded in `target/uat/hsp209/evidencia.md`, not degradations):
//!
//! - Cedar schema persistence: implemented by `DgraphSchemaStore`
//!   (HSP-314 P2), a tenant of the same graph. The bundle schema and the
//!   IAM registry schema survive restarts and policies are validated
//!   against it.
//! - `/repositories` registry (`RepositoryReader/Writer` surface):
//!   no implementation over Dgraph (in-memory registry). The
//!   catalog repository (`/catalog/repositories`) is wired,
//!   and it is the one the vertical modalities use.
//! - Evidence, capsules and investigation over the graph: the stores
//!   exist (`hodei-evidence-storage`, `src/bundles/capsule_dgraph.rs`)
//!   but the service's read wrappers are generic over the embedded
//!   engine or have no handler adapter; the profile answers with the
//!   default behavior (honest 503/404).
//! - Durable quotas: `DgraphQuotaStore` exists (HSP-198) with no consumer in
//!   production; the profile uses the in-memory ledger the embedded
//!   deployment uses today -- same behavior, zero degradation of
//!   authority (the counters are not content metadata).

use std::sync::Arc;

use hodei_catalog::AllowAllAuthorizer;
use hodei_graph_client::{GraphClient, Tenant};
use hodei_iam::infrastructure::DgraphSchemaStore;
use hodei_iam::infrastructure::dgraph::DgraphIamAdapter;
use hodei_trust_policy_storage::{DgraphDecisionStore, DgraphTrustStore};

use crate::app_state::AppState;
use crate::audit::TracingAuditSink;
use crate::blob_probe::BlobStoreProbe;
use crate::bootstrap::BootstrapError;
use crate::composition_root::CompositionRoot;
use crate::config::AppConfig;
use crate::oci::session_journal::DgraphJournal;
use crate::proxy::dgraph_ledger::DgraphProvenanceLedger;
use crate::trust::cache::CachedTrustLedger;

/// The profile's graph: a single connected, authenticated client, with the
/// versioned-schema guard already passed, shared by all the
/// stores. Two clients would be two sessions against the same alpha; the
/// profile talks to one.
pub struct GraphAuthority {
    /// The shared connected client every store of the profile is opened on.
    pub client: Arc<GraphClient>,
    /// The tenant name the durable writers of this profile key under.
    pub tenant: Tenant,
    /// The IAM adapter: the six policy ports of the composition root.
    pub iam: Arc<DgraphIamAdapter>,
}

impl GraphAuthority {
    /// Opens the profile's authority: real connection (TLS with the declared CA and
    /// server name), ACL login with the declared credentials,
    /// and --inside the IAM adapter's open-- the v7 schema guard. Each
    /// step is a gate: none degrades to the next one or to the embedded
    /// engine.
    pub async fn connect(config: &AppConfig) -> Result<Self, BootstrapError> {
        let graph_config = config
            .graph
            .as_ref()
            .ok_or_else(|| {
                BootstrapError::Initialization(
                    "the graph profile requires a [graph] section".to_string(),
                )
            })?
            .clone();
        let endpoint = graph_config.endpoint.clone();
        let mut client = GraphClient::connect(graph_config.clone())
            .await
            .map_err(|error| {
                BootstrapError::DatabaseConnection(format!(
                    "cannot reach the Dgraph alpha at {endpoint}: {error}"
                ))
            })?;
        client.login().await.map_err(|error| {
            BootstrapError::DatabaseConnection(format!(
                "the Dgraph alpha at {endpoint} refused the login: {error}"
            ))
        })?;
        // The schema gate lives in the adapter's open: ensure_ready
        // (v7) decides whether this binary can write to that server. An
        // alpha that is compatible but has a very old or too new schema returns
        // the typed error and boot dies there, without touching anything local.
        let iam = Arc::new(
            DgraphIamAdapter::open(Arc::new(client))
                .await
                .map_err(|error| {
                    BootstrapError::Initialization(format!(
                        "the graph at {endpoint} is not ready for this binary: {error}"
                    ))
                })?,
        );
        let client = iam.client().clone();
        // The profile's durable tenant is the login namespace: the
        // convention of whoever writes, not a second authority. (The ACL
        // namespace is numeric by design; the graph's key tenants are
        // text and here it is named by that number.)
        let namespace = graph_config.namespace.to_string();
        let tenant = Tenant::new(&namespace).map_err(|error| {
            BootstrapError::Initialization(format!(
                "the graph namespace names an invalid tenant: {error}"
            ))
        })?;
        Ok(Self {
            client,
            tenant,
            iam,
        })
    }
}

/// Assembles the production profile's `AppState` over the single authority.
///
/// The order is not decorative:
///
/// 1. `GraphAuthority::connect` -- one client, one login, the v7 guard
///    already passed (the bootstrap does it before calling here).
/// 2. The content store arrives already open from the bootstrap: the byte CAS
///    is opened before the catalog because the integrity probe that
///    the catalog receives depends on it.
/// 3. `DgraphCatalogStore::open` + `with_probe`: catalog repositories,
///    assets, versions, tags, search, content ledger and the coordinated
///    GC ledger, all over the graph.
/// 4. `DgraphTrustStore` (with the embedded profile's TTL cache on top) and
///    `DgraphDecisionStore`: the promotion plane with the decisions over
///    the same authority.
/// 5. `DgraphProvenanceLedger` + `ProxyPlane`: the remote proxy's provenance
///    ledger over the graph.
/// 6. `DgraphJournal` + `UploadRegistry::with_journal`: resumable OCI sessions
///    whose authority is the graph, not process memory.
///
/// Any failure is terminal: there is no branch that "continues without the graph".
pub async fn assemble_graph_profile(
    state: AppState,
    config: &AppConfig,
    graph: &GraphAuthority,
    content: Arc<dyn hodei_hub::blob::BlobStore>,
) -> Result<AppState, BootstrapError> {
    let client = graph.client.clone();
    let endpoint = config
        .graph
        .as_ref()
        .map(|g| g.endpoint.clone())
        .unwrap_or_default();

    // 3a. The trust ledger: grants live in the same authority
    //    as the catalog that mentions them. It is opened before the catalog because
    //    search reads it. On top, the same read TTL cache the embedded
    //    profile uses.
    let trust: Arc<dyn hodei_catalog::TrustLedger> = Arc::new(
        DgraphTrustStore::open(client.clone(), graph.tenant.clone())
            .await
            .map_err(|error| {
                BootstrapError::Initialization(format!(
                    "the trust ledger cannot open on the graph at {endpoint}: {error}"
                ))
            })?,
    );
    let trust_ledger: Arc<dyn hodei_catalog::TrustLedger> =
        Arc::new(CachedTrustLedger::with_default_ttl(trust));

    // 3. The catalog over the graph. The authorizer is the pass-through one: the
    //    real decision is taken at the edge with Cedar, as in the embedded
    //    profile; the integrity probe comes from the CAS opened earlier.
    let catalog = Arc::new(
        hodei_catalog_storage::DgraphCatalogStore::open(
            client.clone(),
            Arc::new(AllowAllAuthorizer),
        )
        .await
        .map_err(|error| {
            BootstrapError::Initialization(format!(
                "the catalog cannot open on the graph at {endpoint}: {error}"
            ))
        })?
        .with_probe(Arc::new(BlobStoreProbe::new(content.clone())))
        // The search serves the trusted channel and the trust state of every
        // result from the same ledger the download path reads.
        .with_trust_ledger(trust_ledger.clone()),
    );
    let reader: Arc<dyn hodei_catalog::CatalogReader> = catalog.clone();
    let writer: Arc<dyn hodei_catalog::CatalogWriter> = catalog.clone();
    let searcher: Arc<dyn hodei_catalog::CatalogSearch> = catalog.clone();
    let ledger: Arc<dyn hodei_catalog::ContentLedger> = catalog.clone();

    // The pieces of the promotion plane: the decision ledger over the
    // graph and the policy the deployment declares. As in the
    // embedded one: a declared but unreadable policy stops boot; an
    // undeclared one leaves every promotion rejected with a recorded decision.
    let policies = crate::trust::promotion::registry_from(
        config.trust.policy_file.as_deref(),
        &config.trust.policy_id,
    )
    .map_err(BootstrapError::Initialization)?;
    let decisions = Arc::new(
        DgraphDecisionStore::open(client.clone())
            .await
            .map_err(|error| {
                BootstrapError::Initialization(format!(
                    "the trust decisions cannot open on the graph at {endpoint}: {error}"
                ))
            })?,
    );
    let plane = Arc::new(crate::trust::promotion::TrustPlane::new(
        decisions,
        policies,
        config.trust.policy_id.clone(),
        chrono::Duration::seconds(i64::from(
            u32::try_from(config.trust.grant_max_validity_secs.max(1)).unwrap_or(u32::MAX),
        )),
    ));

    // 5. The remote proxy's provenance: the durable ledger over the graph.
    let provenance_ledger: Arc<dyn hodei_catalog::ProvenanceLedger> = Arc::new(
        DgraphProvenanceLedger::open(client.clone())
            .await
            .map_err(|error| {
                BootstrapError::Initialization(format!(
                    "the provenance ledger cannot open on the graph at {endpoint}: {error}"
                ))
            })?,
    );
    // The proxy plane: the same wiring as the embedded profile, with the
    // graph's ledger.
    let egress = crate::proxy::egress::EgressPolicy::new(config.proxy.allowed_hosts.clone())
        .map_err(|error| BootstrapError::Initialization(error.to_string()))?;
    for name in &config.proxy.allowed_credentials {
        hodei_catalog::CredentialRef::parse(&format!("env:{name}"))
            .map_err(|error| BootstrapError::Initialization(error.to_string()))?;
    }
    let fetcher = crate::proxy::fetch::ProxyFetcher::new(
        egress,
        crate::proxy::fetch::FetchLimits {
            connect_timeout: std::time::Duration::from_secs(config.proxy.connect_timeout_secs),
            total_timeout: std::time::Duration::from_secs(config.proxy.total_timeout_secs),
            max_bytes: config.proxy.max_bytes,
        },
        config.proxy.max_concurrent,
    )
    .with_allowed_credentials(config.proxy.allowed_credentials.iter().cloned())
    .with_offline(config.proxy.offline);
    let proxy_plane = Arc::new(crate::proxy::ProxyPlane::new(fetcher, provenance_ledger));

    // GC and scrub: they read the roots of the same graph catalog and reclaim
    // from the same CAS; it is the only thing that deletes bytes.
    let scrub = crate::recovery::Scrubber::new(
        ledger.clone(),
        content.clone(),
        std::time::Duration::from_secs(config.content.gc_grace_secs),
    );
    let gc = crate::gc::GcService::new(
        ledger,
        content.clone(),
        std::time::Duration::from_secs(config.content.gc_grace_secs),
    );

    // 6. Resumable OCI sessions: the durable journal over the graph
    //    instead of the in-memory registry.
    let journal = Arc::new(
        DgraphJournal::open(client.clone(), graph.tenant.as_str())
            .await
            .map_err(|error| {
                BootstrapError::Initialization(format!(
                    "the OCI session journal cannot open on the graph at {endpoint}: {error}"
                ))
            })?,
    );
    let uploads = crate::oci::uploads::UploadRegistry::with_journal(
        std::time::Duration::from_secs(config.security.oci_upload_ttl_secs),
        config.security.oci_max_concurrent_uploads,
        journal,
    );

    // The profile's audit: the same diagnostic sink (tracing) and the
    // durable outbox over the graph (the ledger HSP-190/220
    // drain). The outbox dispatcher is left wired in the embedded
    // profile as in it: the HTTP surface records, the outbox stores,
    // the drain is proven by its own IT.
    let outbox = Arc::new(hodei_catalog_storage::DgraphAuditOutbox::new(client));
    // `outbox` stays available for the drain; the service's sink remains
    // the tracing one while there is no HTTP consumer of the
    // dispatcher in production (same state as the embedded profile).
    let _ = &outbox;

    let state = state
        .with_catalog(reader, writer)
        .with_catalog_search(searcher)
        .with_apt_snapshot_store(catalog.clone())
        .with_trust_ledger(trust_ledger)
        .with_proxy_plane(proxy_plane)
        .with_trust_plane(plane)
        .with_content_store(content)
        .with_uploads(Arc::new(uploads));
    // GC and scrub open or warn, as in the embedded profile: a
    // deletion surface that cannot check what is alive is a
    // surface that must remain closed, not one that deletes blindly.
    let state = match scrub {
        Ok(scrub) => state.with_scrub(Arc::new(scrub)),
        Err(error) => {
            tracing::warn!(%error, "the integrity scrub is not available");
            state
        }
    };
    let state = match gc {
        Ok(gc) => state.with_gc(Arc::new(gc)),
        Err(error) => {
            tracing::warn!(%error, "the garbage collection is not available");
            state
        }
    };
    let state = state.with_audit_sink(Arc::new(TracingAuditSink));

    tracing::info!(
        endpoint = %endpoint,
        tenant = %graph.tenant.as_str(),
        "the graph profile assembled: catalog, trust, decisions, provenance, journal, GC and the CAS of bytes"
    );
    Ok(state)
}

/// The profile's graph also carries the IAM policies: the composition
/// root comes in with the graph adapter as provider of the six
/// policy ports and with the Cedar schema store over Dgraph.
pub fn composition_root_for_graph(
    iam: Arc<DgraphIamAdapter>,
    schema_store: Arc<DgraphSchemaStore>,
    authoritative_schema_id: Option<String>,
) -> CompositionRoot {
    CompositionRoot::production_with_authoritative_schema(
        schema_store,
        iam,
        authoritative_schema_id,
    )
}

/// Explicit rejection of a `[database]` that does not declare the graph: the only
/// backend is Dgraph, and any other value (the retired embedded engine
/// included) is rejected by naming it.
///
/// It exists as a named constant because the rejection tests (the real
/// binary boot with `HODEI_DATABASE__DB_TYPE=rocksdb`) need to quote the
/// exact message with which the profile refuses; the path is walked by
/// `AppConfig::validate`, and this one states it.
pub const REFUSAL_MESSAGE: &str = crate::config::REFUSED_DATABASE_TYPE;

#[cfg(test)]
mod tests {
    use crate::config::DatabaseConfig;

    use super::REFUSAL_MESSAGE;

    #[test]
    fn the_refusal_names_the_backend_it_refuses() {
        // The message the rejection test looks for is the same one
        // the validation states: if one changes without the other, boot
        // would refuse with a text nobody expects.
        let database = DatabaseConfig {
            db_type: "rocksdb".to_string(),
        };
        let error = database
            .validate()
            .expect_err("the graph profile refuses a backend that is not the graph");
        let text = error.to_string();
        assert!(text.contains(REFUSAL_MESSAGE), "{text}");
        assert!(text.contains("rocksdb"), "the value is named: {text}");
    }

    #[test]
    fn a_database_declaring_the_graph_validates() {
        DatabaseConfig {
            db_type: "dgraph".to_string(),
        }
        .validate()
        .expect("the graph is the supported backend");
    }
}
