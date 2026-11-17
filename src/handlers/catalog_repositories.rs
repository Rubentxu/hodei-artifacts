//! The repositories of the catalog.
//!
//! The catalog is the authority on what a repository is: it holds the
//! format of its content, the way it behaves and the channel it is served
//! on. This module is the console's view of that: the list of the
//! repositories of the account, and the creation of a new one.
//!
//! # The account comes first
//!
//! The account comes from the **verified principal**, never from the
//! request. The repositories of another account are not evaluated and are
//! not returned: they do not exist for this account.
//!
//! # The permission is Cedar's, one repository at a time
//!
//! A denial is a decision: the repository does not come out of the list.
//! A decision that **could not be taken** is not a decision, and the list
//! does not fill that silence with an empty answer: an empty list says
//! that there is nothing, and what happened is that nobody could ask.
//!
//! The action is the one of the repository, and not the one of its
//! content: this list says which repositories exist and how they behave,
//! not what is inside them. The content is what the search lists, and it
//! asks for the permission of the content.

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_authz::{AuthorizationOutcome, VerifiedContext};
use hodei_catalog::{
    CatalogError, DigestScope, GroupConfig, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, ServeChannel, UpstreamConfig, repository_hrn,
};
use serde::Deserialize;
use serde_json::Value;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{Authentication, correlation_id, error_body, record_audit, the_context};

/// The longest name a repository can have.
const MAX_NAME: usize = 128;

/// What can fail in the repositories of the catalog.
#[derive(Debug)]
pub enum CatalogRepositoryError {
    /// The request could not be authenticated.
    Unauthorized,
    /// The principal may not do this.
    Forbidden,
    /// There is no such repository.
    NotFound,
    /// The request does not say what it says.
    BadRequest(String),
    /// There is already a repository with that name.
    Conflict,
    /// Something failed inside.
    Internal,
    /// The catalog could not answer.
    Unavailable,
}

impl IntoResponse for CatalogRepositoryError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                error_body("unauthenticated", "unauthorized"),
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, error_body("permission", "forbidden")),
            Self::NotFound => (StatusCode::NOT_FOUND, error_body("absence", "not found")),
            Self::BadRequest(reason) => (StatusCode::BAD_REQUEST, error_body("invalid", &reason)),
            Self::Conflict => (StatusCode::CONFLICT, error_body("conflict", "conflict")),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_body("indeterminate", "internal error"),
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                error_body("unavailable", "the repositories are not available"),
            ),
        };
        let mut response = (status, body).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                header::HeaderValue::from_static("Bearer"),
            );
        }
        response
    }
}

/// What the console sees of a repository.
#[derive(Debug, serde::Serialize)]
pub struct RepositorySummary {
    /// The name of the repository inside the account.
    pub name: String,
    /// The format of the content it holds.
    pub format: RepositoryFormat,
    /// How it behaves.
    pub mode: RepositoryMode,
    /// The channel it is served on.
    pub channel: ServeChannel,
}

/// The repositories of the account.
#[derive(Debug, serde::Serialize)]
pub struct RepositoryList {
    /// The repositories the principal may read.
    pub repositories: Vec<RepositorySummary>,
}

/// The body of a creation.
#[derive(Debug, Deserialize)]
pub struct CreateRepositoryBody {
    /// The name of the repository inside the account.
    pub name: String,
    /// The format of the content it will hold.
    pub format: RepositoryFormat,
    /// How it will behave.
    pub mode: RepositoryMode,
    /// The description of the repository.
    #[serde(default)]
    pub description: Option<String>,
    /// The region of the repository.
    #[serde(default)]
    pub region: Option<String>,
    /// The settings of the modality: the upstream of a proxy, the members
    /// of a group, the channel of a repository.
    #[serde(default)]
    pub settings: Option<Value>,
}

/// `GET /api/v1/catalog/repositories`
///
/// The repositories of the account that the principal may read.
pub async fn list_repositories(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RepositoryList>, CatalogRepositoryError> {
    let context = the_context(&state, &headers)
        .await
        .map_err(|failure| match failure {
            Authentication::Unauthorized => CatalogRepositoryError::Unauthorized,
            Authentication::Unavailable => CatalogRepositoryError::Unavailable,
        })?;

    // Without a catalog there are no repositories to list, and it says
    // that it cannot answer: a 404 would say there are none, and that is
    // not what happened.
    let searcher = state
        .catalog_search
        .as_ref()
        .ok_or(CatalogRepositoryError::Unavailable)?;

    let account = context.account().as_str();
    let repositories = searcher
        .repositories(account)
        .await
        .map_err(the_catalog_error)?;
    let permitted = the_permitted(&state, &context, repositories)?;

    Ok(Json(RepositoryList {
        repositories: permitted.iter().map(summary_of).collect(),
    }))
}

/// `POST /api/v1/catalog/repositories`
///
/// The body says the format and the modality, and the settings that the
/// modality needs. A format that does not serve the modality is a bad
/// request: the release would not serve what was declared.
pub async fn create_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request<Body>,
) -> Result<(StatusCode, Json<RepositorySummary>), CatalogRepositoryError> {
    let context = the_context(&state, &headers)
        .await
        .map_err(|failure| match failure {
            Authentication::Unauthorized => CatalogRepositoryError::Unauthorized,
            Authentication::Unavailable => CatalogRepositoryError::Unavailable,
        })?;
    let correlation = correlation_id(&headers, &state.identity);

    // The body is parsed only after the authentication: a missing or
    // invalid credential is always a 401, even when the body is not
    // readable.
    let body = to_bytes(request.into_body(), 1024 * 1024)
        .await
        .map_err(|_| CatalogRepositoryError::BadRequest("the body is not readable".to_string()))?;
    let body: CreateRepositoryBody = serde_json::from_slice(&body).map_err(|error| {
        CatalogRepositoryError::BadRequest(format!("the body is not a repository: {error}"))
    })?;

    let account = context.account().as_str();
    let name = body.name.trim();
    if !valid_name(name) {
        return Err(CatalogRepositoryError::BadRequest(
            "the name of the repository is not valid".to_string(),
        ));
    }
    if !body.format.supports(&body.mode) {
        return Err(CatalogRepositoryError::BadRequest(format!(
            "the format {} does not serve the modality {}",
            body.format.as_str(),
            body.mode.as_str()
        )));
    }
    let config = the_config(&body)?;

    let region = body.region.as_deref().unwrap_or("global");
    let hrn = repository_hrn("hodei", "artifacts", region, account, name);
    let action = crate::actions::create_repository();
    let outcome =
        match state
            .authorize_resource
            .execute_canonical(context.clone(), action.clone(), &hrn)
        {
            Ok(outcome) => outcome,
            Err(_) => {
                record_audit(
                    &state,
                    context.principal().subject().as_str(),
                    account,
                    &action.to_string(),
                    &hrn.to_string(),
                    AuditResult::Failed,
                    &correlation,
                );
                return Err(CatalogRepositoryError::Internal);
            }
        };
    if !outcome.allows_access() {
        let result = match outcome {
            AuthorizationOutcome::Deny(_) => AuditResult::Denied,
            AuthorizationOutcome::Indeterminate(_) => AuditResult::Failed,
            AuthorizationOutcome::Permit => AuditResult::Allowed,
        };
        record_audit(
            &state,
            context.principal().subject().as_str(),
            account,
            &action.to_string(),
            &hrn.to_string(),
            result,
            &correlation,
        );
        return Err(CatalogRepositoryError::Forbidden);
    }

    let writer = state
        .catalog_writer
        .as_ref()
        .ok_or(CatalogRepositoryError::Unavailable)?;
    let now = state.identity.now();
    let descriptor = RepositoryDescriptor {
        hrn: hrn.clone(),
        account: account.to_string(),
        repo_key: name.to_string(),
        format: body.format,
        mode: body.mode,
        config,
        created_at: now,
        updated_at: now,
    };
    let created = writer
        .create_repository(account, descriptor)
        .await
        .map_err(|error| {
            let error = the_catalog_error(error);
            record_audit(
                &state,
                context.principal().subject().as_str(),
                account,
                &action.to_string(),
                &hrn.to_string(),
                AuditResult::Failed,
                &correlation,
            );
            error
        })?;

    record_audit(
        &state,
        context.principal().subject().as_str(),
        account,
        &action.to_string(),
        &hrn.to_string(),
        AuditResult::Allowed,
        &correlation,
    );
    Ok((StatusCode::CREATED, Json(summary_of(&created))))
}

/// The repositories the principal may read.
///
/// Cedar decides one by one. A denial is a decision and the repository
/// does not come out; a decision that could not be taken is not a
/// decision, and the list does not answer with nothing.
fn the_permitted(
    state: &AppState,
    context: &VerifiedContext,
    repositories: Vec<RepositoryDescriptor>,
) -> Result<Vec<RepositoryDescriptor>, CatalogRepositoryError> {
    let action = crate::actions::read_repository();
    let mut permitted = Vec::with_capacity(repositories.len());
    for repository in repositories {
        let outcome = state
            .authorize_resource
            .execute_canonical(context.clone(), action.clone(), &repository.hrn)
            .map_err(|_| CatalogRepositoryError::Internal)?;
        match outcome {
            AuthorizationOutcome::Permit => permitted.push(repository),
            AuthorizationOutcome::Deny(_) => {}
            AuthorizationOutcome::Indeterminate(_) => return Err(CatalogRepositoryError::Internal),
        }
    }
    Ok(permitted)
}

/// What the console sees of a repository.
fn summary_of(repository: &RepositoryDescriptor) -> RepositorySummary {
    RepositorySummary {
        name: repository.repo_key.clone(),
        format: repository.format,
        mode: repository.mode,
        channel: ServeChannel::of(&repository.config),
    }
}

/// The configuration of the repository that is being declared.
///
/// The settings are the ones of the modality, and they are validated with
/// the same rules that serve them: a proxy without an upstream, or a
/// group without members, is a repository that cannot work, and it is
/// refused here rather than after it exists.
fn the_config(body: &CreateRepositoryBody) -> Result<RepositoryConfig, CatalogRepositoryError> {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    if let Some(settings) = &body.settings {
        if !settings.is_object() {
            return Err(CatalogRepositoryError::BadRequest(
                "the settings are not an object".to_string(),
            ));
        }
        config.config_settings = settings.clone();
    }
    match body.mode {
        RepositoryMode::Hosted => Ok(config),
        RepositoryMode::Proxy => match UpstreamConfig::of(&config) {
            Ok(Some(_)) => Ok(config),
            Ok(None) => Err(CatalogRepositoryError::BadRequest(
                "the proxy settings do not declare an upstream".to_string(),
            )),
            Err(error) => Err(CatalogRepositoryError::BadRequest(error.to_string())),
        },
        RepositoryMode::Group => match GroupConfig::of(&config) {
            Ok(Some(_)) => Ok(config),
            Ok(None) => Err(CatalogRepositoryError::BadRequest(
                "the group settings do not declare its members".to_string(),
            )),
            Err(error) => Err(CatalogRepositoryError::BadRequest(error.to_string())),
        },
    }
}

/// Whether the name of a repository is one that can exist.
///
/// The name is the key of the repository inside the account, and it goes
/// on the wire and in the path of its content: it cannot have spaces,
/// slashes or control characters.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

/// The error of the catalog, translated into what the client sees.
fn the_catalog_error(error: CatalogError) -> CatalogRepositoryError {
    match error {
        CatalogError::InvalidRequest { detail } => CatalogRepositoryError::BadRequest(detail),
        CatalogError::Conflict { .. } => CatalogRepositoryError::Conflict,
        CatalogError::Unavailable { .. } => CatalogRepositoryError::Unavailable,
        _ => CatalogRepositoryError::Internal,
    }
}
