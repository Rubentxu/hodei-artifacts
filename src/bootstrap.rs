//! Bootstrap module for Hodei Artifacts API
//!
//! This module handles the initialization of the application, including:
//! - RocksDB database connection setup
//! - Infrastructure adapter creation
//! - Use case composition via CompositionRoot
//! - Optional IAM schema registration

use crate::app_state::AppState;
use crate::auth::JwtAuthenticator;
use crate::authorization::SharedAuthorizer;
#[cfg(feature = "legacy")]
use crate::composition_root::CompositionRoot;
use crate::config::AppConfig;
// HSP-213: los importes del lector embebido quedan tras la feature
// `legacy`; el binario productivo se construye sin ellos.
#[cfg(feature = "legacy")]
use crate::repository_surreal::SurrealRepositoryStore;
#[cfg(feature = "legacy")]
use async_trait::async_trait;
use cedar_policy::{Entities, Schema};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
#[cfg(feature = "legacy")]
use hodei_catalog_storage::SurrealCatalogStore;
use hodei_hub::blob::FileBlobStore;
use std::fs;

#[cfg(feature = "legacy")]
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
#[cfg(feature = "legacy")]
use hodei_iam::register_iam_schema::dto::{RegisterIamSchemaCommand, RegisterIamSchemaResult};
#[cfg(feature = "legacy")]
use hodei_policies::build_schema::error::BuildSchemaError;
#[cfg(feature = "legacy")]
use hodei_policies::build_schema::ports::{
    AUTHORIZATION_BUNDLE_SCHEMA_ID, SchemaStoragePort, StoredSchema,
};
#[cfg(feature = "legacy")]
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
#[cfg(feature = "legacy")]
use surrealdb::Surreal;
#[cfg(feature = "legacy")]
use surrealdb::engine::local::RocksDb;
use tracing::{info, warn};

/// Configuration for the bootstrap process
#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    /// Whether to register the IAM schema on startup
    pub register_iam_schema: bool,
    /// Optional schema version to use
    pub schema_version: Option<String>,
    /// Whether to validate schemas during registration
    pub validate_schemas: bool,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            register_iam_schema: true,
            schema_version: Some("v1.0.0".to_string()),
            validate_schemas: true,
        }
    }
}

/// Bootstrap error types
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("Failed to connect to database: {0}")]
    DatabaseConnection(String),

    #[error("Failed to initialize namespace/database: {0}")]
    Initialization(String),

    #[error("Failed to register IAM schema: {0}")]
    SchemaRegistration(String),

    #[error("Invalid production security configuration: {0}")]
    Security(String),
}

/// Bootstrap the application with the given configuration
///
/// This function:
/// 1. Validates configuration and fails explicitly on any issues
/// 2. Initializes the RocksDB database connection and storage adapters
/// 3. Creates the CompositionRoot with all use case ports
/// 4. Optionally registers the IAM schema
/// 5. Returns the configured AppState ready for Axum
pub async fn bootstrap(
    config: &AppConfig,
    bootstrap_config: BootstrapConfig,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    info!("🚀 Starting Hodei Artifacts API bootstrap");

    // Step 0: Validate configuration and fail explicitly on any issues
    info!("🔍 Validating application configuration");
    config
        .security
        .validate_production_auth()
        .map_err(|error| BootstrapError::Security(error.to_string()))?;
    validate_bootstrap_configuration(config)
        .map_err(|e| BootstrapError::Initialization(e.to_string()))?;
    // The storage preflight is separate from the validation on purpose: the
    // validation looks at the configuration and cannot fail because of the
    // machine, while the preflight looks at the machine and says so when it
    // is the machine that is wrong.
    config
        .preflight_storage()
        .map_err(|error| BootstrapError::Initialization(format!("storage preflight: {error}")))?;

    // Step 1: Initialize infrastructure
    info!("📦 Initializing infrastructure adapters");
    // The engine of the metadata is decided by the profile (HSP-209): with
    // a `[graph]` section the authority is the alpha and the embedded
    // engine is never opened — its path is not created, not locked, not
    // touched. There is no branch that tries one and falls back to the
    // other: the choice was validated at the configuration, and a store
    // that cannot open stops the boot with its own error.
    if config.graph.is_some() {
        return bootstrap_graph_profile(config, bootstrap_config).await;
    }
    // HSP-213: sin `[graph]` el despliegue pide la autoridad embebida, y
    // esa autoridad sale del binario productivo. El binario se construye
    // sin la feature `legacy` y rechaza el perfil con el mensaje de
    // migracion; la herramienta legacy y las suites del arbol conservan
    // la feature mientras dure la ventana.
    #[cfg(feature = "legacy")]
    return bootstrap_embedded_profile(config, bootstrap_config).await;
    #[cfg(not(feature = "legacy"))]
    {
        let _ = bootstrap_config;
        Err(LEGACY_PROFILE_REFUSAL.to_string().into())
    }
}

/// HSP-213: la respuesta del binario productivo al perfil embebido antiguo.
/// Nombra la retirada, el destino (la autoridad externa) y la herramienta
/// que aún lee el almacén local durante la ventana de migración.
pub const LEGACY_PROFILE_REFUSAL: &str = "the embedded engine (SurrealDB over RocksDB) is \
retired from the production binary (HSP-213): declare a [graph] section so the external \
authority is the single source of truth, or use the legacy tool surface (`hodei-recovery`, \
built with --features legacy) to read and migrate an existing local store while the \
migration window stays open (window opened 2026-11-22)";

/// Bootstrap the deployment whose authority is the embedded engine.
///
/// HSP-213: this is the legacy profile, behind the `legacy` feature so the
/// production binary —built `--no-default-features`— never links SurrealDB
/// nor RocksDB. A deployment that still asks for it is refused by
/// [`bootstrap`] with [`LEGACY_PROFILE_REFUSAL`]; the tool surface keeps
/// the feature while the migration window is open.
#[cfg(feature = "legacy")]
async fn bootstrap_embedded_profile(
    config: &AppConfig,
    bootstrap_config: BootstrapConfig,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    let schema_storage = initialize_schema_storage(config).await?;

    // When the production bundle declares a Cedar schema, persist the same
    // schema for IAM policy validation. The gate and the persisted evaluator
    // then validate against one schema rather than two divergent sources.
    if let Some(schema_path) = &config.security.authorization_schema_path {
        let raw = fs::read_to_string(schema_path).map_err(|error| {
            BootstrapError::Security(format!(
                "cannot read authorization schema {schema_path}: {error}"
            ))
        })?;
        let fragment: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|error| BootstrapError::Security(format!("invalid Cedar schema: {error}")))?;
        let envelope = serde_json::json!({
            "format": "cedar-schema-fragments-v1",
            "fragments": [fragment],
        });
        schema_storage
            .save_schema(
                envelope.to_string(),
                Some(AUTHORIZATION_BUNDLE_SCHEMA_ID.to_string()),
            )
            .await
            .map_err(|error| BootstrapError::Security(format!("cannot persist schema: {error}")))?;
    }

    // Initialize policy adapter with the same DB client
    let policy_adapter = Arc::new(SurrealPolicyAdapter::new(
        schema_storage.db().clone().into(),
    ));

    // The account index of the IAM policies cannot be declared from here:
    // defining an index on the local engine keeps the database directory
    // locked by the process that defined it, so the service would serve
    // correctly but could never open the database again — which breaks
    // booting twice against the same database. The index belongs to a command
    // of its own, and a deployment that has not run it is told so, with the
    // command to run.
    match policy_adapter.scope_initialized().await {
        Ok(true) => {}
        Ok(false) => warn!(
            "⚠️  The policy account index is missing. The IAM listing still isolates by account, \
             but it scans the table. Stop the service and run `ensure-iam-index` from the legacy \
             tool package (HSP-213: it no longer ships with the service) to declare it; from a \
             source checkout: `cargo run --features legacy --bin ensure-iam-index -- --db <path>`."
        ),
        Err(error) => warn!(
            "⚠️  The policy account index could not be checked: {}. Stop the service and run \
             `ensure-iam-index` to declare it.",
            error
        ),
    }

    // Step 2: Use Composition Root to create all use case ports
    info!("🏗️  Creating use cases via CompositionRoot");
    // When the deployment declares a bundle schema, that schema is the one
    // every policy is validated and evaluated against. Naming it is what
    // keeps the gate and the persisted evaluator on the same declarations: the
    // IAM schema registered below would otherwise displace it simply by being
    // stored later, and the two circuits would validate against different
    // schemas without anybody deciding that.
    let authoritative_schema_id = config
        .security
        .authorization_schema_path
        .as_ref()
        .map(|_| AUTHORIZATION_BUNDLE_SCHEMA_ID.to_string());
    let root = CompositionRoot::production_with_authoritative_schema(
        schema_storage.clone(),
        policy_adapter,
        authoritative_schema_id,
    );

    // Step 3: Determine schema version
    let schema_version = if bootstrap_config.register_iam_schema {
        info!("📝 Registering IAM schema");
        let result = register_iam_schema(
            &*root.iam_ports.register_iam_schema,
            bootstrap_config.schema_version.clone(),
            bootstrap_config.validate_schemas,
        )
        .await?;

        info!(
            "✅ IAM schema registered successfully (version: {}, entities: {}, actions: {})",
            result.schema_version, result.entity_types_registered, result.action_types_registered
        );

        result.schema_version
    } else {
        warn!("⚠️  Skipping IAM schema registration");
        bootstrap_config
            .schema_version
            .unwrap_or_else(|| "unregistered".to_string())
    };

    // Step 4: Create AppState from CompositionRoot
    info!("🎯 Creating application state");
    let app_state = AppState::from_composition_root(schema_version.clone(), root);
    let app_state = if config.repository.durable {
        let repository_store = Arc::new(SurrealRepositoryStore::new(schema_storage.db().clone()));
        repository_store
            .initialize()
            .await
            .map_err(|error| BootstrapError::Initialization(error.to_string()))?;
        app_state.with_repository_store(repository_store)
    } else {
        app_state
    };
    // The evidence of the deployment is read from the same store the
    // repositories live in: the trust capsule is assembled from the
    // evidence that is there, and the evidence is there.
    let app_state = app_state.with_evidence_store(crate::evidence_surreal::reader_over(Arc::new(
        schema_storage.db().clone(),
    )));
    // The catalog and the store of content. The catalog lives in the same
    // engine as the rest, because a transaction that touches two databases
    // is not a transaction; and the content lives in a directory, because
    // the bytes of an artifact are not rows of a database.
    let app_state = if config.repository.durable {
        // The catalog does not decide again what the HTTP boundary already
        // decided with Cedar. Its queries are already scoped to the
        // verified account, so the isolation between accounts does not
        // depend on this second check: it depends on the key of each row.
        // The check that does matter --the permission to publish or to
        // download-- is asked at the boundary, where the canonical HRN of
        // the repository is known; the port of the catalog does not carry
        // the region, and a second decision about another resource would
        // not be stronger, it would be another decision.
        // The content store comes first: the probe over it is what the
        // catalog needs to be able to tell a healthy record from a
        // broken one.
        //
        // Which store it is depends on the deployment, not on the
        // request: the catalog rows name a digest, and a digest has to
        // live in one place. The chosen backend is the only one ever
        // consulted -- an S3 deployment never reads nor writes
        // `cas_path` -- and a backend that is configured but
        // unreachable stops the boot instead of falling back to the
        // directory, because content that silently lives in two places
        // is content the GC deletes as garbage and the health probe
        // reports as fine.
        let (content, content_check) = open_content_store(config).await?;
        // The catalog does not decide again what the HTTP boundary already
        // decided with Cedar. Its queries are already scoped to the
        // verified account, so the isolation between accounts does not
        // depend on this second check: it depends on the key of each row.
        // The check that does matter --the permission to publish or to
        // download-- is asked at the boundary, where the canonical HRN of
        // the repository is known; the port of the catalog does not carry
        // the region, and a second decision about another resource would
        // not be stronger, it would be a second decision.
        let catalog = Arc::new(
            SurrealCatalogStore::with_authorizer(
                schema_storage.db().clone(),
                Arc::new(hodei_catalog::AllowAllAuthorizer),
            )
            // The probe is what makes the catalog able to say that the
            // bytes are where the record says. Without it, a record whose
            // content is not in the store is served as healthy, and the
            // corruption is confused with the absence.
            .with_probe(Arc::new(crate::blob_probe::BlobStoreProbe::new(
                content.clone(),
            ))),
        );
        catalog
            .initialize()
            .await
            .map_err(|error| BootstrapError::Initialization(error.to_string()))?;
        let reader: Arc<dyn hodei_catalog::CatalogReader> = catalog.clone();
        let ledger: Arc<dyn hodei_catalog::ContentLedger> = catalog.clone();
        let trust_ledger: Arc<dyn hodei_catalog::TrustLedger> = Arc::new(
            crate::trust::cache::CachedTrustLedger::with_default_ttl(catalog.clone()),
        );
        let provenance_ledger: Arc<dyn hodei_catalog::ProvenanceLedger> = catalog.clone();
        // The search is the same catalog with another question: there is
        // no separate index to synchronise, and that is why what the
        // search promises is what there is.
        let searcher: Arc<dyn hodei_catalog::CatalogSearch> = catalog.clone();
        let writer: Arc<dyn hodei_catalog::CatalogWriter> = catalog;
        // The garbage collection reads the roots from the same catalog and
        // reclaims from the same store; it is the only thing that deletes
        // bytes. If the store cannot reclaim, the surface stays closed.
        let scrub = crate::recovery::Scrubber::new(
            ledger.clone(),
            content.clone(),
            Duration::from_secs(config.content.gc_grace_secs),
        );
        let gc = crate::gc::GcService::new(
            ledger,
            content.clone(),
            Duration::from_secs(config.content.gc_grace_secs),
        );
        // The promotion: the decisions live in the same engine as the
        // rest, and the policy that decides is the one the deployment
        // declares. A policy that is declared and cannot be read stops
        // the boot; one that is not declared leaves every promotion
        // refused with a recorded decision.
        let policies = crate::trust::promotion::registry_from(
            config.trust.policy_file.as_deref(),
            &config.trust.policy_id,
        )
        .map_err(BootstrapError::Initialization)?;
        let decisions = Arc::new(hodei_trust_policy_storage::SurrealDecisionStore::new(
            Arc::new(schema_storage.db().clone()),
        ));
        let plane = Arc::new(crate::trust::promotion::TrustPlane::new(
            decisions,
            policies,
            config.trust.policy_id.clone(),
            chrono::Duration::seconds(i64::from(
                u32::try_from(config.trust.grant_max_validity_secs.max(1)).unwrap_or(u32::MAX),
            )),
        ));
        // The egress policy comes from the service configuration, never
        // from a repository: an empty list means no outbound request.
        let egress = crate::proxy::egress::EgressPolicy::new(config.proxy.allowed_hosts.clone())
            .map_err(|error| BootstrapError::Initialization(error.to_string()))?;
        // A credential name that a repository could never reference is a
        // configuration mistake: it is refused at start, not silently kept.
        for name in &config.proxy.allowed_credentials {
            hodei_catalog::CredentialRef::parse(&format!("env:{name}"))
                .map_err(|error| BootstrapError::Initialization(error.to_string()))?;
        }
        let fetcher = crate::proxy::fetch::ProxyFetcher::new(
            egress,
            crate::proxy::fetch::FetchLimits {
                connect_timeout: Duration::from_secs(config.proxy.connect_timeout_secs),
                total_timeout: Duration::from_secs(config.proxy.total_timeout_secs),
                max_bytes: config.proxy.max_bytes,
            },
            config.proxy.max_concurrent,
        )
        .with_allowed_credentials(config.proxy.allowed_credentials.iter().cloned())
        .with_offline(config.proxy.offline);
        let proxy_plane = Arc::new(crate::proxy::ProxyPlane::new(fetcher, provenance_ledger));
        let app_state = app_state
            .with_catalog(reader, writer)
            .with_catalog_search(searcher)
            .with_trust_ledger(trust_ledger)
            .with_proxy_plane(proxy_plane)
            .with_trust_plane(plane)
            .with_content_store(content)
            // El probe de readiness del perfil activo. Lo que exige este
            // despliegue es el catalogo --que vive en el motor embebido-- y
            // el almacen de contenido. La politica de confianza se carga
            // en memoria al arrancar y no se puede perder en caliente, asi
            // que no es una dependencia de readiness.
            //
            // El probe va envuelto en el cache corto: dos sondeos
            // seguidos no gastan el presupuesto dos veces, y el handler no
            // tiene estado.
            .with_readiness(Arc::new(crate::readiness::CachedProbe::new(Arc::new(
                crate::readiness::ProfileProbe::new()
                    .with_catalog(schema_storage.db().clone())
                    .with_content(content_check),
            ))));
        let app_state = match scrub {
            Ok(scrub) => app_state.with_scrub(Arc::new(scrub)),
            Err(error) => {
                warn!(%error, "the integrity scrub is not available");
                app_state
            }
        };
        match gc {
            Ok(gc) => app_state.with_gc(Arc::new(gc)),
            Err(error) => {
                warn!(%error, "the garbage collection is not available");
                app_state
            }
        }
    } else {
        app_state
    };
    let app_state = app_state.with_body_limit(config.server.max_body_size);
    // El registro de las sesiones de subida: el estado que vive entre
    // peticiones, con los limites que dice la configuracion.
    let app_state = app_state.with_uploads(Arc::new(crate::oci::uploads::UploadRegistry::new(
        std::time::Duration::from_secs(config.security.oci_upload_ttl_secs),
        config.security.oci_max_concurrent_uploads,
    )));
    // El libro mayor de las cuotas: los limites que dice la
    // configuracion, compartidos por toda peticion y toda sesion. Un
    // despliegue que no dice cuotas obtiene un libro sin limites, que
    // admite todo y es exactamente lo que dijo.
    let app_state = app_state.with_quota(config.quota.the_limits());
    // La direccion publica del despliegue la dice la configuracion, y no
    // la peticion: un `Host` hostil o un `X-Forwarded-Proto` hostil que
    // cambien la URL que el registro devuelve es una URL que el cliente
    // elige. La configuracion que no se puede entender no arranca.
    let app_state = app_state.with_public_url(
        config
            .public_url
            .the_public_url()
            .map_err(|error| BootstrapError::Initialization(error.to_string()))?,
    );
    // The ring that signs the archive URLs of the provider mirror. A key that
    // is named and cannot be read stops the boot; no key at all is allowed
    // and is announced, because the mirror then refuses to issue archive
    // URLs and a Terraform client cannot install from it.
    let signed_urls = config
        .signing
        .the_ring()
        .map_err(|error| BootstrapError::Initialization(format!("signing keys: {error}")))?;
    if signed_urls.is_configured() {
        info!(
            ttl_secs = signed_urls.ttl_secs(),
            "the signed URLs of the provider mirror are enabled"
        );
    } else {
        warn!(
            "no signing key is configured: the Terraform provider mirror will not issue archive URLs and its version documents answer 503; add a key to [signing]"
        );
    }
    let app_state = app_state.with_signed_urls(signed_urls);
    if config.quota.max_stored_bytes_per_account.is_some()
        || config.quota.max_concurrent_uploads_per_account.is_some()
    {
        info!(
            max_stored_bytes_per_account = ?config.quota.max_stored_bytes_per_account,
            max_concurrent_uploads_per_account = ?config.quota.max_concurrent_uploads_per_account,
            "the account quotas are in force"
        );
    }
    let app_state = configure_production_security(app_state, config)?;
    // La investigacion se cablea DESPUES de la seguridad de produccion:
    // el autorizador que la investigation usa es el que el deployment
    // declara, y cablearla antes seria cablear el de reserva --el que
    // deniega todo-- por error de orden.
    let app_state = wire_the_investigation(app_state, config, &schema_storage)?;
    let app_state = wire_the_risk_facts(app_state, config, &schema_storage);
    // El ciclo reactivo CVE se cablea al final: necesita el plano de
    // confianza, el libro mayor y los hechos de riesgo ya montados. Sin
    // plano no hay ciclo: la entrada de advisories responde 501 y el
    // despliegue sigue sirviendo como antes de este módulo.
    let app_state = wire_the_reactor(app_state, config, &schema_storage).await?;
    if config.security.admin_api_enabled && !app_state.admin_surface_ready() {
        tracing::error!(
            "admin_api_enabled=true but no authenticator/authorizer is configured; the administrative API remains closed"
        );
    }

    info!(
        "✅ Bootstrap completed successfully (schema version: {})",
        schema_version
    );

    Ok(app_state)
}

/// Build the store of content the configuration names and clean the staging
/// an interrupted upload left behind.
///
/// The boot of both profiles —the embedded and the graph one— goes through
/// here so the bytes live in exactly one place and the cleanup rules are
/// decided once: the directory backend removes the files no one is writing;
/// the object store aborts the multipart uploads whose last part is older
/// than the margin, and drops the completed-but-never-published staging
/// objects (HSP-139: an incomplete multipart is invisible to `list_blobs`
/// but not free -- it is billed and it is the one piece of garbage the
/// bucket would never reclaim).
///
/// The staging of a write **in progress** is not touched: a locked file is
/// not removed, and an upload whose last part is younger than the margin
/// stays alone. The age is only the margin of prudence, not the criterion:
/// an old upload that is still being written is a live upload, and taking
/// it away breaks it.
///
/// And a staging that could not be removed is not a reason to not start: it
/// is a problem of disk or of the object store, and the service has work to
/// do.
async fn open_content_store(
    config: &AppConfig,
) -> Result<
    (
        Arc<dyn hodei_hub::blob::BlobStore>,
        crate::readiness::ContentCheck,
    ),
    BootstrapError,
> {
    let (chosen, content_check) = the_content_store(config).await?;
    let content: Arc<dyn hodei_hub::blob::BlobStore> = chosen.as_blob();
    let cleanup = match chosen {
        ChosenContentStore::Filesystem(ref directory) => directory
            .clean_expired_staging(Duration::from_secs(config.content.staging_max_age_secs)),
        ChosenContentStore::S3(ref store) => {
            store
                .clean_staging(Duration::from_secs(config.content.staging_max_age_secs))
                .await
        }
    };
    match cleanup {
        Ok(cleanup) => {
            if cleanup.removed > 0 {
                warn!(
                    removed = cleanup.removed,
                    "the staging of interrupted uploads was removed at the boot"
                );
            }
            if cleanup.alive > 0 {
                info!(
                    alive = cleanup.alive,
                    "the staging of the uploads in progress was left alone at the boot"
                );
            }
            for (path, reason) in &cleanup.failed {
                warn!(
                    %reason,
                    path,
                    "the staging of an interrupted upload could not be removed at the boot"
                );
            }
        }
        Err(error) => {
            warn!(
                %error,
                "the staging could not be read at the boot; the interrupted uploads stay"
            );
        }
    }
    Ok((content, content_check))
}

/// Bootstrap the deployment whose authority is the graph (HSP-209).
///
/// This is the other half of the single-profile contract: a service that
/// declares `[graph]` never opens the embedded engine -- its path is not
/// created, not locked, not touched -- and every store of the profile is
/// assembled by [`crate::profile`] over one connected, logged-in, schema-
/// guarded client. There is no fallback in either direction: if the alpha,
/// the CAS, or any store cannot open, the boot dies with that store's own
/// error, and RocksDB is not even tried.
///
/// What the profile does NOT carry, and says so instead of degrading:
///
/// - The persistence of the Cedar schema: `SchemaStoragePort` has no Dgraph
///   implementation (registered blocker). The gate authorizes from the
///   declared bundle file -- that is the security boundary every request
///   crosses -- and the persisted evaluator runs in NoSchema mode, exactly
///   the mode the IAM-over-graph ITs (HSP-186) certify.
/// - The IAM schema registration: it writes through the same missing port,
///   so a deployment that asks for it stops with the blocker named.
/// - The evidence reader, the investigation, the reactor and the legacy
///   repository registry: their Dgraph stores exist (`HSP-188/206`) or their
///   wiring is not done yet; the surfaces answer with their default closed
///   behavior and the profile does not open a local engine to make them
///   answer.
async fn bootstrap_graph_profile(
    config: &AppConfig,
    bootstrap_config: BootstrapConfig,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    info!("🕸️  Starting the Dgraph profile: the alpha is the single authority of the metadata");

    // Step 0 is shared with the embedded profile and already ran: the
    // security bundle, the configuration validation (which under a graph
    // section refuses the embedded engine), and the storage preflight
    // (which under a graph section does not touch the RocksDB path).

    // Step 1: the authority itself. Connection, ACL login and the versioned
    // schema guard (v7) inside the IAM adapter's open: no store of the
    // profile exists over a server this binary cannot speak to.
    let graph = crate::profile::GraphAuthority::connect(config).await?;

    if config.schema.register_iam_on_startup {
        return Err(Box::new(BootstrapError::SchemaRegistration(
            "the graph profile cannot register the IAM Cedar schema: SchemaStoragePort is not \
             implemented over Dgraph (HSP-209 blocker). Set schema.register_iam_on_startup = \
             false, or run the embedded profile until the port lands"
                .to_string(),
        )));
    }
    if config.security.authorization_schema_path.is_some() {
        warn!(
            "the authorization bundle schema cannot be PERSISTED on the graph profile \
             (HSP-209 blocker: no SchemaStoragePort over Dgraph); the Cedar gate authorizes \
             from the declared file and the persisted evaluator runs in NoSchema mode"
        );
    }

    // Step 2: the bytes. The same decision and the same readiness the
    // embedded profile makes of its content store: an S3 deployment that
    // cannot reach its bucket does not boot, and never writes the
    // filesystem CAS on the side.
    let (content, content_check) = open_content_store(config).await?;

    // Step 3: the use cases over the graph. The IAM ports of the
    // composition root are the Dgraph adapter's; the schema provider is
    // the profile's honest stand-in (writes refused, reads empty).
    let root = crate::profile::composition_root_for_graph(graph.iam.clone());
    let schema_version = bootstrap_config
        .schema_version
        .unwrap_or_else(|| "graph-v7".to_string());
    let app_state = AppState::from_composition_root(schema_version.clone(), root);

    // Step 4: the vertical stores, all over the same client, with the
    // failure of any of them stopping the boot.
    let app_state =
        crate::profile::assemble_graph_profile(app_state, config, &graph, content).await?;

    // The tail the two profiles share: the body limit, the quotas (the
    // in-memory ledger the embedded deployment also uses; the durable
    // `DgraphQuotaStore` waits for its production consumer), and the
    // production security bundle, which on this profile is the file-based
    // gate without the persistence step.
    let app_state = app_state.with_body_limit(config.server.max_body_size);
    // Readiness of the graph profile: the alpha (reachable + valid ACL
    // login, one authenticated read) and the content store. Without this the
    // default `NothingRequired` probe would report ready with the alpha down.
    let app_state =
        app_state.with_readiness(Arc::new(crate::readiness::CachedProbe::new(Arc::new(
            crate::readiness::ProfileProbe::new()
                .with_graph(graph.client.clone())
                .with_content(content_check),
        ))));
    let app_state = app_state.with_quota(config.quota.the_limits());
    if config.quota.max_stored_bytes_per_account.is_some()
        || config.quota.max_concurrent_uploads_per_account.is_some()
    {
        info!(
            max_stored_bytes_per_account = ?config.quota.max_stored_bytes_per_account,
            max_concurrent_uploads_per_account = ?config.quota.max_concurrent_uploads_per_account,
            "the account quotas are in force"
        );
    }
    let app_state = configure_production_security(app_state, config)?;
    // The investigation limits travel without the surface: the graph
    // profile has no evidence reader over the alpha yet, so the endpoint
    // answers with its default closed behavior and no local engine is
    // opened to give it an answer.
    let app_state = app_state.with_investigation_limits(config.investigation.clone());
    let app_state = crate::risk_facts::wire_graph_risk_facts(app_state, config, &graph).await?;
    let app_state = crate::evidence_dgraph::wire_graph_evidence_reader(app_state, &graph).await?;
    if config.security.admin_api_enabled && !app_state.admin_surface_ready() {
        tracing::error!(
            "admin_api_enabled=true but no authenticator/authorizer is configured; the administrative API remains closed"
        );
    }

    info!(
        "✅ Bootstrap completed successfully on the graph profile (schema version: {})",
        schema_version
    );
    Ok(app_state)
}

/// The store of content the deployment chose, with the operations that
/// each backend implements on its own type.
///
/// The catalog rows name a digest, and a digest has to live in exactly one
/// place: both consumers of the bytes --the probe and the garbage
/// collection-- must not decide again what the configuration already
/// decided. The shared part of the two backends is the `Arc<dyn
/// BlobStore>`; the staging cleanup travels discriminated because each
/// implementation decides what "abandoned" means with its own primitives
/// --a lock on a file, the age of the last part of a multipart upload.
enum ChosenContentStore {
    Filesystem(Arc<FileBlobStore>),
    S3(Arc<hodei_hub_s3::S3BlobStore>),
}

impl ChosenContentStore {
    /// The store every consumer --probe, scrub, GC, raw and OCI-- is
    /// given. It is the only view of the content the service ever uses.
    fn as_blob(&self) -> Arc<dyn hodei_hub::blob::BlobStore> {
        match self {
            Self::Filesystem(store) => store.clone(),
            Self::S3(store) => store.clone(),
        }
    }
}

/// Build the store of content the configuration names.
///
/// There is no fallback between backends, in either direction. The table
/// was already validated by `ContentConfig::validate`; what this adds is
/// the part that can only be done by touching the infrastructure: the
/// bucket must answer before the service says it is up, because content
/// that silently lives in two places is content the GC deletes as
/// garbage and the health probe reports as fine.
///
/// Credentials are never read from the configuration: the SDK's own
/// provider chain --environment, shared config, IAM role-- is the only
/// source, so no key can end up in a TOML file, a log line or an audit
/// record. The default chain has no timeout of its own and a deployment
/// without a bucket policy could hang the boot inside a retry loop, so
/// the check itself carries one bounded deadline; a bucket that does not
/// answer in time is an unreachable bucket, and unreachable is an error.
async fn the_content_store(
    config: &AppConfig,
) -> Result<(ChosenContentStore, crate::readiness::ContentCheck), BootstrapError> {
    use crate::config::ContentBackend;
    match config.content.backend {
        ContentBackend::Filesystem => {
            let store = FileBlobStore::new(&config.content.cas_path).map_err(|error| {
                BootstrapError::Initialization(crate::config::redact_secrets(&format!(
                    "content directory '{}' cannot be opened: {error}. Check that it exists and the service user can write to it, or set HODEI_CONTENT__CAS_PATH.",
                    config.content.cas_path
                )))
            })?;
            // La comprobacion de readiness del almacen: el directorio
            // existe y es un directorio. No se lee nada de dentro.
            let check = crate::readiness::ContentCheck::Filesystem {
                root: std::path::PathBuf::from(&config.content.cas_path),
            };
            Ok((ChosenContentStore::Filesystem(Arc::new(store)), check))
        }
        ContentBackend::S3 => {
            let table = config
                .content
                .s3
                .as_ref()
                // `ContentConfig::validate` refuses this combination
                // already; if we are here with no table, something
                // bypassed the validation, and the honest answer is the
                // same explicit error, not a panic.
                .ok_or_else(|| {
                    BootstrapError::Security(
                        "backend = \"s3\" requires the [content.s3] table with a bucket; \
                         there is no fallback to the filesystem"
                            .to_string(),
                    )
                })?;
            let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
            if let Some(region) = table.region.as_deref() {
                loader = loader.region(aws_config::Region::new(region.to_string()));
            }
            let sdk = loader.load().await;
            let scheme = if table.tls { "https" } else { "http" };
            let mut builder = aws_sdk_s3::config::Builder::from(&sdk)
                .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest());
            if let Some(endpoint) = table.endpoint.as_deref() {
                // The endpoint may be given whole --with its scheme--
                // or as a bare `host[:port]`, which is what the table
                // documents. The scheme of `tls` only prefixes what
                // does not already have one; an operator who wrote
                // https:// and tls = false said the more specific
                // thing, and that is what is obeyed.
                let url = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
                    endpoint.to_string()
                } else {
                    format!("{scheme}://{endpoint}")
                };
                builder = builder.endpoint_url(url);
                // The stores that are not AWS S3 --MinIO, LocalStack,
                // Ceph-- expect the bucket in the path, not in the DNS
                // name of the host. Against real AWS this is harmless
                // only for compatibility; the configuration declares the
                // endpoint precisely when it is not AWS, and an operator
                // pointing a custom endpoint expects path style.
                builder = builder.force_path_style(true);
            }
            let client = aws_sdk_s3::Client::from_conf(builder.build());
            let store = hodei_hub_s3::S3BlobStore::new(
                client.clone(),
                hodei_hub_s3::S3BlobConfig::new(table.bucket.clone()),
            )
            .map_err(|error| {
                BootstrapError::Initialization(format!(
                    "the content store of bucket {:?} was declared with values it cannot serve: {error:?}",
                    table.bucket
                ))
            })?;
            // The preflight. `head_bucket` is the cheapest call that
            // says both "the credentials work" and "the bucket exists".
            let reachable = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                client.head_bucket().bucket(table.bucket.clone()).send(),
            )
            .await;
            match reachable {
                Ok(Ok(_response)) => {}
                // The bucket is there but not ours --403 says that more
                // often than 404, which is why both are distinguished--:
                // still an error, never a fallback.
                Ok(Err(error)) => {
                    return Err(BootstrapError::Initialization(the_bucket_failure(
                        &table.bucket,
                        &format!("cannot be used: {error:?}"),
                    )));
                }
                Err(_elapsed) => {
                    return Err(BootstrapError::Initialization(the_bucket_failure(
                        &table.bucket,
                        "did not answer within 10 seconds",
                    )));
                }
            }
            // La comprobacion de readiness del almacen: `head_bucket`,
            // la llamada mas barata que existe. El presupuesto de red es
            // el del probe: un bucket que no contesta produce un
            // `timeout`, no una peticion colgada.
            let check = crate::readiness::ContentCheck::S3 {
                client: client.clone(),
                bucket: table.bucket.clone(),
            };
            Ok((ChosenContentStore::S3(Arc::new(store)), check))
        }
    }
}

/// The startup error for a content bucket that cannot be used: what failed,
/// and what to check. Credentials never appear: the detail is redacted.
fn the_bucket_failure(bucket: &str, detail: &str) -> String {
    crate::config::redact_secrets(&format!(
        "the content bucket {bucket:?} {detail}. Check [content.s3] endpoint/region/bucket (HODEI_CONTENT__S3__ENDPOINT, HODEI_CONTENT__S3__BUCKET), that the endpoint is reachable from this host, and that the credentials of the AWS provider chain (AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY or an IAM role) may access the bucket."
    ))
}

/// Configure the production JWT authenticator and Cedar authorizer when the
/// security settings provide the complete bundle. A partial bundle is a
/// startup error, not a silent fallback to DenyAll.
fn configure_production_security(
    state: AppState,
    config: &AppConfig,
) -> Result<AppState, BootstrapError> {
    let security = &config.security;
    let fields = [
        security.auth_jwt_secret.is_some() || security.auth_jwks_path.is_some(),
        security.auth_issuer.is_some(),
        security.auth_audience.is_some(),
        security.authorization_policy_path.is_some(),
        security.authorization_schema_path.is_some(),
    ];
    if !fields.iter().any(|present| *present) {
        return Ok(state);
    }
    let (secret, jwks_path, issuer, audience, policy_path, schema_path) = (
        security.auth_jwt_secret.clone(),
        security.auth_jwks_path.clone(),
        security.auth_issuer.clone(),
        security.auth_audience.clone(),
        security.authorization_policy_path.clone(),
        security.authorization_schema_path.clone(),
    );
    let (Some(issuer), Some(audience), Some(policy_path), Some(schema_path)) =
        (issuer, audience, policy_path, schema_path)
    else {
        let missing = [
            (
                security.auth_jwt_secret.is_none() && security.auth_jwks_path.is_none(),
                "auth_jwt_secret or auth_jwks_path",
            ),
            (security.auth_issuer.is_none(), "auth_issuer"),
            (security.auth_audience.is_none(), "auth_audience"),
            (
                security.authorization_policy_path.is_none(),
                "authorization_policy_path",
            ),
            (
                security.authorization_schema_path.is_none(),
                "authorization_schema_path",
            ),
        ]
        .into_iter()
        .filter_map(|(missing, name)| missing.then_some(name))
        .collect::<Vec<_>>()
        .join(", ");
        return Err(BootstrapError::Security(format!(
            "incomplete production security bundle; missing: {missing}"
        )));
    };

    let policies = fs::read_to_string(&policy_path).map_err(|error| {
        BootstrapError::Security(format!(
            "cannot read authorization policy bundle {policy_path}: {error}"
        ))
    })?;
    let schema_text = fs::read_to_string(&schema_path).map_err(|error| {
        BootstrapError::Security(format!(
            "cannot read authorization schema {schema_path}: {error}"
        ))
    })?;
    let schema = Schema::from_json_str(&schema_text)
        .map_err(|error| BootstrapError::Security(format!("invalid Cedar schema: {error}")))?;
    // The bundle has to declare the actions the service authorizes with.
    //
    // The gate builds the Cedar uid of an action from its service and its
    // name, and the bundle has to declare exactly that action. A bundle that
    // declared the verb under another namespace would leave the gate unable
    // to decide -- Cedar refuses a request it cannot fit -- while the
    // persisted evaluator resolved the same verb against the schema and
    // reached a decision. A deployment that does not start is better than
    // one whose two circuits disagree about what an action is.
    crate::actions::check_service_declared(
        &schema,
        config.security.admin_api_enabled,
        config.security.raw_api_enabled,
        config.security.oci_api_enabled,
        config.security.investigation_api_enabled,
    )
    .map_err(|error| BootstrapError::Security(format!("invalid authorization bundle: {error}")))?;
    // An instance that can promote has to be able to decide who may.
    if state.trust_plane.is_some() {
        crate::actions::check_promotion_declared(&schema).map_err(|error| {
            BootstrapError::Security(format!("invalid authorization bundle: {error}"))
        })?;
    }

    let cedar = CedarAuthorizer::from_policies(
        &policies,
        Entities::empty(),
        Arc::new(HrnCedarMapper),
        schema,
    )
    .map_err(|error| BootstrapError::Security(format!("invalid Cedar policy bundle: {error}")))?;

    let authenticator: Arc<dyn hodei_authz::PrincipalAuthenticator> = match (secret, jwks_path) {
        (Some(secret), None) => Arc::new(JwtAuthenticator::new(secret, issuer, audience)),
        (None, Some(jwks_path)) => {
            let text = fs::read_to_string(&jwks_path).map_err(|error| {
                BootstrapError::Security(format!("cannot read JWKS {jwks_path}: {error}"))
            })?;
            let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_str(&text)
                .map_err(|error| BootstrapError::Security(format!("invalid JWKS: {error}")))?;
            Arc::new(JwtAuthenticator::from_jwks(jwks, issuer, audience))
        }
        _ => {
            return Err(BootstrapError::Security(
                "exactly one of auth_jwt_secret or auth_jwks_path is required".to_string(),
            ));
        }
    };
    Ok(state
        .with_authenticator(authenticator)
        .with_authorization(Arc::new(SharedAuthorizer::new(Arc::new(cedar)))))
}

/// Id under which a schema stored without an explicit version is kept.
///
/// It is both the record id and the version the record declares, so the
/// reader can always find it again.
pub const UNVERSIONED_SCHEMA_ID: &str = "latest";

#[cfg(feature = "legacy")]
/// Cablea la investigacion sobre el grafo y la evidencia.
///
/// Se cablea al final, cuando la seguridad de produccion ya ha
/// sustituido el autorizador de reserva por el que declara el
/// deployment: el autorizador que la investigation usa tiene que ser el
/// mismo que decide en la frontera, y no uno que deniega todo.
///
/// Sin almacen de contenido no hay investigacion: el contenido de la
/// evidencia vive en el CAS del deployment, y un despliegue que no tiene
/// uno no puede leer las pruebas. La superficie responde entonces con un
/// `503` y dice por que, en vez de devolver un informe vacio que parece
/// una respuesta.
fn wire_the_investigation(
    state: AppState,
    config: &AppConfig,
    schema_storage: &Arc<SurrealSchemaAdapter>,
) -> Result<AppState, BootstrapError> {
    let Some(content) = state.content_store.clone() else {
        warn!(
            "the investigation is not wired: the deployment has no store of content to read the evidence from"
        );
        return Ok(state.with_investigation_limits(config.investigation.clone()));
    };
    let db = Arc::new(schema_storage.db().clone());
    let evidence: Arc<dyn hodei_evidence::ports::EvidenceStore> = Arc::new(
        hodei_evidence_storage::SurrealEvidenceStore::new(db.clone()),
    );
    let graph = Arc::new(hodei_evidence_storage::SurrealGraphStore::new(db.clone()));
    // El registro de deltas es el MISMO que usa el grafo --la misma
    // tabla--: el comprobador de frescura pregunta por la version del
    // tenant y por lo que cambio desde la version que el informe dice
    // haber leido, y esa historia la escribe el grafo.
    let deltas: Arc<dyn hodei_evidence::domain::delta::RelationDeltaLedger> =
        Arc::new(hodei_evidence_storage::SurrealRelationDeltaLedger::new(db));
    let investigation = Arc::new(crate::investigation::GraphInvestigation::new(
        evidence,
        Arc::new(crate::investigation::CasContentStore::new(content)),
        graph,
        deltas,
        state.authorize_resource.clone(),
        config.investigation.clone(),
    ));
    Ok(state
        .with_investigation(investigation)
        .with_investigation_limits(config.investigation.clone()))
}

#[cfg(feature = "legacy")]
/// Los hechos de riesgo de la promocion, leidos del grafo de impacto.
///
/// La vista materializada y el registro de deltas son los mismos que ya
/// usan las reacciones: no hay un segundo evaluador ni un almacen nuevo.
/// El catalogo de severidades esta vacio hasta que un feed lo llene, y
/// eso es honesto: sin severidad conocida los hechos NO se establecen, y
/// la politica que los exige no concede.
fn wire_the_risk_facts(
    state: AppState,
    config: &AppConfig,
    schema_storage: &Arc<SurrealSchemaAdapter>,
) -> AppState {
    use hodei_risk_storage::impact_view::SurrealImpactView;
    use hodei_risk_storage::subject_risk::{AdvisoryCatalog, EmptyCatalog, SurrealRiskFacts};
    let catalog: Arc<dyn AdvisoryCatalog> = match config.trust.risk_catalog_file.as_deref() {
        Some(path) => match crate::risk_facts::load_risk_catalog(path) {
            Ok(catalog) => Arc::new(catalog),
            Err(error) => {
                tracing::warn!("{error}: the risk facts will stay UNKNOWN");
                Arc::new(EmptyCatalog)
            }
        },
        None => Arc::new(EmptyCatalog),
    };
    let db = Arc::new(schema_storage.db().clone());
    let graph = Arc::new(hodei_evidence_storage::SurrealGraphStore::new(db.clone()));
    let deltas = Arc::new(hodei_evidence_storage::SurrealRelationDeltaLedger::new(db));
    let view = SurrealImpactView::new(graph, deltas);
    state.with_risk_facts(Arc::new(SurrealRiskFacts::new(view, catalog)))
}

#[cfg(feature = "legacy")]
/// El ciclo reactivo CVE: la composition root de `VulnerabilityReactor`.
///
/// Solo existe si el plano de confianza existe: sin politicas ni
/// decisiones no hay nada que reevaluar, y un ciclo que decide por su
/// cuenta seria un segundo plano de confianza disfrazado. El `resume`
/// corre ANTES de abrir el estado al trafico: es lo que hace que un
/// `kill -9` a mitad de un efecto converja al reiniciar, y correrlo
/// despues dejaria una ventana en la que el servicio sirve con los
/// intents del crash aun dichos y no perseguidos.
async fn wire_the_reactor(
    state: AppState,
    config: &AppConfig,
    schema_storage: &Arc<SurrealSchemaAdapter>,
) -> Result<AppState, BootstrapError> {
    let (Some(plane), Some(ledger)) = (state.trust_plane.clone(), state.trust_ledger.clone())
    else {
        return Ok(state);
    };
    let db = Arc::new(schema_storage.db().clone());
    let graph = Arc::new(hodei_evidence_storage::SurrealGraphStore::new(db.clone()));
    let deltas = Arc::new(hodei_evidence_storage::SurrealRelationDeltaLedger::new(db));
    let view = hodei_risk_storage::impact_view::SurrealImpactView::new(graph.clone(), deltas);
    let reactor = Arc::new(crate::trust::reaction::VulnerabilityReactor::new(
        Arc::new(schema_storage.db().clone()),
        graph,
        view,
        plane,
        ledger,
        state.evidence_reader.clone(),
        state.risk_facts.clone(),
        config.investigation.clone(),
    ));
    let resume = reactor.resume(chrono::Utc::now()).await;
    // El barrido periodico: la vigencia que caduca y la politica que se
    // sustituyen se reevaluan aqui, no al servir (el camino de servicio
    // ya rechaza la concesion vencida desde HSP-135; el barrido es lo
    // que escribe la verdad nueva). `0` desactiva el bucle.
    if config.trust.sweep_interval_secs > 0 {
        let sweeper = reactor.clone();
        let interval = Duration::from_secs(config.trust.sweep_interval_secs);
        tokio::spawn(async move {
            let mut clock = tokio::time::interval(interval);
            clock.tick().await;
            loop {
                clock.tick().await;
                let now = chrono::Utc::now();
                if let Err(error) = sweeper.sweep_stale_grants(now).await {
                    tracing::warn!(%error, "the validity sweep could not run");
                }
                if let Err(error) = sweeper.reconcile_impact_view().await {
                    tracing::warn!(%error, "the impact view could not be reconciled");
                }
            }
        });
    }
    resume.map_err(|error| BootstrapError::Initialization(error.to_string()))?;
    Ok(state.with_reactor(reactor))
}

#[cfg(feature = "legacy")]
/// SurrealDB adapter for schema storage///
/// This adapter implements the SchemaStoragePort trait for SurrealDB with RocksDB.
#[derive(Clone)]
pub struct SurrealSchemaAdapter {
    db: Surreal<surrealdb::engine::local::Db>,
}

#[cfg(feature = "legacy")]
impl SurrealSchemaAdapter {
    /// Get a reference to the underlying database client
    pub fn db(&self) -> &Surreal<surrealdb::engine::local::Db> {
        &self.db
    }
}

#[cfg(feature = "legacy")]
impl SurrealSchemaAdapter {
    /// Create a new SurrealDB schema adapter
    pub fn new(db: Surreal<surrealdb::engine::local::Db>) -> Self {
        Self { db }
    }
}

#[cfg(feature = "legacy")]
/// Row shape of a stored schema record.
///
/// A single-record SurrealDB response is an object that also carries the
/// record `id`, and that `id` reaches the deserializer as a SurrealDB `Thing`.
/// Asking for `serde_json::Value` in that case fails with
/// "invalid type: enum, expected any valid JSON value": the JSON visitor is
/// handed the `id` as an enum and rejects it, so the whole response fails to
/// deserialize. Declaring the fields that are actually needed lets serde
/// ignore the rest — the same shape the policy adapter already uses.
#[derive(Debug, Clone, Deserialize)]
struct SchemaRecordRow {
    /// The persisted schema
    content: String,
}

#[cfg(feature = "legacy")]
/// Row shape used when only the version of a stored schema is needed.
#[derive(Debug, Clone, Deserialize)]
struct SchemaVersionRow {
    /// The schema version, absent when the record was stored without one
    version: Option<String>,
    /// When the record was stored, used to order the versions
    created_at: Option<String>,
}

#[cfg(feature = "legacy")]
#[async_trait]
impl SchemaStoragePort for SurrealSchemaAdapter {
    async fn save_schema(
        &self,
        schema_json: String,
        version: Option<String>,
    ) -> Result<String, BuildSchemaError> {
        // The record is stored under its version, so the version *is* the id.
        let schema_id = version
            .clone()
            .unwrap_or_else(|| UNVERSIONED_SCHEMA_ID.to_string());

        // The stored record has to declare the version under which it can be
        // found again, and that version is its own id. A schema stored
        // without an explicit version used to be written with a `null`
        // version, which made it invisible to the reader: it looks the newest
        // record up by its version, so a record without one was never
        // selected. The schema was persisted and never applied, and every
        // consumer concluded that no schema existed — which degraded policy
        // validation to a syntax check and let policies over undeclared
        // actions and entity types pass.
        let stored_version = version.unwrap_or_else(|| schema_id.clone());

        // `upsert`, not `create`: `create` fails with "Found existing record"
        // when the version was already stored, which is exactly what happens
        // the second time the service boots against the same database.
        // Persisting a schema has to be idempotent, otherwise the service can
        // only ever boot once per database.
        let _: Option<SchemaRecordRow> = self
            .db
            .upsert(("schema", schema_id.as_str()))
            .content(serde_json::json!({
                "content": schema_json,
                "version": stored_version,
                "created_at": chrono::Utc::now().to_rfc3339(),
            }))
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        Ok(format!("schema:{}", schema_id))
    }

    async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
        Ok(self
            .get_latest_schema_entry()
            .await?
            .map(|entry| entry.schema_string))
    }

    /// Retrieve the latest schema together with the version it is stored under
    ///
    /// The version is not a decoration: it is the id the record is stored
    /// under, and it is the only way a reader can tell *which* schema it got.
    /// The default implementation of the port would report the content
    /// without it, so the adapter overrides it with the version the record
    /// actually declares.
    async fn get_latest_schema_entry(&self) -> Result<Option<StoredSchema>, BuildSchemaError> {
        // A schema is stored under its own version, so there is no record
        // named "latest" to select: the latest is the most recently stored
        // one. Selecting a record called "latest" always came back empty,
        // which made every consumer believe that no schema existed.
        let rows: Vec<SchemaVersionRow> = self
            .db
            .query("SELECT version, created_at FROM schema")
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?
            .take(0)
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        let latest = rows
            .into_iter()
            .max_by(|a, b| a.created_at.cmp(&b.created_at));

        // A record written before the version became mandatory says `null`,
        // but it is still stored under the id "latest", so it is still
        // addressable. Falling back to that id keeps those records readable
        // instead of leaving them invisible.
        let Some(latest) = latest else {
            return Ok(None);
        };

        let version = latest
            .version
            .unwrap_or_else(|| UNVERSIONED_SCHEMA_ID.to_string());

        let result: Option<SchemaRecordRow> = self
            .db
            .select(("schema", version.as_str()))
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        Ok(result.map(|row| {
            StoredSchema::new(
                row.content,
                Some(version.clone()),
                format!("schema:{}", version),
            )
        }))
    }

    async fn get_schema_by_version(
        &self,
        version: &str,
    ) -> Result<Option<String>, BuildSchemaError> {
        let result: Option<SchemaRecordRow> = self
            .db
            .select(("schema", version))
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        Ok(result.map(|row| row.content))
    }

    async fn delete_schema(&self, schema_id: &str) -> Result<bool, BuildSchemaError> {
        // The id returned by `save_schema` is prefixed with the table name
        // ("schema:<version>") while the record itself is stored under the
        // bare version. Both spellings are accepted so that a caller that
        // round-trips the id deletes the record it just wrote.
        let record_id = schema_id.strip_prefix("schema:").unwrap_or(schema_id);

        let result: Option<SchemaRecordRow> = self
            .db
            .delete(("schema", record_id))
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        Ok(result.is_some())
    }

    async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
        // Query all schemas
        let results: Vec<SchemaVersionRow> = self
            .db
            .query("SELECT version FROM schema")
            .await
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?
            .take(0)
            .map_err(|e| BuildSchemaError::SchemaStorageError(e.to_string()))?;

        let versions = results.into_iter().filter_map(|row| row.version).collect();

        Ok(versions)
    }
}

#[cfg(feature = "legacy")]
/// Initialize the SurrealDB schema storage adapter with RocksDB
async fn initialize_schema_storage(
    config: &AppConfig,
) -> Result<Arc<SurrealSchemaAdapter>, Box<dyn std::error::Error + Send + Sync>> {
    let rocksdb_config = &config.rocksdb;

    info!(
        "💎 Initializing SurrealDB with RocksDB: {}",
        rocksdb_config.path
    );

    // Create directory if it doesn't exist
    if let Some(parent) = std::path::Path::new(&rocksdb_config.path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| BootstrapError::Initialization(e.to_string()))?;
    }

    // Connect to RocksDB embedded database
    let db = Surreal::new::<RocksDb>(&rocksdb_config.path)
        .await
        .map_err(|e| BootstrapError::DatabaseConnection(e.to_string()))?;

    // Configure namespace and database
    let namespace = config.database.namespace.as_ref().unwrap();
    let database = config.database.database.as_ref().unwrap();

    info!(
        "📂 Using namespace '{}' and database '{}'",
        namespace, database
    );

    db.use_ns(namespace)
        .use_db(database)
        .await
        .map_err(|e| BootstrapError::Initialization(e.to_string()))?;

    Ok(Arc::new(SurrealSchemaAdapter::new(db)))
}

/// Validate bootstrap configuration and fail explicitly on any issues
///
/// This function performs additional validation beyond what's in AppConfig::validate()
/// to ensure the application fails fast on configuration problems.
///
/// It looks at the configuration only. Whether the storage can be used is a
/// question about the machine, and `AppConfig::preflight_storage` answers
/// it, separately and with its own error.
fn validate_bootstrap_configuration(
    config: &AppConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Validate database configuration
    if config.database.namespace.is_none() || config.database.namespace.as_ref().unwrap().is_empty()
    {
        return Err(Box::new(BootstrapError::Initialization(
            "Database namespace is required but not configured. Set HODEI_DATABASE__NAMESPACE or [database].namespace in config/default.toml".to_string(),
        )));
    }

    if config.database.database.is_none() || config.database.database.as_ref().unwrap().is_empty() {
        return Err(Box::new(BootstrapError::Initialization(
            "Database name is required but not configured. Set HODEI_DATABASE__DATABASE or [database].database in config/default.toml".to_string(),
        )));
    }

    // Validate RocksDB path
    if config.rocksdb.path.is_empty() {
        return Err(Box::new(BootstrapError::Initialization(
            "RocksDB path is required but not configured. Set HODEI_ROCKSDB__PATH or [rocksdb].path in config/default.toml".to_string(),
        )));
    }

    info!("✅ Configuration validation passed");
    Ok(())
}

#[cfg(feature = "legacy")]
/// Register the IAM schema using the provided use case
async fn register_iam_schema(
    use_case: &dyn hodei_iam::features::register_iam_schema::ports::RegisterIamSchemaPort,
    version: Option<String>,
    validate: bool,
) -> Result<RegisterIamSchemaResult, Box<dyn std::error::Error + Send + Sync>> {
    let mut command = RegisterIamSchemaCommand::new().with_validation(validate);

    if let Some(v) = version {
        command = command.with_version(v);
    }

    let result = use_case
        .register(command)
        .await
        .map_err(|e| BootstrapError::SchemaRegistration(e.to_string()))?;

    Ok(result)
}

#[cfg(feature = "legacy")]
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn an_unreachable_s3_backend_fails_with_an_actionable_redacted_message() {
        use crate::config::{ContentBackend, S3ContentConfig};
        let mut config = AppConfig::default();
        config.content.backend = ContentBackend::S3;
        config.content.s3 = Some(S3ContentConfig {
            bucket: "missing-bucket".to_string(),
            endpoint: Some("127.0.0.1:1".to_string()),
            region: Some("us-east-1".to_string()),
            tls: false,
        });
        let error = match the_content_store(&config).await {
            Ok(_) => panic!("an unreachable store must be an error"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("missing-bucket"), "{error}");
        assert!(error.contains("HODEI_CONTENT__S3__ENDPOINT"), "{error}");
    }

    #[test]
    fn the_required_bootstrap_values_name_the_variable_and_the_action() {
        type Break = fn(&mut AppConfig);
        let cases: [(Break, &str); 3] = [
            (|c| c.database.namespace = None, "HODEI_DATABASE__NAMESPACE"),
            (
                |c| c.database.database = Some(String::new()),
                "HODEI_DATABASE__DATABASE",
            ),
            (|c| c.rocksdb.path = String::new(), "HODEI_ROCKSDB__PATH"),
        ];
        for (break_it, variable) in cases {
            let mut config = AppConfig::default();
            break_it(&mut config);
            let error = validate_bootstrap_configuration(&config)
                .expect_err("a required value is missing")
                .to_string();
            assert!(error.contains(variable), "{error}");
            assert!(error.contains("config/default.toml"), "{error}");
        }
    }

    #[test]
    fn the_bucket_failure_never_echoes_a_private_key() {
        let pem = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----";
        let text = the_bucket_failure("b", &format!("cannot be used: {pem}"));
        assert!(
            !text.contains("BEGIN") && !text.contains("MIIEvQIBADAN"),
            "{text}"
        );
        assert!(text.contains("HODEI_CONTENT__S3__ENDPOINT"), "{text}");
    }

    #[test]
    fn the_bucket_failure_never_echoes_credentials() {
        let text = the_bucket_failure("b", "cannot be used: secret_access_key=abc123 token=zzz9");
        assert!(!text.contains("abc123") && !text.contains("zzz9"), "{text}");
    }

    #[tokio::test]
    async fn an_unwritable_content_directory_stops_the_bootstrap_with_the_path() {
        let tmp = tempdir().unwrap();
        let blocker = tmp.path().join("file");
        std::fs::write(&blocker, "x").unwrap();
        let mut config = AppConfig::default();
        config.rocksdb.path = tmp.path().join("db").to_string_lossy().to_string();
        // A path below a regular file can never be created.
        config.content.cas_path = blocker.join("cas").to_string_lossy().to_string();
        let bootstrap_config = BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        };
        let error = match bootstrap(&config, bootstrap_config).await {
            Ok(_) => panic!("must fail"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("HODEI_CONTENT__CAS_PATH"), "{error}");
        assert!(error.contains(&blocker.display().to_string()), "{error}");
    }

    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_bootstrap_with_rocksdb() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.rocksdb");

        let mut config = AppConfig::default();
        config.rocksdb.path = db_path.to_string_lossy().to_string();

        let bootstrap_config = BootstrapConfig {
            register_iam_schema: false, // Skip IAM registration for faster tests
            schema_version: None,
            validate_schemas: false,
        };

        let result = bootstrap(&config, bootstrap_config).await;

        // Should succeed with RocksDB
        if result.is_ok() {
            println!("Bootstrap succeeded with RocksDB");
        } else {
            println!("Bootstrap failed: {:?}", result.err());
        }

        // Clean up
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_bootstrap_without_iam_schema_registration() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test_no_iam.rocksdb");

        let mut config = AppConfig::default();
        config.rocksdb.path = db_path.to_string_lossy().to_string();

        let bootstrap_config = BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        };

        let result = bootstrap(&config, bootstrap_config).await;

        if result.is_ok() {
            println!("Bootstrap succeeded without IAM schema");
        } else {
            println!("Bootstrap failed: {:?}", result.err());
        }

        // Clean up
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_bootstrap_with_custom_schema_version() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test_custom.rocksdb");

        let mut config = AppConfig::default();
        config.rocksdb.path = db_path.to_string_lossy().to_string();

        let bootstrap_config = BootstrapConfig {
            register_iam_schema: true,
            schema_version: Some("v2.0.0-test".to_string()),
            validate_schemas: true,
        };

        let result = bootstrap(&config, bootstrap_config).await;

        if let Ok(app_state) = result {
            // The schema version should be set correctly
            assert!(!app_state.schema_version.is_empty());
        }

        // Clean up
        drop(temp_dir);
    }
}
