//! Execution receipts: `POST /api/v1/execution-receipts` (HSP-323 slice 8,
//! origin HSP-37).
//!
//! The caller names an artifact (repository and path) and hands over the
//! execution receipt that says which run produced it. The service:
//!
//! 1. authenticates the caller and lets Cedar decide with the dedicated action
//!    `IngestExecutionReceipt` on the repository (no other action implies it);
//! 2. validates the receipt against the contract (pinned producer, closed
//!    charsets, a run that succeeded);
//! 3. **re-hashes the bytes it holds** for the artifact and refuses the receipt
//!    when the digest it declares is not the digest of those bytes: nothing is
//!    written, so no trust lineage is published;
//! 4. admits the receipt through the evidence pipeline as evidence about the
//!    artifact's exact identity (`artifact/<repository>/sha256-<hex>`), which
//!    the trust capsule reads. The evidence is **Unverified** (the producer is
//!    not authenticated): it never makes `builder_trust` Verified. See
//!    `docs/security/execution-receipts.md`.
//!
//! Every outcome after the authorization is audited: `Allowed`, `Denied` for a
//! refusal the caller caused (422, 409, 413, 429) and `Failed` for the rest
//! (404, 5xx).
//!
//! The repository is always looked up in the account of the verified
//! principal: a repository of another account is the same 404 as one that
//! does not exist. Nothing in the answer, the audit or the logs carries a
//! value of the receipt that is not an identifier.

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
};
use hodei_catalog::{RepositoryFormat, RepositoryMode};
use hodei_evidence::ContentDigest;
use hodei_hub::blob::BlobError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::execution_receipt::{ExecutionReceiptV1, IngestError, Linked};
use crate::handlers::correlation_id;
use crate::handlers::freezes::audit;
use crate::resolution::{
    ResolutionError, authorize_repository, the_content_store, the_context, the_current_asset,
    the_repository_in,
};
use crate::trust::hrn::artifact_hrn;

/// The largest body of a request: a receipt is a few hundred bytes.
const MAX_BODY: usize = 16 * 1024;

/// The chunk the artifact is re-hashed in (the artifact is never held whole).
const HASH_CHUNK: u64 = 1024 * 1024;

/// How a request that cannot be served ends.
#[derive(Debug)]
pub enum ReceiptFailure {
    /// An error of the shared surface.
    Surface(ResolutionError),
    /// A status, a stable code and a message that never echoes input.
    Plain(StatusCode, &'static str, &'static str),
}

impl From<ResolutionError> for ReceiptFailure {
    fn from(error: ResolutionError) -> Self {
        Self::Surface(error)
    }
}

impl IntoResponse for ReceiptFailure {
    fn into_response(self) -> Response {
        match self {
            Self::Surface(error) => error.into_response(),
            Self::Plain(status, code, message) => (
                status,
                Json(serde_json::json!({ "error": code, "message": message })),
            )
                .into_response(),
        }
    }
}

/// The body of `POST /api/v1/execution-receipts`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestBody {
    /// The hosted raw repository that holds the artifact.
    pub repository: String,
    /// The path of the artifact within the repository.
    pub path: String,
    /// The receipt.
    pub receipt: ExecutionReceiptV1,
}

/// The answer of an admission.
#[derive(Debug, Serialize)]
pub struct IngestResponse {
    /// `created`, `already_accepted` or `revised`.
    pub result: &'static str,
    /// The id of the evidence record.
    pub evidence_id: String,
    /// The exact identity of the artifact the receipt is linked to.
    pub subject: String,
    /// The digest of the artifact, verified against its bytes.
    pub digest: String,
    /// The run id of the receipt.
    pub run_id: String,
    /// The source revision the receipt declares.
    pub source_revision: String,
}

/// The sha256 of the stored bytes of `digest`, read in chunks.
///
/// The hash is computed here from the bytes the store returns, and not taken
/// from what the catalog or the store say about them.
async fn rehash(
    store: &dyn hodei_hub::BlobStore,
    digest: &ContentDigest,
    max_bytes: u64,
) -> Result<ContentDigest, ReceiptFailure> {
    let length = store.length(digest).await.map_err(|error| match error {
        BlobError::NotFound => ReceiptFailure::Plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "E_ARTIFACT_BYTES_MISSING",
            "the artifact is named by the catalog but its bytes are not in the store",
        ),
        _ => ResolutionError::Unavailable.into(),
    })?;
    // The size is asked before a single byte is read.
    if length > max_bytes {
        return Err(ReceiptFailure::Plain(
            StatusCode::PAYLOAD_TOO_LARGE,
            "E_ARTIFACT_TOO_LARGE",
            "the artifact is larger than the service re-hashes for a receipt",
        ));
    }
    let mut hasher = Sha256::new();
    let mut offset = 0u64;
    while offset < length {
        let take = HASH_CHUNK.min(length - offset);
        let chunk = store
            .read_range(digest, offset, take)
            .await
            .map_err(|_| ResolutionError::Unavailable)?;
        if chunk.len() as u64 != take {
            return Err(ResolutionError::Unavailable.into());
        }
        hasher.update(&chunk);
        offset += take;
    }
    Ok(ContentDigest::sha256(
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    ))
}

/// The status a failure ends in.
fn status_of(failure: &ReceiptFailure) -> StatusCode {
    match failure {
        ReceiptFailure::Surface(error) => error.the_verdict().0,
        ReceiptFailure::Plain(status, _, _) => *status,
    }
}

/// `POST /api/v1/execution-receipts`
pub async fn ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Result<Response, ReceiptFailure> {
    let context = the_context(&state, &headers).await?;
    let correlation = correlation_id(&headers, &state.identity);
    let bytes = axum::body::to_bytes(request.into_body(), MAX_BODY)
        .await
        .map_err(|_| ResolutionError::BadRequest("the body is too large"))?;
    // A body that does not parse is refused without echoing any of it: it may
    // hold a secret in a field the contract does not have.
    let body: IngestBody = serde_json::from_slice(&bytes)
        .map_err(|_| ResolutionError::BadRequest("the body is not a valid receipt request"))?;

    let ingestor = state.receipt_ingestor.clone().ok_or(
        ResolutionError::NotImplemented(
            "execution receipts need the evidence context wired: a receipt that cannot be kept is not accepted",
        ),
    )?;
    let gc = state.gc.clone().ok_or(ResolutionError::NotImplemented(
        "execution receipts need the garbage collection service to retain the evidence content",
    ))?;

    let origin =
        the_repository_in(&state, &context, &body.repository, &[RepositoryFormat::Raw]).await?;
    if origin.mode != RepositoryMode::Hosted {
        return Err(ResolutionError::NotImplemented(
            "only hosted repositories hold artifacts that a receipt can link in this release",
        )
        .into());
    }
    let action = crate::actions::ingest_execution_receipt();
    if let Err(error) =
        authorize_repository(&state.authorize_resource, &context, action.clone(), &origin)
    {
        audit(
            &state,
            &context,
            &action.to_string(),
            &origin.repo_key,
            if matches!(error, ResolutionError::Forbidden) {
                AuditResult::Denied
            } else {
                AuditResult::Failed
            },
            &correlation,
        );
        return Err(error.into());
    }

    // From here on the caller is authorized, and every way out is audited.
    let resource = format!("{}/{}", origin.repo_key, body.path);
    let outcome = admit(
        &state,
        &context,
        &origin,
        &body,
        &ingestor,
        &gc,
        &correlation,
    )
    .await;
    match outcome {
        Ok(response) => Ok(response),
        Err(failure) => {
            let status = status_of(&failure);
            let result = if matches!(status.as_u16(), 409 | 413 | 422 | 429) {
                AuditResult::Denied
            } else {
                AuditResult::Failed
            };
            audit(
                &state,
                &context,
                &action.to_string(),
                &resource,
                result,
                &correlation,
            );
            Err(failure)
        }
    }
}

/// The admission proper, after the authorization.
async fn admit(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    origin: &hodei_catalog::RepositoryDescriptor,
    body: &IngestBody,
    ingestor: &crate::execution_receipt::ReceiptIngestor,
    gc: &crate::gc::GcService,
    correlation: &str,
) -> Result<Response, ReceiptFailure> {
    let action = crate::actions::ingest_execution_receipt();
    let account = context.account().as_str().to_string();

    // Fail closed on the contract before touching a byte of the artifact.
    if let Err(error) = body.receipt.validate() {
        let (code, message) = match error {
            crate::execution_receipt::ReceiptError::NotSuccessful => (
                "E_RECEIPT_NOT_SUCCESS",
                "only a run that succeeded can be linked to an artifact",
            ),
            _ => (
                "E_RECEIPT_INVALID",
                "the receipt does not satisfy the execution receipt contract",
            ),
        };
        return Err(ReceiptFailure::Plain(
            StatusCode::UNPROCESSABLE_ENTITY,
            code,
            message,
        ));
    }
    let declared = body.receipt.digest().ok_or(ResolutionError::Internal)?;

    // Bound what one caller can make the service spend.
    let limits = state.receipt_limits.clone();
    let _permit = limits
        .concurrency
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ReceiptFailure::Plain(
                StatusCode::TOO_MANY_REQUESTS,
                "E_RECEIPT_BUSY",
                "too many receipts are being admitted: retry shortly",
            )
        })?;

    let asset = the_current_asset(state, context, &body.repository, &body.path)
        .await?
        .ok_or(ResolutionError::NotFound)?;
    let store = the_content_store(state)?;
    let actual = rehash(store.as_ref(), &asset.digest, limits.max_artifact_bytes).await?;
    if actual != declared || actual != asset.digest {
        return Err(ReceiptFailure::Plain(
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_RECEIPT_DIGEST_MISMATCH",
            "the digest of the receipt is not the digest of the bytes of the artifact; nothing was recorded",
        ));
    }

    let subject = artifact_hrn(origin.hrn.region(), &account, &origin.repo_key, &actual);
    // The retention first, then the content: the receipt bytes are kept
    // against the collector from the moment they exist. A retention this call
    // placed is released again when the admission fails; one that was already
    // there (an identical receipt admitted before) is never touched.
    let receipt_digest = ContentDigest::of_bytes(&body.receipt.canonical_bytes());
    let hold_id = format!("receipt-{}", receipt_digest.value);
    // A read that fails is treated as "it was already there": the worst case
    // is a retention left in place, which is the safe side for evidence.
    let existed = gc
        .hold(&hold_id)
        .await
        .map_or(true, |found| found.is_some());
    let hold = hodei_catalog::ContentHold {
        id: hold_id.clone(),
        scope: hodei_catalog::HoldScope::Digest {
            digest: receipt_digest,
        },
        reason: "evidence content of an execution receipt".to_string(),
        actor: context.principal().subject().as_str().to_string(),
        created_at: chrono::Utc::now(),
        expires_at: None,
    };
    gc.place_holds(std::slice::from_ref(&hold))
        .await
        .map_err(|_| ResolutionError::Unavailable)?;

    let linked = ingestor
        .ingest(&account, subject.clone(), &body.receipt, store.clone())
        .await;
    let linked = match linked {
        Ok(linked) => linked,
        Err(error) => {
            if !existed && let Err(release) = gc.release_hold(&hold_id).await {
                tracing::error!(%release, "the retention of a refused receipt was not released");
            }
            return Err(match error {
                IngestError::Refused => ReceiptFailure::Plain(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "E_RECEIPT_REFUSED",
                    "the evidence pipeline refused the receipt",
                ),
                IngestError::Conflict => ReceiptFailure::Plain(
                    StatusCode::CONFLICT,
                    "E_RECEIPT_CONFLICT",
                    "a different receipt already exists for this run and artifact; it is not replaced",
                ),
                IngestError::Unavailable => ReceiptFailure::Surface(ResolutionError::Unavailable),
            });
        }
    };
    audit(
        state,
        context,
        &action.to_string(),
        &subject.to_string(),
        AuditResult::Allowed,
        correlation,
    );
    let (status, result) = match &linked {
        Linked::Created(_) => (StatusCode::CREATED, "created"),
        Linked::Existing(_) => (StatusCode::OK, "already_accepted"),
    };
    let response = IngestResponse {
        result,
        evidence_id: linked.evidence_id().to_string(),
        subject: subject.to_string(),
        digest: actual.to_string(),
        run_id: body.receipt.run_id.clone(),
        source_revision: body.receipt.source_revision.clone(),
    };
    Ok((status, Json(response)).into_response())
}
