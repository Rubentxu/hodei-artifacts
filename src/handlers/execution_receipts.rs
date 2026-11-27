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
//!    the trust capsule reads.
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
) -> Result<ContentDigest, ReceiptFailure> {
    let length = store.length(digest).await.map_err(|error| match error {
        BlobError::NotFound => ReceiptFailure::Plain(
            StatusCode::CONFLICT,
            "E_ARTIFACT_BYTES_MISSING",
            "the artifact is named by the catalog but its bytes are not in the store",
        ),
        _ => ResolutionError::Unavailable.into(),
    })?;
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

    let account = context.account().as_str().to_string();
    let origin =
        the_repository_in(&state, &context, &body.repository, &[RepositoryFormat::Raw]).await?;
    if origin.mode != RepositoryMode::Hosted {
        return Err(ResolutionError::NotImplemented(
            "only hosted repositories hold artifacts that a receipt can link in this release",
        )
        .into());
    }
    let action = crate::actions::ingest_execution_receipt();
    match authorize_repository(&state.authorize_resource, &context, action.clone(), &origin) {
        Ok(_) => {}
        Err(error) => {
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
    }

    let resource = format!("{}/{}", origin.repo_key, body.path);
    // Fail closed on the contract before touching a byte of the artifact.
    if let Err(error) = body.receipt.validate() {
        audit(
            &state,
            &context,
            &action.to_string(),
            &resource,
            AuditResult::Denied,
            &correlation,
        );
        let (status, code, message) = match error {
            crate::execution_receipt::ReceiptError::NotSuccessful => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_RECEIPT_NOT_SUCCESS",
                "only a run that succeeded can be linked to an artifact",
            ),
            _ => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_RECEIPT_INVALID",
                "the receipt does not satisfy the execution receipt contract",
            ),
        };
        return Err(ReceiptFailure::Plain(status, code, message));
    }
    let declared = body.receipt.digest().ok_or(ResolutionError::Internal)?;

    let asset = the_current_asset(&state, &context, &body.repository, &body.path)
        .await?
        .ok_or(ResolutionError::NotFound)?;
    let store = the_content_store(&state)?;
    let actual = rehash(store.as_ref(), &asset.digest).await?;
    if actual != declared || actual != asset.digest {
        audit(
            &state,
            &context,
            &action.to_string(),
            &resource,
            AuditResult::Denied,
            &correlation,
        );
        return Err(ReceiptFailure::Plain(
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_RECEIPT_DIGEST_MISMATCH",
            "the digest of the receipt is not the digest of the bytes of the artifact; nothing was recorded",
        ));
    }

    let subject = artifact_hrn(origin.hrn.region(), &account, &origin.repo_key, &actual);
    // The retention first, then the content: the receipt bytes are kept
    // against the collector from the moment they exist.
    let receipt_digest = ContentDigest::of_bytes(&body.receipt.canonical_bytes());
    let hold = hodei_catalog::ContentHold {
        id: format!("receipt-{}", receipt_digest.value),
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
        .await
        .map_err(|error| match error {
            IngestError::Refused => ReceiptFailure::Plain(
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_RECEIPT_REFUSED",
                "the evidence pipeline refused the receipt",
            ),
            IngestError::Unavailable => ReceiptFailure::Surface(ResolutionError::Unavailable),
        });
    let linked = match linked {
        Ok(linked) => linked,
        Err(failure) => {
            audit(
                &state,
                &context,
                &action.to_string(),
                &subject.to_string(),
                AuditResult::Failed,
                &correlation,
            );
            return Err(failure);
        }
    };
    audit(
        &state,
        &context,
        &action.to_string(),
        &subject.to_string(),
        AuditResult::Allowed,
        &correlation,
    );
    let (status, result) = match &linked {
        Linked::Created(_) => (StatusCode::CREATED, "created"),
        Linked::Existing(_) => (StatusCode::OK, "already_accepted"),
        Linked::Revised(_) => (StatusCode::CREATED, "revised"),
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
