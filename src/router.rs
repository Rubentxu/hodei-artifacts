//! HTTP routing for the Hodei Artifacts API.
//!
//! The router is built once during bootstrap and owns:
//! - the health probes (`/health`, `/health/ready`, `/health/live`)
//! - the versioned API surface (`/api/v1`)
//! - the OpenAPI / Swagger UI documentation
//! - the cross-cutting middleware (tracing, timeout, CORS)
//!
//! Keeping the router in the library target (instead of `main.rs`) is what
//! allows the HTTP contract to be asserted in-process without spawning a
//! server and binding a port.

use crate::app_state::AppState;
use crate::config::AppConfig;
use crate::handlers::health::{health_check, liveness, readiness};
use crate::openapi::create_api_doc;
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderValue, Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use hodei_authz::{Credentials, VerifiedContext, verify_principal};
use kernel::Hrn;
use std::time::Duration;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    timeout::TimeoutLayer,
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use utoipa_swagger_ui::SwaggerUi;

/// How much room the `multipart` envelope gets on top of the file.
///
/// The limit that applies to an upload is the configured one, and it is the
/// limit of the *file*. The envelope --the boundaries and the headers of
/// the parts-- travels with it, so the transport gets this much more room
/// to read the body before it refuses it. It is a bound and not a
/// reservation: a client that sends fewer parts uses less.
const MULTIPART_ENVELOPE_SLACK: usize = 256 * 1024;

/// Build the Axum router with all routes and middleware.
pub fn build_router(app_state: AppState, config: &AppConfig) -> Router {
    // The administrative surface is closed unless it is explicitly enabled.
    // This service does not authenticate requests yet, so the default must
    // deny before any handler runs.
    let admin_api = admin_v1_routes(app_state.clone()).layer(middleware::from_fn_with_state(
        (config.security.admin_api_enabled, app_state.clone()),
        admin_gate,
    ));
    let repositories =
        repository_v1_routes(app_state.clone(), config.security.investigation_api_enabled);
    let api = admin_api.merge(repositories);
    // The raw content surface is a surface of its own, and it is **not**
    // under `/api/v1`: the administrative API and the endpoint that serves
    // packages are different things, and a client that talks to one has
    // no reason to believe it is talking to the other.
    let raw = if config.security.raw_api_enabled {
        raw_routes(app_state.clone())
    } else {
        Router::new()
    };
    // The OCI distribution pull surface (`/v2`), also a surface of its
    // own: the distribution protocol and the raw content surface serve
    // different clients, and neither is under the administrative API.
    let oci = if config.security.oci_api_enabled {
        oci_routes(app_state.clone())
    } else {
        Router::new()
    };

    // The health surface. The three endpoints answer three different
    // questions: `/health` is the legacy check, `/health/live` says that
    // the process lives and `/health/ready` says that the deployment can
    // serve. The last two are not the same answer, and answering the
    // readiness with the liveness is a lie that takes the deployment out
    // of rotation when it could still recover.
    let health = Router::new()
        .route("/health", get(health_check))
        .route("/health/ready", get(readiness))
        .route("/health/live", get(liveness))
        .with_state(app_state.clone());

    Router::new()
        // Health check endpoints
        .merge(health)
        // The service discovery the Terraform CLI does at the host root:
        // the document that names the module registry of the account. The
        // CLI always fetches it over https, so a plain-http hub declares
        // the service in its CLI config (`host` + `services`) instead.
        .route(
            "/.well-known/terraform.json",
            get(crate::handlers::terraform_modules::discovery).with_state(app_state.clone()),
        )
        // API v1 routes
        .nest("/api/v1", api)
        // The raw content surface.
        .merge(raw)
        // The OCI distribution pull surface.
        .merge(oci)
        // Swagger UI - serve at /swagger-ui
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", create_api_doc()))
        // Middleware layers (applied in reverse order)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<Body>| {
                    let correlation_id = request
                        .headers()
                        .get("x-request-id")
                        .and_then(|value| value.to_str().ok())
                        .filter(|value| {
                            !value.is_empty()
                                && value.len() <= 128
                                && value.chars().all(|character| {
                                    !character.is_control() && !character.is_whitespace()
                                })
                        })
                        .unwrap_or("-");
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        // The path and not the URI: a query carries
                        // credentials --a token, a signed address-- and a
                        // trace is a place where they would stay.
                        uri = %request.uri().path(),
                        correlation_id = %correlation_id,
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(config.server.request_timeout_secs),
        ))
        .layer(cors_layer(config))
}

/// Fail closed on the administrative surface.
///
/// The opt-in only opens the surface when the state has both a real credential
/// verifier and a non-default authorizer. Even then every request must present
/// a bearer credential and receive a Cedar Permit for the concrete route.
/// CORS preflight (`OPTIONS`) is answered by the outer CORS layer and does not
/// reach this gate; no handler runs in that path either.
async fn admin_gate(
    State((enabled, app_state)): State<(bool, AppState)>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    if !enabled {
        return (StatusCode::FORBIDDEN, "administrative API disabled").into_response();
    }
    if !app_state.admin_surface_ready() {
        tracing::error!(
            "administrative API requested but no authenticator/authorizer is configured; keeping it closed"
        );
        return (
            StatusCode::FORBIDDEN,
            "administrative API requires authentication",
        )
            .into_response();
    }

    let token = match crate::handlers::bearer_token(req.headers()) {
        Some(token) => token,
        None => return unauthorized(),
    };
    let principal = match verify_principal(
        app_state.authenticator.as_ref(),
        &Credentials::bearer(token),
    )
    .await
    {
        Ok(principal) => principal,
        Err(_) => return unauthorized(),
    };
    let context = VerifiedContext::from_principal(principal);
    let action = crate::actions::access_admin_api();

    let path = req
        .uri()
        .path()
        .trim_start_matches("/api/v1/")
        .trim_start_matches('/');
    // Include the HTTP method so a policy cannot grant "list" and silently
    // grant "create" on the same path.
    let route = if path.is_empty() {
        "surface".to_string()
    } else {
        path.replace('/', "-")
    };
    let resource_path = format!("{}-{route}", req.method().as_str().to_ascii_lowercase());
    let resource = Hrn::with_region(
        "hodei".to_string(),
        "admin".to_string(),
        "global".to_string(),
        context.account().as_str().to_string(),
        "AdminApi".to_string(),
        resource_path,
    );
    let outcome =
        match app_state
            .authorize_resource
            .execute_canonical(context.clone(), action, &resource)
        {
            Ok(outcome) => outcome,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "authorization unavailable",
                )
                    .into_response();
            }
        };
    let context_for_next = context;
    if !outcome.allows_access() {
        return (StatusCode::FORBIDDEN, "administrative permission denied").into_response();
    }

    // The handlers use the verified account for object-level IAM scope; the
    // context comes from authentication, never from a request field.
    req.extensions_mut().insert(context_for_next);
    next.run(req).await
}

fn unauthorized() -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

/// Build the CORS policy from the explicit origin allowlist.
///
/// An empty allowlist means no cross-origin access. `*` is rejected by
/// `SecurityConfig::validate`, so the permissive layer cannot be reintroduced
/// by configuration.
fn cors_layer(config: &AppConfig) -> CorsLayer {
    if config.security.cors_allowed_origins.is_empty() {
        return CorsLayer::new();
    }

    // Normalize first so a value such as `"*/"` cannot slip past the wildcard
    // guard and reach `AllowOrigin::list`, which panics on a wildcard.
    let normalized: Vec<String> = config
        .security
        .cors_allowed_origins
        .iter()
        .map(|origin| origin.trim_end_matches('/').to_string())
        .collect();

    // `build_router` is public and callers may skip `AppConfig::validate`.
    // Enforce the same invariant here so a wildcard cannot reintroduce the
    // permissive policy through an unvalidated config.
    if normalized.iter().any(|origin| origin.trim() == "*") {
        tracing::error!("CORS wildcard is not allowed; cross-origin access stays closed");
        return CorsLayer::new();
    }

    let origins: Vec<HeaderValue> = normalized
        .iter()
        .filter_map(|origin| match HeaderValue::from_str(origin) {
            Ok(value) => Some(value),
            Err(_) => {
                tracing::error!(
                    origin = %origin,
                    "ignoring invalid CORS origin; cross-origin access stays closed for it"
                );
                None
            }
        })
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        // `HEAD` goes with `GET`: the raw content surface answers to
        // `HEAD`, and a browser that asks for a header cannot be met with
        // a `405` on a method it cannot use.
        .allow_methods([
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::DELETE,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-request-id"),
            // The headers that the raw content surface needs to do what it
            // promises: without them, a browser cannot ask for a range,
            // nor write conditionally, nor declare the digest of what it
            // publishes.
            header::RANGE,
            header::IF_MATCH,
            header::IF_NONE_MATCH,
            header::HeaderName::from_static("x-checksum-sha256"),
        ])
}

/// API v1 routes
fn admin_v1_routes(app_state: AppState) -> Router {
    Router::new()
        // Schema management
        .route(
            "/schemas/build",
            post(crate::handlers::schemas::build_schema),
        )
        .route("/schemas/load", get(crate::handlers::schemas::load_schema))
        .route(
            "/schemas/register-iam",
            post(crate::handlers::schemas::register_iam_schema),
        )
        // Policy validation and evaluation
        .route(
            "/policies/validate",
            post(crate::handlers::policies::validate_policy),
        )
        .route(
            "/policies/evaluate",
            post(crate::handlers::policies::evaluate_policies),
        )
        // IAM Policy Management
        .route("/iam/policies", post(crate::handlers::iam::create_policy))
        .route("/iam/policies", get(crate::handlers::iam::list_policies))
        .route("/iam/policies/get", post(crate::handlers::iam::get_policy))
        .route(
            "/iam/policies/update",
            put(crate::handlers::iam::update_policy),
        )
        .route(
            "/iam/policies/delete",
            delete(crate::handlers::iam::delete_policy),
        )
        .route(
            "/iam/evaluate",
            post(crate::handlers::iam::evaluate_iam_policies),
        )
        // Garbage collection of the store of content. Each route is its own
        // resource for the gate: the plan can be granted without the sweep.
        .route("/gc/plan", get(crate::handlers::gc::plan))
        .route("/gc/sweep", post(crate::handlers::gc::sweep))
        .route(
            "/gc/holds",
            get(crate::handlers::gc::list_holds).post(crate::handlers::gc::place_hold),
        )
        .route("/gc/holds/release", post(crate::handlers::gc::release_hold))
        // The reactive CVE cycle: the admission of an advisory is its own
        // resource for the gate, because whoever holds it can turn off the
        // trusted channel of an artifact (HSP-137).
        .route(
            "/advisories/accept",
            post(crate::handlers::advisories::accept),
        )
        // The status of the effects of the reactive cycle (HSP-323 AC3):
        // read-only, scoped to the verified account, its own resource.
        .route(
            "/reactions/intents",
            get(crate::handlers::reactions::list_intents),
        )
        // Integrity scrub of the registry: read-only, its own resource.
        .route("/scrub", get(crate::handlers::scrub::report))
        // Migration from Nexus/Artifactory: inventory and import, both
        // read-only on the origin.
        .route(
            "/migrations/inventory",
            post(crate::handlers::migration::inventory),
        )
        .route(
            "/migrations/import",
            post(crate::handlers::migration::import),
        )
        // Playground routes
        .route(
            "/playground/evaluate",
            post(crate::handlers::playground::playground_evaluate),
        )
        .merge(policy_studio_routes())
        // TODO: Add more routes as needed
        // .route("/users", post(crate::handlers::users::create_user))
        // .route("/users/:id", get(crate::handlers::users::get_user))
        // .route("/groups", post(crate::handlers::groups::create_group))
        .with_state(app_state)
}

/// The routes of the OCI distribution surface.
///
/// The path is read **raw**, from the URI as it arrived: a `%2e%2e` that
/// has already become `..` cannot be refuted by an extractor that decoded
/// it. The handler validates the name before it looks anywhere.
///
/// The pull and the push share the same path because the protocol
/// shares it: `GET /v2/<name>/blobs/<digest>` reads and
/// `POST /v2/<name>/blobs/uploads/` writes. The dispatcher decides by
/// method and by path, and the pull is never confused with the push.
fn oci_routes(app_state: AppState) -> Router {
    Router::new()
        .route(
            "/v2/",
            axum::routing::get(crate::handlers::oci::version_check),
        )
        .route(
            "/v2/{*target}",
            axum::routing::get(crate::handlers::oci::dispatch)
                .head(crate::handlers::oci::dispatch)
                .post(crate::handlers::oci::dispatch)
                .patch(crate::handlers::oci::dispatch)
                .put(crate::handlers::oci::dispatch)
                .delete(crate::handlers::oci::dispatch),
        )
        // The token endpoint: where the Docker client goes for its
        // credential. It lives with the distribution surface because it is
        // part of its protocol, and it is opened and closed with it.
        .route(
            crate::oci::token::TOKEN_PATH,
            axum::routing::get(crate::oci::token::token).post(crate::oci::token::token),
        )
        .with_state(app_state)
}

/// The routes of the raw content surface.
///
/// The path is read **raw**, from the URI as it arrived, and not from the
/// `Path` extractor: the extractor decodes, and a `%2e%2e` that has
/// already become `..` cannot be refuted. That is why the path is
/// declared as a wildcard and read by the rules of `crate::raw::path`.
fn raw_routes(app_state: AppState) -> Router {
    Router::new()
        .route(
            "/repository/{*target}",
            axum::routing::put(crate::handlers::raw::publish)
                .get(crate::handlers::raw::download)
                .head(crate::handlers::raw::describe),
        )
        // The npm registry surface. It is read raw as well: the name of a
        // scoped package arrives as `@scope%2fname` and only the protocol
        // grammar may decode it.
        .route(
            "/npm/{*target}",
            axum::routing::get(crate::handlers::npm::handle)
                .head(crate::handlers::npm::handle)
                .put(crate::handlers::npm::handle)
                .delete(crate::handlers::npm::handle)
                // Any other method gets the same JSON error as the rest.
                .fallback(crate::handlers::npm::handle),
        )
        // The PyPI surface: `twine upload` and `pip install`. The project
        // name arrives normalized and the file name percent-encoded, and
        // only the protocol grammar may decode them.
        .route(
            "/pypi/{*target}",
            axum::routing::get(crate::handlers::pypi::handle)
                .head(crate::handlers::pypi::handle)
                .post(crate::handlers::pypi::handle)
                // Any other method gets the same error as the rest.
                .fallback(crate::handlers::pypi::handle)
                // The default limit of `axum` is 2 MiB, and a distribution
                // does not fit in 2 MiB. The limit that applies is the
                // configured one, and it is enforced in two layers: the
                // transport caps the whole body --the `multipart` envelope
                // included-- and the handler caps the bytes of the file.
                // Without the transport cap, an envelope with gigantic
                // headers would pile up in memory with no limit.
                .layer(axum::extract::DefaultBodyLimit::max(
                    app_state
                        .max_body_size
                        .saturating_add(MULTIPART_ENVELOPE_SLACK),
                )),
        )
        // The Cargo sparse registry: `GET config.json`, the deterministic
        // index paths, the download and `PUT api/v1/crates/new`. The path
        // is the protocol grammar's own, so it arrives raw and the handler
        // is the one that decodes it.
        .route(
            "/cargo/{*target}",
            axum::routing::get(crate::handlers::cargo::handle)
                .put(crate::handlers::cargo::handle)
                // Any other method gets the same JSON error as the rest.
                .fallback(crate::handlers::cargo::handle)
                // A publish body carries the whole .crate, so the default
                // limit of `axum` would refuse it before the protocol.
                .layer(axum::extract::DefaultBodyLimit::max(
                    app_state
                        .max_body_size
                        .saturating_add(MULTIPART_ENVELOPE_SLACK),
                )),
        )
        // Debian APT: the pool of `.deb`, the generated `Packages` and
        // `Release`, its `Release.gpg` and the hub key. Paths are the
        // format's own and arrive raw to the handler.
        .route(
            "/apt/{*target}",
            axum::routing::get(crate::handlers::apt::handle)
                .put(crate::handlers::apt::handle)
                .fallback(crate::handlers::apt::handle)
                .layer(axum::extract::DefaultBodyLimit::max(
                    app_state
                        .max_body_size
                        .saturating_add(MULTIPART_ENVELOPE_SLACK),
                )),
        )
        // RPM/yum: the pool of `.rpm`, the generated `primary` index and
        // `repomd`, its `repomd.xml.asc` and the hub key. Same contract
        // as apt for raw paths and body limits.
        .route(
            "/yum/{*target}",
            axum::routing::get(crate::handlers::yum::handle)
                .put(crate::handlers::yum::handle)
                .fallback(crate::handlers::yum::handle)
                .layer(axum::extract::DefaultBodyLimit::max(
                    app_state
                        .max_body_size
                        .saturating_add(MULTIPART_ENVELOPE_SLACK),
                )),
        )
        // The Terraform/OpenTofu provider network mirror: `GET index.json`,
        // `{hostname}/{namespace}/{type}/index.json`, `{version}.json` and
        // the provider zips. Read-only: the mirror protocol has no writes,
        // and any other method gets the same JSON error as the rest.
        .route(
            "/terraform/{*target}",
            axum::routing::get(crate::handlers::terraform::handle)
                .fallback(crate::handlers::terraform::handle)
                .layer(axum::extract::DefaultBodyLimit::max(
                    app_state
                        .max_body_size
                        .saturating_add(MULTIPART_ENVELOPE_SLACK),
                )),
        )
        // The Conan 2 remote surface: the v2 protocol with revisions
        // (`GET /v1/ping`, `GET /v2/users/authenticate`, search, snapshots
        // and the `PUT` of recipe and package files). The path is the
        // protocol grammar's own, so it arrives raw and the handler is the
        // one that decodes it.
        .route(
            "/conan/{*target}",
            axum::routing::get(crate::handlers::conan::handle)
                .head(crate::handlers::conan::handle)
                .put(crate::handlers::conan::handle)
                .fallback(crate::handlers::conan::handle)
                // A conan_package.tgz is a whole package, so the default
                // limit of `axum` would refuse it before the protocol.
                .layer(axum::extract::DefaultBodyLimit::max(
                    app_state
                        .max_body_size
                        .saturating_add(MULTIPART_ENVELOPE_SLACK),
                )),
        )
        // The Ansible Galaxy surface: the collections API v3 (`GET /api/`,
        // collection and version documents, the paginated versions, the
        // artifact bytes and the `POST` publish form). The path is the
        // protocol grammar's own, so it arrives raw and the handler is the
        // one that decodes it.
        .route(
            "/galaxy/{*target}",
            axum::routing::get(crate::handlers::galaxy::handle)
                .head(crate::handlers::galaxy::handle)
                .post(crate::handlers::galaxy::handle)
                .fallback(crate::handlers::galaxy::handle)
                // A collection tarball is a whole artifact, so the default
                // limit of `axum` would refuse it before the protocol.
                .layer(axum::extract::DefaultBodyLimit::max(
                    app_state
                        .max_body_size
                        .saturating_add(MULTIPART_ENVELOPE_SLACK),
                )),
        )
        // The Go module proxy: `GET {module}/@v/list|.info|.mod|.zip` and
        // `{module}/@latest`, plus the `PUT` of those same paths that
        // publishes into a hosted repository. Module paths carry `!` and
        // `%2f` escapes that must not be decoded by the router.
        .route(
            "/go/{*target}",
            axum::routing::get(crate::handlers::go::handle)
                .put(crate::handlers::go::handle)
                .fallback(crate::handlers::go::handle)
                // A module zip is a whole source tree, so the default
                // limit of `axum` would refuse it before the protocol.
                .layer(axum::extract::DefaultBodyLimit::max(
                    app_state
                        .max_body_size
                        .saturating_add(MULTIPART_ENVELOPE_SLACK),
                )),
        )
        // The Hugging Face surface: the read dialect of the hub (`GET
        // /api/models/.../revision/...`, the `HEAD` resolve with its
        // relative `Location` and the streamed blob). Read-only: the
        // upload of the dialect is not declared in this release, and any
        // other method gets the same JSON error as the rest. The path is
        // the protocol grammar's own, so it arrives raw and the handler
        // is the one that decodes it.
        .route(
            "/hf/{*target}",
            axum::routing::get(crate::handlers::hf::handle)
                .head(crate::handlers::hf::handle)
                .fallback(crate::handlers::hf::handle),
        )
        // The NuGet v3 surface: the service index, the flat container,
        // the registration leaf and the `PUT package/{id}/{version}` that
        // pushes a nupkg. Package ids and versions arrive raw: the
        // handler is the one that normalizes them.
        .route(
            "/nuget/{*target}",
            axum::routing::get(crate::handlers::nuget::handle)
                .put(crate::handlers::nuget::handle)
                .fallback(crate::handlers::nuget::handle)
                // A nupkg is a whole package, so the default limit of
                // `axum` would refuse it before the protocol.
                .layer(axum::extract::DefaultBodyLimit::max(
                    app_state
                        .max_body_size
                        .saturating_add(MULTIPART_ENVELOPE_SLACK),
                )),
        )
        .with_state(app_state)
}

/// Repository routes have their own credential/authorization gate; they are
/// deliberately not behind `admin_api_enabled`.
fn repository_v1_routes(app_state: AppState, investigation_enabled: bool) -> Router {
    Router::new()
        .route(
            "/repositories",
            get(crate::handlers::repositories::get_repository)
                .post(crate::handlers::repositories::create_repository),
        )
        .route("/capsules", get(crate::handlers::capsules::get_capsule))
        // The repositories of the catalog: the ones that hold content, and
        // the way they behave. They live under their own path because
        // `/repositories` is the registry of repositories, which is a
        // different thing: a name and a description, without content.
        .route(
            "/catalog/repositories",
            get(crate::handlers::catalog_repositories::list_repositories)
                .post(crate::handlers::catalog_repositories::create_repository),
        )
        // What can be created. The console offers the modalities that the
        // API serves, and it learns them here instead of keeping a table
        // of its own.
        .route(
            "/capabilities",
            get(crate::handlers::capabilities::get_capabilities),
        )
        // What the account has and what it may have. The quota says no
        // on the upload surfaces, and it says why here: a gate that
        // refuses without saying what it holds is a mystery, not a
        // contract. Only the caller's own ledger is readable.
        .route("/account/usage", get(crate::handlers::quota::get_usage))
        // The search of the catalog and the detail of a package. They
        // answer with the account of the verified principal and with the
        // repositories that principal can read, and not one byte more.
        .route("/search", get(crate::handlers::search::get_search))
        .route(
            "/packages/{repository}/{*path}",
            get(crate::handlers::packages::get_package),
        )
        // Promotion to the trusted channel and its revocation. They are
        // tenant operations, not platform administration: each one
        // authenticates, and Cedar decides on the repository.
        // The provenance of what a proxy brought from its upstream.
        .route("/provenance", get(crate::handlers::raw::provenance))
        // Trusted publishers (HSP-152): the OIDC exchange authenticates with
        // the ID token itself, the management with Cedar on the repository.
        .route(
            "/publishers/exchange",
            post(crate::handlers::publishers::exchange),
        )
        .route(
            "/trusted-publishers/{repository}",
            get(crate::handlers::publishers::list),
        )
        .route(
            "/trusted-publishers/{repository}/{id}",
            axum::routing::put(crate::handlers::publishers::put_publisher)
                .delete(crate::handlers::publishers::delete_publisher),
        )
        // Dependency freeze (HSP-161): an immutable view of a hosted repository,
        // read only from its manifest and pinned against the GC. Creating and
        // releasing one is the dedicated action `ManageDependencyFreeze`.
        .route("/freezes", post(crate::handlers::freezes::create))
        .route("/freezes/unpin", post(crate::handlers::freezes::unpin))
        .route("/freezes/{id}", get(crate::handlers::freezes::manifest))
        .route(
            "/freezes/{id}/content/{*path}",
            get(crate::handlers::freezes::content),
        )
        .route("/promotions", post(crate::handlers::promotions::promote))
        .route(
            "/promotions/revoke",
            post(crate::handlers::promotions::revoke),
        )
        .with_state(app_state.clone())
        .merge(investigation_route(
            app_state.clone(),
            investigation_enabled,
        ))
}

/// The investigation of a subject: a read that explains what the service
/// knows about it.
///
/// It is mounted only when the deployment says so: the Cedar bundle has to
/// declare the action it asks for, and a surface that is mounted without
/// that declaration answers `500` to every request, which is a worse
/// answer than a `404`.
fn investigation_route(app_state: AppState, enabled: bool) -> axum::Router {
    if enabled {
        axum::Router::new()
            .route(
                "/investigations",
                post(crate::handlers::investigation::investigate),
            )
            .route(
                "/investigations/freshness",
                post(crate::handlers::investigation::freshness),
            )
            .with_state(app_state)
    } else {
        axum::Router::new()
    }
}

/// The Policy Studio routes (HSP-151). Each route is its own resource for
/// the gate; every request body is bounded (413 above the limit), because a
/// policy document is small and a revision is stored forever.
fn policy_studio_routes() -> Router<AppState> {
    Router::new()
        // Policy Studio: the lifecycle of a trust policy, server-side
        // (HSP-151). The gate asks Cedar per method and path, so each route
        // is its own resource: a revision can be read without being savable
        // and saved without being activatable. validate/simulate never
        // write (their answer carries `applied: false`); only
        // activate/rollback move the head, through the lifecycle's CAS.
        .route(
            "/policy-studio/revisions",
            get(crate::handlers::policy_studio::load_revision)
                .post(crate::handlers::policy_studio::save),
        )
        .route(
            "/policy-studio/head",
            get(crate::handlers::policy_studio::head),
        )
        .route(
            "/policy-studio/validate",
            post(crate::handlers::policy_studio::validate),
        )
        .route(
            "/policy-studio/simulate",
            post(crate::handlers::policy_studio::simulate),
        )
        .route(
            "/policy-studio/activate",
            post(crate::handlers::policy_studio::activate),
        )
        .route(
            "/policy-studio/rollback",
            post(crate::handlers::policy_studio::rollback),
        )
        .route(
            "/policy-studio/history",
            get(crate::handlers::policy_studio::history),
        )
        .layer(axum::extract::DefaultBodyLimit::max(
            crate::handlers::policy_studio::STUDIO_BODY_LIMIT,
        ))
}
