//! The investigation of a subject.
//!
//! The endpoint answers the question the investigation exists for: what
//! does the service know about this artefact, what supports each claim,
//! what is missing, and what was left out. The answer is the report of the
//! investigator: versioned, with the citations of every claim and with the
//! coverage of what was read.
//!
//! It is a **read**. There is no route here that promotes an artefact,
//! that changes a policy, that approves an exception or that writes a
//! decision: the surface has one method, and that method only reads.
//!
//! # The order of the gates
//!
//! 1. The **principal** is verified. A request without a credential does
//!    not get to look at what it asks for.
//! 2. The **account** comes from the verified principal. A subject of
//!    another account is hidden behind the same `404` as a subject that
//!    does not exist: the answer to a thing that is not there is the same
//!    whether it is not there or it belongs to somebody else.
//! 3. **Cedar** decides on the subject, one subject at a time. A decision
//!    that could not be taken is not a permission.
//! 4. Only then does anything get **read**.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use hodei_investigator::investigator::InvestigationError;
use hodei_investigator::report::InvestigationReport;
use hodei_investigator::scope::{RetrievalBudget, RetrievalScope};
use kernel::Hrn;

use crate::app_state::AppState;
use crate::handlers::{Authentication, error_body, the_context};

/// What can fail in an investigation.
#[derive(Debug)]
pub enum InvestigationApiError {
    /// The request could not be authenticated.
    Unauthorized,
    /// The request was not coherent.
    BadRequest(String),
    /// The request was authenticated and not allowed.
    Forbidden,
    /// There is no such subject, or it is not the caller's.
    NotFound,
    /// The stores could not answer.
    Unavailable,
    /// What was read did not fit the scope it was read under.
    OutOfScope(String),
    /// The world changed while it was being read.
    Stale(String),
}

impl IntoResponse for InvestigationApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                error_body("unauthenticated", "unauthorized"),
            ),
            Self::BadRequest(reason) => (StatusCode::BAD_REQUEST, error_body("invalid", &reason)),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                error_body(
                    "permission",
                    "the principal may not investigate this subject",
                ),
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                error_body("absence", "there is no such subject"),
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                error_body("unavailable", "the investigation is not available"),
            ),
            Self::OutOfScope(reason) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_body("corruption", &reason),
            ),
            Self::Stale(reason) => (StatusCode::CONFLICT, error_body("conflict", &reason)),
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

/// What the client asks for.
#[derive(Debug, serde::Deserialize)]
pub struct InvestigationRequest {
    /// The subject of the investigation: a canonical HRN of an artefact.
    pub subject: String,
    /// The question.
    ///
    /// It is optional: without a model nobody answers it, and the report
    /// declares it unanswered. It is kept in the trace, so that whoever
    /// reads the report later knows what was asked.
    #[serde(default)]
    pub question: Option<String>,
    /// How many evidence pieces may be read.
    ///
    /// It can only **narrow** the limit of the deployment: a client that
    /// asks for less gets less, and a client that asks for more gets the
    /// limit of the deployment.
    #[serde(default)]
    pub max_evidence: Option<usize>,
    /// How many relations may be read. It can only narrow.
    #[serde(default)]
    pub max_relations: Option<usize>,
    /// How many bytes of evidence content may be read. It can only narrow.
    #[serde(default)]
    pub max_bytes: Option<u64>,
}

/// `POST /api/v1/investigations`
///
/// The body names the subject and, optionally, the question. The answer is
/// the report of the investigator: the findings with their class and their
/// verification status, the coverage of what was read, and the manifest of
/// the reads with their digests.
///
/// The method is a `POST` because the question travels in the body, and the
/// body of a query is not the body of a command: **nothing is written**,
/// and the Cedar action of this surface is a read.
pub async fn investigate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<InvestigationRequest>,
) -> Result<Json<InvestigationReport>, InvestigationApiError> {
    // 1. The principal is verified.
    let context = the_context(&state, &headers)
        .await
        .map_err(|failure| match failure {
            Authentication::Unauthorized => InvestigationApiError::Unauthorized,
            Authentication::Unavailable => InvestigationApiError::Unavailable,
        })?;

    // 2. The subject: a canonical HRN of an artefact of the service.
    let subject = Hrn::from_string(request.subject.trim()).ok_or_else(|| {
        InvestigationApiError::BadRequest("the subject is not a canonical HRN".to_string())
    })?;
    if subject.partition() != "hodei"
        || subject.service() != "artifacts"
        || subject.resource_type() != "artifact"
    {
        // A well-formed HRN of another service or type is not a subject of
        // an investigation, and it is not evaluated.
        return Err(InvestigationApiError::NotFound);
    }

    // The account comes from the verified principal. A foreign HRN is
    // hidden behind the same 404 as a missing one.
    if subject.account_id() != context.account().as_str() {
        return Err(InvestigationApiError::NotFound);
    }

    // 3. Cedar decides on the subject. A decision that could not be taken
    // is not a permission, and neither is a denial: what is not permitted
    // is not investigated.
    let outcome = state
        .authorize_resource
        .execute_canonical(context.clone(), crate::actions::investigate(), &subject)
        .map_err(|_| {
            InvestigationApiError::BadRequest("the subject is not a valid resource".to_string())
        })?;
    if !outcome.allows_access() {
        return Err(InvestigationApiError::Forbidden);
    }

    // 4. The question has a size, and the budget is the deployment's.
    let limits = &state.investigation_limits;
    let question = request.question.clone().unwrap_or_default();
    if question.chars().count() > limits.max_question_length {
        return Err(InvestigationApiError::BadRequest(
            "the question is longer than the limit of the deployment".to_string(),
        ));
    }
    let budget = the_budget(&request, limits);

    // The actor is the identity verified, and the subject is the one that
    // was named: a scope that crosses accounts closes itself, before it
    // reads.
    let actor = the_actor(&context)?;
    let mut scope = RetrievalScope::new(actor, subject);
    scope.budget = budget;
    // The scope allows what the budget allows: what does not fit is not
    // truncated in silence, it is declared omitted.
    scope.max_nodes = budget.max_objects.saturating_add(budget.max_relations);

    let investigation = state
        .investigation
        .as_ref()
        .ok_or(InvestigationApiError::Unavailable)?
        .investigate(
            &state.identity.new_id(),
            &context,
            &scope,
            question.trim(),
            chrono::Utc::now(),
        )
        .await
        .map_err(the_investigation_error)?;

    Ok(Json(investigation.report))
}

/// The identity that asks, as the HRN the scope is built with.
///
/// The actor is the **verified** principal, and its account has to be the
/// account of the question: an identity that claims one account while the
/// credential carries another is not an identity this service can ask
/// with.
fn the_actor(context: &hodei_authz::VerifiedContext) -> Result<Hrn, InvestigationApiError> {
    let actor = Hrn::from_string(context.principal().subject().as_str())
        .ok_or(InvestigationApiError::Forbidden)?;
    if actor.account_id() != context.account().as_str() {
        return Err(InvestigationApiError::Forbidden);
    }
    Ok(actor)
}

/// The budget of the request: the limit of the deployment, narrowed.
fn the_budget(
    request: &InvestigationRequest,
    limits: &crate::config::InvestigationConfig,
) -> RetrievalBudget {
    RetrievalBudget {
        max_objects: request
            .max_evidence
            .map_or(limits.max_evidence, |asked| asked.min(limits.max_evidence)),
        max_relations: request.max_relations.map_or(limits.max_relations, |asked| {
            asked.min(limits.max_relations)
        }),
        max_bytes: request
            .max_bytes
            .map_or(limits.max_bytes, |asked| asked.min(limits.max_bytes)),
    }
}

/// The error of the investigator, translated into what the client sees.
fn the_investigation_error(error: InvestigationError) -> InvestigationApiError {
    match error {
        // Whoever asks cannot read what they ask for: the door was closed.
        InvestigationError::NotAuthorized { .. } => InvestigationApiError::Forbidden,
        // The world changed while it was read: there is no coherent
        // snapshot to present, and the client can ask again.
        InvestigationError::Stale(reason) => InvestigationApiError::Stale(reason),
        // The retrieval left the scope: what was read is not what was
        // asked for, and that is not fixed with an answer.
        InvestigationError::OutOfScope(violation) => {
            InvestigationApiError::OutOfScope(format!("the retrieval left the scope: {violation}"))
        }
        // Nobody answered, or the store could not attend.
        InvestigationError::NoAnswer(_reason) => InvestigationApiError::Unavailable,
        // A claim that does not hold and an answer that could not be read:
        // both are failures of what is there, not of who asks.
        InvestigationError::InvalidClaim(error) => InvestigationApiError::OutOfScope(format!(
            "the investigation produced a claim that does not hold: {error}"
        )),
        InvestigationError::Unreadable(reason) => InvestigationApiError::OutOfScope(format!(
            "the answer of the investigation could not be read: {reason}"
        )),
    }
}
