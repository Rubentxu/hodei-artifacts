//! Bootstrap module for Hodei Artifacts API
//!
//! This module handles the initialization of the application, including:
//! - Dgraph profile assembly (the single authority of the metadata)
//! - Infrastructure adapter creation
//! - Use case composition via CompositionRoot
//! - Optional IAM schema registration

use crate::app_state::AppState;
use crate::auth::JwtAuthenticator;
use crate::authorization::SharedAuthorizer;
use crate::config::AppConfig;
use cedar_policy::{Entities, Schema};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_hub::blob::FileBlobStore;
use std::fs;

use hodei_iam::register_iam_schema::dto::{RegisterIamSchemaCommand, RegisterIamSchemaResult};
use hodei_policies::build_schema::ports::{AUTHORIZATION_BUNDLE_SCHEMA_ID, SchemaStoragePort};
use std::sync::Arc;
use std::time::Duration;
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
/// 2. Refuses a configuration that does not declare the `[graph]` authority
/// 3. Assembles the Dgraph profile (stores, use cases, content store)
/// 4. Optionally registers the IAM schema
/// 5. Returns the configured AppState ready for Axum
pub async fn bootstrap(
    config: &AppConfig,
    bootstrap_config: BootstrapConfig,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    info!("🚀 Starting Hodei Artifacts API bootstrap");

    // A configuration without `[graph]` is the retired embedded profile: it
    // is refused with the way out before anything is validated or touched
    // (not even the content path is created); there is no other profile to
    // fall back to.
    if config.graph.is_none() {
        return Err(crate::config::GRAPH_PROFILE_REQUIRED.to_string().into());
    }

    // Step 0: Validate configuration and fail explicitly on any issues
    info!("🔍 Validating application configuration");
    config
        .security
        .validate_production_auth()
        .map_err(|error| BootstrapError::Security(error.to_string()))?;
    info!("✅ Configuration validation passed");
    // The storage preflight is separate from the validation on purpose: the
    // validation looks at the configuration and cannot fail because of the
    // machine, while the preflight looks at the machine and says so when it
    // is the machine that is wrong.
    config
        .preflight_storage()
        .map_err(|error| BootstrapError::Initialization(format!("storage preflight: {error}")))?;

    // Step 1: the profile. The graph is the only authority of the metadata
    // (HSP-209, HSP-314 F4).
    info!("📦 Initializing infrastructure adapters");
    bootstrap_graph_profile(config, bootstrap_config).await
}

/// Build the store of content the configuration names and clean the staging
/// an interrupted upload left behind.
///
/// The boot goes through
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
    let margin = Duration::from_secs(the_staging_margin_secs(config));
    let cleanup = match chosen {
        ChosenContentStore::Filesystem(ref directory) => directory.clean_expired_staging(margin),
        ChosenContentStore::S3(ref store) => store.clean_staging(margin).await,
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

/// The age past which the boot may remove an abandoned staging.
///
/// A resumable OCI session (HSP-201) outlives the process that wrote it: the
/// journal keeps the session open for `oci_upload_ttl_secs` after its last
/// chunk, and the staging is what the resume writes into. Removing a staging
/// younger than that TTL breaks a session the journal still serves, so the
/// margin is never shorter than the TTL plus a minute of slack (the journal
/// stamps the session slightly after the chunk reached the file).
fn the_staging_margin_secs(config: &AppConfig) -> u64 {
    let configured = config.content.staging_max_age_secs;
    let ttl = config.security.oci_upload_ttl_secs;
    if configured < ttl {
        warn!(
            configured,
            ttl,
            "staging_max_age_secs is shorter than the OCI upload TTL: the boot cleanup keeps \
             the staging of a session that is still resumable"
        );
    }
    configured.max(ttl.saturating_add(60))
}

/// Bootstrap the deployment whose authority is the graph (HSP-209).
///
/// This is the single profile of the service: every store is assembled by
/// [`crate::profile`] over one connected, logged-in, schema-guarded client.
/// There is no fallback: if the alpha, the CAS, or any store cannot open,
/// the boot dies with that store's own error.
///
/// What the profile does NOT carry, and says so instead of degrading:
///
/// - Nothing about the Cedar schema is missing: `SchemaStoragePort` is
///   implemented over Dgraph (`DgraphSchemaStore`), the bundle schema is
///   persisted, and the IAM schema registration runs when it is asked for.
/// - The investigation and the `/repositories` registry have no port over
///   Dgraph yet: the surfaces answer with their default closed behavior.
///   (The evidence reader, the risk facts and the CVE reactor are wired over
///   Dgraph: HSP-314 F3a/F3b.)
async fn bootstrap_graph_profile(
    config: &AppConfig,
    bootstrap_config: BootstrapConfig,
) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    info!("🕸️  Starting the Dgraph profile: the alpha is the single authority of the metadata");

    // Step 0 already ran: the security bundle, the configuration
    // validation and the storage preflight.

    // Step 1: the authority itself. Connection, ACL login and the versioned
    // schema guard (v7) inside the IAM adapter's open: no store of the
    // profile exists over a server this binary cannot speak to.
    let graph = crate::profile::GraphAuthority::connect(config).await?;

    // The Cedar schema store of the profile: one tenant of the graph, the
    // same authority as every other store. The bundle schema the deployment
    // declares is persisted under its well-known id, so the gate and the
    // persisted evaluator validate against ONE schema.
    let schema_store = Arc::new(hodei_iam::infrastructure::DgraphSchemaStore::new(
        graph.client.clone(),
        graph.tenant.as_str(),
    ));
    if let Some(schema_path) = &config.security.authorization_schema_path {
        persist_bundle_schema(schema_store.as_ref(), schema_path).await?;
    }
    let authoritative_schema_id = config
        .security
        .authorization_schema_path
        .as_ref()
        .map(|_| AUTHORIZATION_BUNDLE_SCHEMA_ID.to_string());

    // Step 2: the bytes. The content store decision and its readiness: an S3 deployment that
    // cannot reach its bucket does not boot, and never writes the
    // filesystem CAS on the side.
    let (content, content_check) = open_content_store(config).await?;

    // Step 3: the use cases over the graph. The IAM ports of the
    // composition root are the Dgraph adapter's, and the schema provider is
    // the Dgraph schema store.
    let root = crate::profile::composition_root_for_graph(
        graph.iam.clone(),
        schema_store,
        authoritative_schema_id,
    );
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
    let app_state = AppState::from_composition_root(schema_version.clone(), root);

    // Step 4: the vertical stores, all over the same client, with the
    // failure of any of them stopping the boot.
    let app_state =
        crate::profile::assemble_graph_profile(app_state, config, &graph, content).await?;

    // The tail: the body limit, the quotas (the
    // in-memory ledger; the durable
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
    // The investigation limits travel without the surface: the endpoint
    // answers with its default closed behavior and no local engine is
    // opened to give it an answer.
    let app_state = app_state.with_investigation_limits(config.investigation.clone());
    let app_state = crate::public_url_wiring::wire_public_url_and_signing(app_state, config)?;
    let app_state = crate::risk_facts::wire_graph_risk_facts(app_state, config, &graph).await?;
    let app_state = crate::evidence_dgraph::wire_graph_evidence_reader(app_state, &graph).await?;
    let app_state = crate::reactor_dgraph::wire_graph_reactor(app_state, config, &graph).await?;
    let app_state =
        crate::investigation_dgraph::wire_graph_investigation(app_state, config, &graph).await?;
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
            // The store's readiness check: the directory
            // exists and is a directory. Nothing inside is read.
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
            // The store's readiness check: `head_bucket`,
            // the cheapest call there is. The network budget is
            // the probe's: a bucket that does not answer produces a
            // `timeout`, not a hanging request.
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

/// Persist the Cedar schema the deployment's authorization bundle declares,
/// in the envelope the build-schema use case writes, under the id that
/// makes it the authoritative one.
async fn persist_bundle_schema(
    storage: &dyn SchemaStoragePort,
    schema_path: &str,
) -> Result<(), BootstrapError> {
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
    storage
        .save_schema(
            envelope.to_string(),
            Some(AUTHORIZATION_BUNDLE_SCHEMA_ID.to_string()),
        )
        .await
        .map_err(|error| BootstrapError::Security(format!("cannot persist schema: {error}")))?;
    Ok(())
}

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

#[cfg(test)]
mod staging_margin_tests {
    use super::*;

    /// The cleanup margin never undercuts the life of a resumable session.
    #[test]
    fn the_staging_margin_never_undercuts_the_upload_ttl() {
        let mut config = AppConfig::default();
        config.content.staging_max_age_secs = 3600;
        config.security.oci_upload_ttl_secs = 86_400;
        assert_eq!(the_staging_margin_secs(&config), 86_460);
        config.security.oci_upload_ttl_secs = 600;
        assert_eq!(the_staging_margin_secs(&config), 3600);
    }
}
