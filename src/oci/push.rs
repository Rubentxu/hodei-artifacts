//! OCI Distribution 1.1 push: upload sessions and the manifest.
//!
//! The push protocol has two pieces and one rule:
//!
//! 1. **Upload sessions**: a `POST` opens one, a `PATCH` (or
//!    several) fills it, a `GET` says how much it holds, a `PUT` closes it
//!    with the digest and a `DELETE` cancels it.
//! 2. **The manifest**: a `PUT` to `/v2/<name>/manifests/<reference>`
//!    publishes the manifest bytes and, if the reference is a
//!    tag, the tag points to them.
//!
//! And the rule is the one that separates a registry from a store: **what is
//! published is exactly what the client said it was**. The digest
//! the client declares is checked against the bytes that arrived,
//! and if they do not match nothing is published: not the blob, nor the manifest, nor
//! the tag.
//!
//! The content is written **in chunks** to the content store, and
//! the commit is the only thing that publishes: a half-written blob is never
//! visible, neither to the one uploading nor to the one downloading.

use axum::body::Body;
use axum::extract::OriginalUri;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use hodei_catalog::{PackageCoordinates, Publication};
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobError, BlobWriteSession, CommitExpectation};
use hodei_hub::oci::ManifestKind;

use crate::app_state::AppState;
use crate::audit::AuditResult;
use crate::handlers::{correlation_id, record_audit};
use crate::oci::failure::OciFailure;
use crate::oci::target::{Target, is_a_digest, the_digest_of_reference, the_tag_name, the_target};
use crate::oci::{
    the_catalog, the_catalog_error, the_content_store, the_context, the_decision, the_repository,
};

/// The header that announces the distribution API version.
const API_VERSION_HEADER: &str = "Docker-Distribution-API-Version";

/// The header with the upload session identifier.
///
/// The OCI protocol does not require it (it is Docker's), but the clients that
/// know it use it, and it costs nothing to carry it.
const UPLOAD_UUID_HEADER: &str = "Docker-Upload-UUID";

/// The header with the content digest.
const CONTENT_DIGEST_HEADER: &str = "Docker-Content-Digest";

/// The header with the tags the manifest accepts.
const OCI_TAG_HEADER: &str = "OCI-Tag";

/// The header with the manifest's `subject`.
///
/// The protocol says the push response **may** carry the published
/// manifest's `subject`, and it does: it is what tells the
/// client which manifest what it uploaded has been hung from.
const OCI_SUBJECT_HEADER: &str = "OCI-Subject";

/// The request parameters the protocol uses.
#[derive(Debug, Default)]
struct TheQuery {
    /// The digest the client declares.
    digest: Option<String>,
    /// The digest the client wants to mount from another repository.
    mount: Option<String>,
    /// The source repository of the mount.
    from: Option<String>,
    /// The tags the manifest accepts.
    tags: Vec<String>,
}

/// The request parameters.
///
/// The query is read by hand and not with an extractor: the parameters that
/// matter are few, and the one that repeats (`tag`) has to repeat
/// without an extractor complaining.
fn the_query(uri: &OriginalUri) -> TheQuery {
    let mut query = TheQuery::default();
    for (key, value) in url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes()) {
        match key.as_ref() {
            "digest" => query.digest = Some(value.into_owned()),
            "mount" => query.mount = Some(value.into_owned()),
            "from" => query.from = Some(value.into_owned()),
            "tag" => query.tags.push(value.into_owned()),
            _ => {}
        }
    }
    query
}

/// The entry point of the write surface.
///
/// The method and the path say which operation it is, and each operation has its
/// rules. What there is **not** is a "generic" operation: a request
/// that is not one of the protocol's is rejected with the protocol's
/// error, not with a router 404 that confuses absence with
/// error.
pub async fn mutate(
    state: AppState,
    uri: OriginalUri,
    method: Method,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    match method {
        Method::POST => start_upload(state, uri, headers, request).await,
        Method::PATCH => append_chunk(state, uri, headers, request).await,
        Method::PUT => put(state, uri, headers, request).await,
        Method::GET => upload_status(state, uri, headers).await,
        Method::DELETE => cancel_upload(state, uri, headers).await,
        other => Err(OciFailure::unsupported(format!(
            "the distribution API does not have the method {other}"
        ))),
    }
}

/// `POST /v2/<name>/blobs/uploads/`
///
/// Opens an upload session, or mounts a blob that is already in another
/// repository, or publishes the whole blob if the client declares its digest.
async fn start_upload(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(uri.path())?;
    let Target::UploadStart { name } = target else {
        return Err(OciFailure::unsupported(
            "this path does not start an upload",
        ));
    };
    let repository = the_repository(&state, &context, &name).await?;
    // A group stores nothing: writing against it is rejected with the
    // dialect error that the distribution reserves for what cannot be
    // done on a read-only repository.
    crate::oci::the_writable(&repository)?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    let query = the_query(&uri);

    // The mount: the blob is already in another repository of the SAME account.
    // If it cannot be mounted, it falls back to a normal upload: it is what
    // a registry that does not mount does, and the client uploads the bytes.
    if let Some(mount) = query.mount.as_deref()
        && let Some(response) = the_mount(
            &state,
            &context,
            &name,
            mount,
            query.from.as_deref(),
            &headers,
        )
        .await?
    {
        return Ok(response);
    }

    // The monolithic upload: the whole body, with its digest.
    if let Some(digest) = query.digest.as_deref() {
        return the_monolithic(
            &state,
            &context,
            &name,
            digest,
            request.into_body(),
            &headers,
        )
        .await;
    }

    // The session: the client receives the `Location` and keeps writing to
    // it.
    //
    // The quota admits the session **before** it exists: opening an
    // upload when the account already has all it can have is a no without
    // touching the disk. The ticket lives with the session, and the reservation returns
    // to the ledger if the session dies without committing.
    let ticket = state
        .quota
        .begin(context.account().as_str(), None)
        .map_err(the_quota_refusal)?;
    let store = the_content_store(&state)?;
    let writer = store
        .begin()
        .await
        .map_err(|error| the_blob_error(&error))?;
    let session = state
        .uploads
        .open(context.account().as_str(), &name, writer, ticket)
        .await?;
    let id = session.lock().await.the_id().to_string();
    let location = the_location_of_upload(&name, &id);

    let mut response = StatusCode::ACCEPTED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&location).expect("the location is a header value"),
    );
    // An upload that has no bytes yet: the range is `0-0`, which is
    // what the protocol says and what the client expects.
    headers.insert(header::RANGE, HeaderValue::from_static("0-0"));
    headers.insert(
        UPLOAD_UUID_HEADER,
        HeaderValue::from_str(&id).expect("the session id is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// The mount of a blob that is already in another repository.
///
/// It only mounts a blob of the **same account**: a repository of another
/// account does not exist for this one, and a mount from the other side would
/// copy bytes from one account to another. And what cannot be mounted is not
/// said: the request falls back to a normal upload, which is what a
/// registry that does not mount does.
async fn the_mount(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    mount: &str,
    from: Option<&str>,
    headers: &HeaderMap,
) -> Result<Option<Response>, OciFailure> {
    // The digest to be mounted has to be a digest: a mount with a
    // malformed digest is a client error, and not a mount
    // that cannot be done.
    let digest = the_digest_of_reference(mount)?;

    // The source repository: the same one, if the client does not say another.
    let source = from.unwrap_or(name);
    let Ok(source_repository) = the_repository(state, context, source).await else {
        return Ok(None);
    };
    // And the principal has to be able to read from it: a mount is a
    // read from one repository and a write to another.
    if the_decision(
        state,
        context,
        crate::actions::download_asset(),
        &source_repository,
    )
    .is_err()
    {
        return Ok(None);
    }

    let (reader, _) = the_catalog(state)?;
    let asset = reader
        .asset(
            context.account().as_str(),
            source,
            &format!("blobs/{digest}"),
        )
        .await
        .ok();
    let Some(asset) = asset else {
        return Ok(None);
    };
    // And the bytes have to be in the store: a row that points to a
    // blob that is not there is a broken row, and mounting a broken blob is
    // publishing a broken blob.
    let store = the_content_store(state)?;
    if !store.exists(&asset.digest).await.unwrap_or(false) {
        return Ok(None);
    }

    // Mounting is reading from the source: if the source is a trusted channel, the
    // digest has to be granted there. Otherwise, the quarantined blob
    // would leave for an open repository without ever being promoted. What is not
    // admitted falls back to a normal upload, just like what does not exist: neither
    // the 201 nor the 202 say whether the digest is in quarantine.
    if crate::trust::admit(
        state,
        context.account().as_str(),
        &source_repository,
        &asset.digest,
    )
    .await
    .is_err()
    {
        return Ok(None);
    }

    // The mount copies no bytes: the CAS is a shared tree and the content
    // is already there. What is written is the reference of the destination
    // repository, which is what makes the blob belong to that repository.
    publish_blob(
        state,
        context,
        name,
        &asset.digest,
        asset.size_bytes,
        headers,
    )
    .await?;

    let mut response = StatusCode::CREATED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_blob(name, &digest))
            .expect("the location is a header value"),
    );
    headers.insert(
        CONTENT_DIGEST_HEADER,
        HeaderValue::from_str(&format!("sha256:{digest}")).expect("the digest is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(Some(response))
}

/// The monolithic upload: the whole body and its digest.
///
/// The protocol defines it as a `POST` with `?digest=` and the whole
/// content. The server checks that what arrived is what the
/// client declared, and if not, publishes nothing.
async fn the_monolithic(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    declared: &str,
    body: Body,
    headers: &HeaderMap,
) -> Result<Response, OciFailure> {
    let declared = the_digest_of_reference(declared)?;
    let store = the_content_store(state)?;
    let mut ticket = state
        .quota
        .begin(context.account().as_str(), the_declared_size(headers))
        .map_err(the_quota_refusal)?;
    let mut writer = store
        .begin()
        .await
        .map_err(|error| the_blob_error(&error))?;
    let length = write_the_body(
        writer.as_mut(),
        body,
        state.max_body_size as u64,
        &mut ticket,
    )
    .await?;

    let found = writer.the_digest();
    if found.value != declared {
        // The client said it was sending something else: the write is aborted
        // and not a byte remains in the store.
        let _ = writer.abort().await;
        return Err(OciFailure::digest_invalid(
            "the content does not match the declared digest",
        ));
    }
    // What the store gains is what the account pays for: a blob that was already
    // there adds not a byte. It is looked at before the commit, because
    // after the commit it is always there.
    let already_had = store
        .exists(&found)
        .await
        .map_err(|error| the_blob_error(&error))?;
    let digest = writer
        .commit(CommitExpectation {
            digest: found.clone(),
            length,
        })
        .await
        .map_err(|error| the_blob_error(&error))?;
    ticket.settle(if already_had { 0 } else { length });

    publish_blob(state, context, name, &digest, length, headers).await?;

    let mut response = StatusCode::CREATED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_blob(name, &digest.value))
            .expect("the location is a header value"),
    );
    headers.insert(
        CONTENT_DIGEST_HEADER,
        HeaderValue::from_str(&format!("sha256:{}", digest.value))
            .expect("the digest is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// `PATCH /v2/<name>/blobs/uploads/<uuid>`
///
/// Appends a chunk to the session.
async fn append_chunk(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(uri.path())?;
    let Target::UploadSession { name, session: id } = target else {
        return Err(OciFailure::unsupported(
            "this path does not carry an upload session",
        ));
    };
    let repository = the_repository(&state, &context, &name).await?;
    // A group stores nothing: writing against it is rejected with the
    // dialect error that the distribution reserves for what cannot be
    // done on a read-only repository.
    crate::oci::the_writable(&repository)?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    // The session may be **on any replica**: the local cache goes
    // first; if it does not exist here —or exists with the handle released—, the
    // journal authority says its offset and the content store
    // reopens the staging by its handle. A `404` only when the session
    // does not exist for the authority either.
    let store = the_content_store(&state)?;
    let shared = state
        .uploads
        .find_or_resume(context.account().as_str(), &name, &id, store.as_ref())
        .await?
        .ok_or_else(|| OciFailure::blob_upload_unknown("the upload session does not exist"))?;
    let mut session = shared.lock().await;
    session.touch();
    let written = session
        .the_local_offset()
        .expect("a resumed session has its writer");

    // A range that cannot be read is not checked: it is rejected.
    if the_range_is_malformed(&headers) {
        return Err(OciFailure::blob_upload_range_mismatch(
            "the chunk declares a range that cannot be read",
        )
        .with_range(the_range_header(written)));
    }

    // The range the client declares and the length of its body have to
    // match: the protocol says the `Content-Range` is the range
    // of the chunk, and a chunk that says one thing and measures another is a client
    // that does not know where it is.
    if let Some((start, end)) = the_range_does_not_match_the_length(&headers) {
        return Err(OciFailure::blob_upload_range_mismatch(format!(
            "the chunk declares the range {start}-{end} and a body of another length"
        ))
        .with_range(the_range_header(written)));
    }

    // The range, if the client sends it. The protocol says chunks
    // go in order and a chunk that does not fit gets a 416: the
    // client recovers the offset with a `GET` and continues. With a journal, the
    // offset **is the authority's**: the freshly reopened handle is
    // already at it, and a chunk that arrives late —another writer won
    // the CAS, or the client resends what is already written— falls into the same 416
    // with the `Range` to continue from, which is what makes the replay not
    // duplicate any part.
    if let Some(range) = the_content_range(&headers)
        && range.0 != written
    {
        // The status is the `416` the protocol specifies for a range
        // that cannot be satisfied, and the response's `Range`
        // says where to continue: the client recovers the offset and
        // continues.
        return Err(OciFailure::blob_upload_range_mismatch(format!(
            "the chunk starts at {} and the upload is at {written}",
            range.0
        ))
        .with_range(the_range_header(written)));
    }

    let (writer, ticket) = session
        .the_writer_and_ticket()
        .expect("a session with an offset has its writer");
    let length = write_the_body(
        writer.as_mut(),
        request.into_body(),
        state.max_body_size as u64,
        ticket,
    )
    .await?;

    // The advance is arbitrated by the authority: the journal only admits the
    // offset of what is already **durable** in the store —what is written in
    // the synced file; in multipart, what is confirmed by the
    // server—, never what the handle's buffer holds. If another
    // writer won the CAS, this `PATCH` answers the `416` with the real
    // `Range`: the chunk written ahead of the authority is not
    // published —it is trimmed on reopening, or left out of the complete—,
    // and the client resends it from the declared offset.
    let durable = session
        .flush_durable()
        .await
        .map_err(|error| the_blob_error(&error))?;
    match state
        .uploads
        .advance(&id, session.the_journal_version(), durable)
        .await
    {
        crate::oci::uploads::Advance::Admitted(version) => {
            session.set_journal_version(version);
            // With a journal and handle, the handle is released: the next
            // request —here or on another replica— resumes it through the
            // handle. A multipart with bytes in the buffer does not release;
            // the registry knows.
            session.release_writer_when_possible();
            // The progress that is announced is the cursor of **this**
            // handle: what the client must keep sending from
            // here. The journal declares the durable offset —lower
            // in multipart—, and that is the one the `Range` will say if the
            // process dies with the buffer: the client resends from what is
            // confirmed, and what is confirmed is the only thing that is published.
            Ok(the_upload_progress(&name, &id, written + length))
        }
        crate::oci::uploads::Advance::Conflict(offset) => {
            // The authority already has another offset: what this handle
            // wrote ahead is not of this session. The handle is released
            // without publishing —the resume trims the excess—, and
            // the 416 says where to continue.
            session.release_writer_when_possible();
            Err(OciFailure::blob_upload_range_mismatch(
                "another writer advanced this session; the chunk was not admitted",
            )
            .with_range(the_range_header(offset)))
        }
        crate::oci::uploads::Advance::Gone => Err(OciFailure::blob_upload_unknown(
            "the upload session does not exist",
        )),
        crate::oci::uploads::Advance::Unavailable(message) => Err(OciFailure::unavailable(
            format!("the session journal did not admit the chunk: {message}"),
        )),
    }
}

/// `GET /v2/<name>/blobs/uploads/<uuid>`
///
/// Says how much the session holds.
pub async fn upload_status(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(uri.path())?;
    let Target::UploadSession { name, session: id } = target else {
        return Err(OciFailure::unsupported(
            "this path does not carry an upload session",
        ));
    };
    let repository = the_repository(&state, &context, &name).await?;
    // A group stores nothing: writing against it is rejected with the
    // dialect error that the distribution reserves for what cannot be
    // done on a read-only repository.
    crate::oci::the_writable(&repository)?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    // The status can be queried **from any replica**: if the
    // local cache does not have the session written, the journal authority
    // says the offset without reopening the store —querying how much
    // an upload holds does not need to take the handle nor exclude whoever
    // has it—. A `GET` that did not resume could not say the progress of
    // a session opened on another replica, and the client would lose the
    // `Range` to resume.
    let written = match state.uploads.find(context.account().as_str(), &name, &id) {
        Some(shared) => {
            let mut session = shared.lock().await;
            session.touch();
            match session.the_local_offset() {
                Some(offset) => offset,
                // The handle is released: the authority says the durable
                // offset, which is the `Range` to continue from.
                None => state
                    .uploads
                    .the_journal_offset(context.account().as_str(), &name, &id)
                    .await?
                    .ok_or_else(|| {
                        OciFailure::blob_upload_unknown("the upload session does not exist")
                    })?,
            }
        }
        None => state
            .uploads
            .the_journal_offset(context.account().as_str(), &name, &id)
            .await?
            .ok_or_else(|| OciFailure::blob_upload_unknown("the upload session does not exist"))?,
    };

    let mut response = StatusCode::NO_CONTENT.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_upload(&name, &id))
            .expect("the location is a header value"),
    );
    headers.insert(
        header::RANGE,
        HeaderValue::from_str(&the_range_header(written)).expect("the range is a header value"),
    );
    headers.insert(
        UPLOAD_UUID_HEADER,
        HeaderValue::from_str(&id).expect("the session id is a header value"),
    );
    Ok(response)
}

/// `DELETE /v2/<name>/blobs/uploads/<uuid>`
///
/// Cancels the session: what had been written is never published.
async fn cancel_upload(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let target = the_target(uri.path())?;
    let Target::UploadSession { name, session: id } = target else {
        return Err(OciFailure::unsupported(
            "this path does not carry an upload session",
        ));
    };
    let repository = the_repository(&state, &context, &name).await?;
    // A group stores nothing: writing against it is rejected with the
    // dialect error that the distribution reserves for what cannot be
    // done on a read-only repository.
    crate::oci::the_writable(&repository)?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    // Cancelling is aborting the upload **according to the authority**: the session
    // may be open on another replica, and a DELETE that only looked at
    // the local cache could never cancel what another process
    // wrote. Revival takes the handle —the advisory lock
    // excludes the replica that holds it, and that DELETE is answered
    // 503 with retry—; what is aborted is the store's staging,
    // and the authority is closed with it.
    let store = the_content_store(&state)?;
    let shared = state
        .uploads
        .find_or_resume(context.account().as_str(), &name, &id, store.as_ref())
        .await?
        .ok_or_else(|| OciFailure::blob_upload_unknown("the upload session does not exist"))?;
    {
        let mut session = shared.lock().await;
        // The write abort closes the staging without publishing: a staging
        // file is deleted, a multipart is aborted in the
        // object store. The quota is released with the handle's reservation.
        if let Some(writer) = session.the_writer() {
            let _ = writer.abort().await;
        }
    }
    state.uploads.close(&id).await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `PUT`: either an upload session is closed, or a manifest is published.
async fn put(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> Result<Response, OciFailure> {
    let target = the_target(uri.path())?;
    match target {
        Target::UploadSession { name, session } => {
            close_upload(state, uri, headers, request, name, session).await
        }
        Target::Manifest { name, reference } => {
            put_manifest(state, uri, headers, request, name, reference).await
        }
        _ => Err(OciFailure::unsupported("this path cannot be written to")),
    }
}

/// `PUT /v2/<name>/blobs/uploads/<uuid>?digest=<digest>`
///
/// Closes the session: checks the digest, publishes the blob and returns the
/// place where it is.
async fn close_upload(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
    name: String,
    id: String,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let repository = the_repository(&state, &context, &name).await?;
    // A group stores nothing: writing against it is rejected with the
    // dialect error that the distribution reserves for what cannot be
    // done on a read-only repository.
    crate::oci::the_writable(&repository)?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    let query = the_query(&uri);
    let Some(declared) = query.digest.as_deref() else {
        return Err(OciFailure::digest_invalid(
            "the closing request must carry the digest of the whole blob",
        ));
    };
    let declared = the_digest_of_reference(declared)?;

    // The session being closed may be on **any replica**: it is
    // revived through the authority's handle, the bytes still missing are
    // written and it is checked against what was declared. The handle that
    // closes is not the owner of the truth of the offset: the journal is.
    let store = the_content_store(&state)?;
    let shared = state
        .uploads
        .find_or_resume(context.account().as_str(), &name, &id, store.as_ref())
        .await?
        .ok_or_else(|| OciFailure::blob_upload_unknown("the upload session does not exist"))?;
    let mut session = shared.lock().await;
    session.touch();

    // What the session already holds: the closing `PUT` may carry no
    // body (it is what clients that upload in chunks do), and what
    // is declared is the **whole blob**, not the last chunk. With a
    // journal, moreover, the handle may come reopened cold: the
    // first bytes were written by another replica, and this one has only seen
    // the missing ones.
    let already = session
        .the_local_offset()
        .expect("a resumed session has its writer");

    // A range that cannot be read is not checked: it is rejected.
    if the_range_is_malformed(&headers) {
        return Err(OciFailure::blob_upload_range_mismatch(
            "the chunk declares a range that cannot be read",
        )
        .with_range(the_range_header(already)));
    }

    // The range the client declares and the length of its body have to
    // match, for the same reason as in the `PATCH`.
    if let Some((start, end)) = the_range_does_not_match_the_length(&headers) {
        return Err(OciFailure::blob_upload_range_mismatch(format!(
            "the chunk declares the range {start}-{end} and a body of another length"
        ))
        .with_range(the_range_header(already)));
    }

    // The range, if the client sends it. The protocol says chunks
    // go in order: a closing `PUT` with a chunk that does not fit gets
    // the same `416` that a `PATCH` that does not fit gets, with the `Range`
    // that says where to continue. A `400` here would say "something is wrong" without
    // saying where the offset mismatch is.
    if let Some(range) = the_content_range(&headers)
        && range.0 != already
    {
        return Err(OciFailure::blob_upload_range_mismatch(format!(
            "the chunk starts at {} and the upload is at {already}",
            range.0
        ))
        .with_range(the_range_header(already)));
    }

    // The last chunk may come in the closing `PUT`.
    let (writer, ticket) = session
        .the_writer_and_ticket()
        .expect("a session with an offset has its writer");
    let last = write_the_body(
        writer.as_mut(),
        request.into_body(),
        state.max_body_size as u64,
        ticket,
    )
    .await?;
    let length = already + last;

    // The declared digest: what the client says the whole blob is.
    // The store's commit checks it against the complete staging —
    // whatever handle closes—; the local check here
    // is a cheap optimization for the handle that went through all the bytes.
    let expectation = ContentDigest::sha256(declared.clone());

    // Before publishing, the authority has to admit the offset: a
    // close that published ahead of it would publish bytes that
    // nobody declared. The journal admits up to the handle's **durable** part;
    // a `416` here says that another writer won and this close
    // publishes nothing.
    let durable = session
        .flush_durable()
        .await
        .map_err(|error| the_blob_error(&error))?;
    match state
        .uploads
        .advance(&id, session.the_journal_version(), durable)
        .await
    {
        crate::oci::uploads::Advance::Admitted(version) => {
            session.set_journal_version(version);
        }
        crate::oci::uploads::Advance::Conflict(offset) => {
            session.release_writer_when_possible();
            return Err(OciFailure::blob_upload_range_mismatch(
                "another writer advanced this session; the close was not admitted",
            )
            .with_range(the_range_header(offset)));
        }
        crate::oci::uploads::Advance::Gone => {
            return Err(OciFailure::blob_upload_unknown(
                "the upload session does not exist",
            ));
        }
        crate::oci::uploads::Advance::Unavailable(message) => {
            return Err(OciFailure::unavailable(format!(
                "the session journal did not admit the close: {message}"
            )));
        }
    }

    if session.the_digest_covers_all() {
        let found = session
            .the_writer()
            .expect("a session that covers all has its writer")
            .the_digest();
        if found.value != declared {
            // The digest does not match: the upload is discarded and no
            // visible reference remains. A blob with a digest that is not
            // its own is not a blob. The explicit close removes
            // the authority too: a rejected session is not left
            // resumable.
            let _ = session.the_writer().expect("the writer").abort().await;
            drop(session);
            let _ = state.uploads.close(&id).await;
            return Err(OciFailure::digest_invalid(
                "the content does not match the declared digest",
            ));
        }
    }
    // What the store gains is what the account pays for: a layer that was already
    // pushed by someone else adds no bytes. It is looked at before the
    // commit, and settled as soon as the blob exists: what the catalog
    // publishes afterwards is another transaction, and the bytes are already there.
    let already_had = the_content_store(&state)?
        .exists(&expectation)
        .await
        .map_err(|error| the_blob_error(&error))?;
    let commit = session
        .the_writer()
        .expect("a session with an offset has its writer")
        .commit(CommitExpectation {
            digest: expectation.clone(),
            length,
        })
        .await;
    let digest = match commit {
        Ok(digest) => digest,
        Err(error) => {
            // The commit rejected the content —or the store failed—: the
            // session's authority ceases to exist. An unverified staging
            // is not a resumable session.
            drop(session);
            let _ = state.uploads.close(&id).await;
            return Err(the_blob_error(&error));
        }
    };
    session.settle(if already_had { 0 } else { length });
    drop(session);
    // The authority is closed with the blob already published: what the
    // journal declared is exactly what the store verified. If
    // the close fails, the blob **is** published —the commit's promise
    // is stronger than the journal's mark—: the local seat is removed
    // and the open flag is resolved by the TTL sweep, whose resume of the
    // already consumed staging publishes nothing. The client receives its 201.
    if let Err(error) = state.uploads.close(&id).await {
        state.uploads.remove(&id);
        let _ = error;
    }

    publish_blob(&state, &context, &name, &digest, length, &headers).await?;

    let mut response = StatusCode::CREATED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_blob(&name, &digest.value))
            .expect("the location is a header value"),
    );
    headers.insert(
        CONTENT_DIGEST_HEADER,
        HeaderValue::from_str(&format!("sha256:{}", digest.value))
            .expect("the digest is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// `PUT /v2/<name>/manifests/<reference>`
///
/// Publishes the manifest.
///
/// What is stored are the **exact bytes** the client sent: the
/// manifest is the truth of what it is, and a rewritten manifest is a
/// manifest with another digest.
async fn put_manifest(
    state: AppState,
    uri: OriginalUri,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
    name: String,
    reference: String,
) -> Result<Response, OciFailure> {
    let context = the_context(&state, &headers).await?;
    let repository = the_repository(&state, &context, &name).await?;
    // A group stores nothing: writing against it is rejected with the
    // dialect error that the distribution reserves for what cannot be
    // done on a read-only repository.
    crate::oci::the_writable(&repository)?;
    the_decision(
        &state,
        &context,
        crate::actions::upload_asset(),
        &repository,
    )?;

    // The quota decides before reading: a manifest that does not fit is not
    // even read. If the client did not declare a length, the body is
    // charged against the ledger as soon as it arrives whole.
    let mut ticket = state
        .quota
        .begin(context.account().as_str(), the_declared_size(&headers))
        .map_err(the_quota_refusal)?;

    // The manifest bytes, as they are.
    let body = read_the_body(request.into_body(), state.max_body_size as u64).await?;
    if let Err(refusal) = ticket.admit(body.len() as u64) {
        return Err(the_quota_refusal(refusal));
    }

    // The media type is declared by the header, and it has to be from the
    // subset this registry publishes.
    let declared_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or(value).trim())
        .unwrap_or_default()
        .to_string();
    if declared_type.is_empty() {
        return Err(OciFailure::manifest_invalid(
            "the manifest must declare its media type",
        ));
    }
    // The manifest class says which descriptors to require, and a
    // type this registry does not speak is not published.
    let Some(kind) = hodei_hub::oci::the_kind_of(&declared_type) else {
        return Err(OciFailure::manifest_invalid(format!(
            "the registry does not publish manifests of type {declared_type}"
        )));
    };

    // And the header has to match the body's `mediaType`
    // when the body carries it: the manifest says what it is, and the
    // header is what the client asked for.
    let manifest = serde_json::from_slice::<serde_json::Value>(&body).map_err(|error| {
        OciFailure::manifest_invalid(format!("the manifest is not readable: {error}"))
    })?;
    match manifest.get("mediaType").and_then(|value| value.as_str()) {
        Some(declared) if declared != declared_type => {
            return Err(OciFailure::manifest_invalid(format!(
                "the manifest declares {declared} and the request declares {declared_type}"
            )));
        }
        // The media-spec allows **omitting** the `mediaType` and inheriting it from
        // the header: `helm push` omits it. The header was already required
        // above and already checked to be a type this registry
        // publishes, so the manifest does declare its type: it declares it
        // in the request. A later `helm pull` confirms it.
        None | Some(_) => {}
    }

    // The digest: if the reference is a digest, it has to be that of the
    // bytes that arrived. If not, nothing is published and the tag is not
    // created.
    let digest = ContentDigest::of_bytes(&body);
    if is_a_digest(&reference) {
        let expected = the_digest_of_reference(&reference)?;
        if expected != digest.value {
            return Err(OciFailure::digest_invalid(format!(
                "the manifest does not match the digest {reference}"
            )));
        }
    }

    // The required references: what the manifest names has to be
    // in the repository. A manifest that points to something that is not
    // there is a manifest that cannot be downloaded.
    //
    // The `subject` is **not** required: the protocol says a manifest
    // whose `subject` is not yet there is accepted, because a client can
    // upload the referrer before the subject.
    let required = the_required_digests(&manifest, kind)?;
    let subject = the_subject_of(&manifest)?;
    let (reader, _) = the_catalog(&state)?;
    for (required, class) in &required {
        let path = match class {
            TheClass::Blob => format!("blobs/{required}"),
            TheClass::Manifest => format!("manifests/{required}"),
        };
        let asset = reader.asset(context.account().as_str(), &name, &path).await;
        if let Err(error) = asset {
            // What is **not** there is a client error: the manifest
            // names something the repository does not have. What the
            // catalog does not know is another matter, and is said as what it is.
            return Err(match error {
                hodei_catalog::CatalogError::Missing { .. } => {
                    OciFailure::manifest_blob_unknown(format!(
                        "the repository does not have the {} {required}",
                        class.as_str()
                    ))
                }
                other => crate::oci::the_catalog_error(&other),
            });
        }
    }

    // The bytes, as they are. What the store gains is what the account
    // pays for: the same manifest twice does not take up space twice.
    let store = the_content_store(&state)?;
    let already_had = store
        .exists(&digest)
        .await
        .map_err(|error| the_blob_error(&error))?;
    store
        .put(&body)
        .await
        .map_err(|error| the_blob_error(&error))?;
    ticket.settle(if already_had { 0 } else { body.len() as u64 });

    // The tag: the reference is a tag, or the client asks for
    // tags with `?tag=`. Both can happen at once.
    let mut tags = Vec::new();
    if !is_a_digest(&reference) {
        tags.push(reference.clone());
    }
    tags.extend(the_query(&uri).tags);
    for tag in &tags {
        the_tag_name(tag)?;
    }

    let asset_path = format!("manifests/{}", digest.value);
    let coordinates = PackageCoordinates {
        package_name: asset_path.clone(),
        package_version: digest.value.clone(),
    };
    let publication = Publication {
        repository: name.clone(),
        asset_path: asset_path.clone(),
        coordinates: coordinates.clone(),
        digest: digest.clone(),
        size_bytes: body.len() as u64,
        // Tags are **not** part of the publication: a tag is
        // mutable in the protocol, and the publication creates the tag
        // once. Retagging is moving it, which is another operation.
        tags: Vec::new(),
        actor: context.principal().subject().as_str().to_string(),
        correlation_id: correlation_id(&headers, &state.identity),
        // Replacement is decided by the **repository**, not the client: a
        // published manifest is not rewritten, and what is behind
        // a tag that moves stays declared with its digest.
        overwrite: true,
        precondition: None,
        // The `subject` goes in the publication, and is written with it: a
        // half-visible referrer would be a referrer that exists when
        // it does not exist.
        subject: subject.clone(),
    };
    let (_, writer) = the_catalog(&state)?;
    writer
        .publish(context.account().as_str(), publication)
        .await
        .map_err(|error| the_catalog_error(&error))?;

    // Tags **move**. The protocol says a tag is
    // mutable: publishing a manifest with the name of a tag that
    // already exists repoints the tag, and what is behind the
    // previous one stays published with its digest.
    //
    // It is what distinguishes a registry from a store: in a store the
    // name is the thing, and in a registry the tag is a reference
    // that can be moved. Moving it touches the bytes of neither of the
    // two manifests.
    for tag in &tags {
        writer
            .move_tag(context.account().as_str(), &name, tag, coordinates.clone())
            .await
            .map_err(|error| the_catalog_error(&error))?;
    }

    let correlation = correlation_id(&headers, &state.identity);
    record_audit(
        &state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        "publish",
        &format!("{name}/manifests/{}", digest.value),
        AuditResult::Allowed,
        &correlation,
    );

    let mut response = StatusCode::CREATED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_manifest(&name, &digest.value))
            .expect("the location is a header value"),
    );
    headers.insert(
        CONTENT_DIGEST_HEADER,
        HeaderValue::from_str(&format!("sha256:{}", digest.value))
            .expect("the digest is a header value"),
    );
    // The tags the manifest accepted: the client asks for them
    // with `?tag=` and the registry says them one by one.
    for tag in &tags {
        if let Ok(value) = HeaderValue::from_str(tag) {
            headers.append(OCI_TAG_HEADER, value);
        }
    }
    // The manifest's `subject`, if it has one: the protocol says the
    // response may state it, and it does so because the client uploading a
    // referrer wants to know what it hangs from.
    if let Some(subject) = &subject
        && let Ok(value) = HeaderValue::from_str(&subject.to_string())
    {
        headers.insert(OCI_SUBJECT_HEADER, value);
    }
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    Ok(response)
}

/// The digests the manifest requires to be in the repository.
///
/// The manifest class says **what** is required:
///
/// - An **image manifest** requires its config and its layers.
/// - An **index** requires its children, which are manifests.
/// - An **artifact manifest** requires nothing: its config and its
///   blobs are optional, and what it names has to be there.
///
/// The digest is returned **without the algorithm**, which is how the
/// catalog names it: a blob lives at `blobs/<digest>`, and the `sha256:` prefix
/// belongs to the OCI reference, not to the asset path. A digest that
/// does not carry it is not a digest, and a manifest that names it cannot be
/// checked.
fn the_required_digests(
    manifest: &serde_json::Value,
    kind: ManifestKind,
) -> Result<Vec<(String, TheClass)>, OciFailure> {
    let mut required = Vec::new();
    match kind {
        ManifestKind::Image => {
            let config = manifest
                .get("config")
                .and_then(|config| config.get("digest"))
                .and_then(|digest| digest.as_str())
                .ok_or_else(|| {
                    OciFailure::manifest_invalid("the manifest does not name a config")
                })?;
            // The **empty** descriptor is not required: it says "this image has no
            // config", and a registry that required it in the
            // store would reject a manifest the protocol says
            // is valid. A client that sends it inline --with its
            // `data`-- never uploads it.
            if !the_empty_descriptor(manifest.get("config")) {
                required.push((the_hexadecimal_digest(config)?, TheClass::Blob));
            }
            if let Some(layers) = manifest.get("layers").and_then(|layers| layers.as_array()) {
                for layer in layers {
                    let Some(digest) = layer.get("digest").and_then(|digest| digest.as_str())
                    else {
                        return Err(OciFailure::manifest_invalid(
                            "a layer of the manifest does not name a digest",
                        ));
                    };
                    if the_empty_descriptor(Some(layer)) {
                        continue;
                    }
                    // A **foreign** layer --one that declares its `urls`--
                    // is not in the store: the client downloads it from where
                    // the manifest says. Requiring it would be rejecting a
                    // manifest the protocol says is valid.
                    if the_foreign_descriptor(Some(layer)) {
                        if !the_foreign_urls_are_admisible(layer) {
                            return Err(OciFailure::manifest_invalid(
                                "the urls of a non-distributable layer must be https",
                            ));
                        }
                        continue;
                    }
                    required.push((the_hexadecimal_digest(digest)?, TheClass::Blob));
                }
            }
        }
        ManifestKind::Index => {
            let children = manifest
                .get("manifests")
                .and_then(|manifests| manifests.as_array())
                .ok_or_else(|| {
                    OciFailure::manifest_invalid("the index does not name its manifests")
                })?;
            for child in children {
                let Some(digest) = child.get("digest").and_then(|digest| digest.as_str()) else {
                    return Err(OciFailure::manifest_invalid(
                        "a manifest of the index does not name a digest",
                    ));
                };
                required.push((the_hexadecimal_digest(digest)?, TheClass::Manifest));
            }
        }
        ManifestKind::Artifact => {
            // An artifact's config is optional, and so are its blobs:
            // what is not named is not required.
            //
            // And what is named with the **empty** descriptor is not
            // required either: that descriptor says "there is no config",
            // and a registry that required it in the store would reject a
            // manifest the protocol says is valid.
            if let Some(config) = manifest.get("config") {
                let Some(digest) = config.get("digest").and_then(|digest| digest.as_str()) else {
                    return Err(OciFailure::manifest_invalid(
                        "the config of the artifact does not name a digest",
                    ));
                };
                if !the_empty_descriptor(Some(config)) {
                    required.push((the_hexadecimal_digest(digest)?, TheClass::Blob));
                }
            }
            if let Some(blobs) = manifest.get("blobs").and_then(|blobs| blobs.as_array()) {
                for blob in blobs {
                    let Some(digest) = blob.get("digest").and_then(|digest| digest.as_str()) else {
                        return Err(OciFailure::manifest_invalid(
                            "a blob of the artifact does not name a digest",
                        ));
                    };
                    // The foreign layer --the non-distributable one-- is not
                    // required in the store either: the criterion is the same as for
                    // an image's layers, and an exemption that depends
                    // on the shape of the manifest is an exemption that
                    // gets forgotten.
                    if the_foreign_descriptor(Some(blob)) {
                        if !the_foreign_urls_are_admisible(blob) {
                            return Err(OciFailure::manifest_invalid(
                                "the urls of a non-distributable blob must be https",
                            ));
                        }
                        continue;
                    }
                    required.push((the_hexadecimal_digest(digest)?, TheClass::Blob));
                }
            }
        }
    }
    Ok(required)
}

/// Whether a descriptor is the **empty** descriptor.
///
/// The empty descriptor says "there is nothing here": an artifact manifest
/// without config, or an image without config. The bytes it
/// names are a two-byte `{}`, and a client can send them inline
/// --with its `data`-- without ever uploading them to the store. A registry
/// that required them in the store would reject a valid manifest.
fn the_empty_descriptor(descriptor: Option<&serde_json::Value>) -> bool {
    let Some(descriptor) = descriptor else {
        return false;
    };
    let media_type = descriptor
        .get("mediaType")
        .and_then(|media_type| media_type.as_str())
        .unwrap_or_default();
    let digest = descriptor
        .get("digest")
        .and_then(|digest| digest.as_str())
        .unwrap_or_default();
    hodei_hub::oci::is_the_empty_descriptor(media_type, digest)
}

/// Whether a descriptor is that of a **foreign** layer.
///
/// The foreign layer is the **non-distributable** one: its media type says so
/// (`application/vnd.oci.image.layer.nondistributable.v1.tar`, with or without
/// `+gzip`), and its `urls` say where it is downloaded from. The registry does not have it
/// nor has to have it, and a manifest that names it is a valid
/// manifest: requiring the layer would be rejecting a manifest the protocol
/// says is valid.
///
/// The exemption is **exactly** that of the non-distributable layer, and not
/// that of any descriptor with `urls`: a descriptor that declares `urls` and
/// any media type is a normal layer with a hint, and
/// exempting it would mean no longer requiring in the store content that the
/// manifest names. The media type is what distinguishes it.
fn the_foreign_descriptor(descriptor: Option<&serde_json::Value>) -> bool {
    let Some(descriptor) = descriptor else {
        return false;
    };
    let Some(media_type) = descriptor.get("mediaType").and_then(|media| media.as_str()) else {
        return false;
    };
    if !THE_FOREIGN_MEDIA_TYPES.contains(&media_type) {
        return false;
    }
    descriptor
        .get("urls")
        .and_then(|urls| urls.as_array())
        .is_some_and(|urls| !urls.is_empty())
}

/// Whether the `urls` of a non-distributable layer are admissible.
///
/// The layer is downloaded from where the manifest says, and that "where" is decided by
/// whoever publishes: an `http` to an internal address --that of the cloud
/// credentials-- is a client that is going to ask for the
/// credentials of the machine that pulls the image. That is why only
/// `https` is admitted: the protocol allows any URI, and a supply-chain
/// platform cannot go publishing addresses that the
/// client is going to follow.
///
/// The layer's exemption is necessary --the protocol has it--, but the
/// exemption does not have to be an open door.
fn the_foreign_urls_are_admisible(descriptor: &serde_json::Value) -> bool {
    let Some(urls) = descriptor.get("urls").and_then(|urls| urls.as_array()) else {
        return false;
    };
    urls.iter()
        .all(|url| url.as_str().is_some_and(|url| url.starts_with("https://")))
}

/// The media types of a **non-distributable** layer.
///
/// They are the ones defined by the image specification, and they are the only ones
/// exempted from being in the store.
const THE_FOREIGN_MEDIA_TYPES: [&str; 2] = [
    "application/vnd.oci.image.layer.nondistributable.v1.tar",
    "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip",
];

/// The class of what a manifest names.
///
/// A blob lives at `blobs/<digest>` and a manifest at
/// `manifests/<digest>`: they are different paths, and a manifest that requires
/// one cannot serve the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheClass {
    /// A blob of the repository.
    Blob,
    /// A manifest of the repository.
    Manifest,
}

impl TheClass {
    /// The class name, for the error message.
    fn as_str(&self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Manifest => "manifest",
        }
    }
}

/// The manifest's `subject`, if it has one.
///
/// It is the digest of the manifest this one hangs from, and it is **not** required to
/// exist: the protocol says a registry has to accept a
/// manifest whose `subject` is not yet there, because a client can
/// upload the referrer before the subject.
fn the_subject_of(manifest: &serde_json::Value) -> Result<Option<ContentDigest>, OciFailure> {
    let Some(subject) = manifest.get("subject") else {
        return Ok(None);
    };
    let Some(digest) = subject.get("digest").and_then(|digest| digest.as_str()) else {
        return Err(OciFailure::manifest_invalid(
            "the subject does not name a digest",
        ));
    };
    Ok(Some(ContentDigest::sha256(the_hexadecimal_digest(digest)?)))
}

/// The hexadecimal of a digest, without its algorithm.
///
/// A digest without the `sha256:` prefix is not a digest: the manifest
/// has to name the content the way the protocol names it, and a half-written
/// digest cannot be checked.
fn the_hexadecimal_digest(digest: &str) -> Result<String, OciFailure> {
    let Some((algorithm, hexadecimal)) = digest.split_once(':') else {
        return Err(OciFailure::manifest_invalid(format!(
            "the digest {digest:?} does not carry its algorithm"
        )));
    };
    if algorithm != "sha256"
        || hexadecimal.is_empty()
        || !hexadecimal.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(OciFailure::manifest_invalid(format!(
            "the digest {digest:?} is not one this registry understands"
        )));
    }
    // And it has to be **a** digest: a half-written hexadecimal does not
    // name a content, and a manifest that names it cannot be
    // checked. Without this check, a half-written `subject`
    // would be stored in the catalog and the manifest that declared it
    // would be left published and unreadable forever.
    let candidate = ContentDigest::sha256(hexadecimal);
    if !candidate.is_well_formed() {
        return Err(OciFailure::manifest_invalid(format!(
            "the digest {digest:?} is not a well formed digest"
        )));
    }
    Ok(candidate.value)
}

/// Writes the body into the write session.
///
/// The bytes go **in chunks**, and the digest is computed as they pass:
/// the client may declare what it will be, and if it declares none, the
/// server computes it over what arrives. In both cases, the commit
/// checks that what is published is exactly what was declared.
///
/// Each chunk is charged to the ledger **before** it is written, over
/// the session's accumulated total: the quota is applied before
/// storing, not after having already stored the excess.
async fn write_the_body(
    writer: &mut dyn BlobWriteSession,
    body: Body,
    limit: u64,
    ticket: &mut crate::quota::QuotaTicket,
) -> Result<u64, OciFailure> {
    let mut stream = body.into_data_stream();
    let mut length: u64 = 0;
    let base = writer.bytes_written();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| OciFailure::blob_upload_invalid("the body could not be read"))?;
        length = length.saturating_add(chunk.len() as u64);
        if length > limit {
            // The content is larger than what is admitted: the
            // write is aborted and it is said, instead of accepting it and
            // failing later because of disk.
            let _ = writer.abort().await;
            return Err(OciFailure::size_invalid(format!(
                "the content is larger than the {limit} bytes this service accepts"
            )));
        }
        if let Err(refusal) = ticket.admit(base + length) {
            let _ = writer.abort().await;
            return Err(the_quota_refusal(refusal));
        }
        writer
            .append(&chunk)
            .await
            .map_err(|error| the_blob_error(&error))?;
    }
    Ok(length)
}

/// The length the client declared, if it declared one.
///
/// It is the `Content-Length`, and it only counts if it is a number: a
/// strange header is not a promise, it is noise, and the ledger is left without a
/// declared length and charges the body chunk by chunk.
fn the_declared_size(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

/// The quota's refusal, stated in the distribution dialect.
fn the_quota_refusal(refusal: crate::quota::QuotaRefusal) -> OciFailure {
    match refusal {
        crate::quota::QuotaRefusal::StorageOver { .. } => OciFailure::storage_over_quota(
            "the content would take this account past its stored quota",
        ),
        crate::quota::QuotaRefusal::UploadsSaturated { retry_after_secs } => {
            OciFailure::too_many_requests(
                "this account already has as many uploads in flight as it may have",
                retry_after_secs,
            )
        }
    }
}

/// Reads the whole body.
///
/// The manifest is a document: it is read whole to be able to compare it with
/// its media type and to know which digests it requires. The limit is the
/// same as that of any other write.
async fn read_the_body(body: Body, limit: u64) -> Result<Vec<u8>, OciFailure> {
    let mut bytes = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| OciFailure::manifest_invalid("the body could not be read"))?;
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > limit {
            return Err(OciFailure::size_invalid(format!(
                "the manifest is larger than the {limit} bytes this service accepts"
            )));
        }
    }
    Ok(bytes)
}

/// The range the client declares in the `Content-Range`.
///
/// The protocol says the range is `start-end`, with both
/// ends included, and that the first chunk has to start at 0.
fn the_content_range(headers: &HeaderMap) -> Option<(u64, u64)> {
    let value = headers
        .get("content-range")
        .and_then(|value| value.to_str().ok())?
        .trim();
    // HTTP writes the range with a unit and a total (`bytes 0-99/200`), and
    // the OCI protocol writes it bare (`0-99`). Both are read.
    let value = value.strip_prefix("bytes ").unwrap_or(value).trim();
    // The total does not say where the chunk goes: it is ignored.
    let value = value.split('/').next().unwrap_or(value).trim();
    let (start, end) = value.split_once('-')?;
    Some((start.trim().parse().ok()?, end.trim().parse().ok()?))
}

/// Whether the range the client declares is **malformed**.
///
/// The protocol says the range has to be well formed, and a
/// client that sends it wrong cannot write where it wants: a range that
/// is not understood is a range that cannot be satisfied. Without this
/// check, a `Content-Range` that cannot be read is overlooked and
/// the chunk is accepted without checking anything.
fn the_range_is_malformed(headers: &HeaderMap) -> bool {
    headers.get("content-range").is_some() && the_content_range(headers).is_none()
}

/// Whether the range the client declares does not match the length of its
/// body.
///
/// The protocol says the `Content-Range` is the chunk's range and the
/// `Content-Length` is the chunk's length: the two say the same. A
/// client that declares one thing and sends another does not know where it is, and the
/// response is the same one an out-of-order chunk receives: a `416` with
/// the `Range` that says where to continue.
///
/// Without `Content-Range` there is nothing to compare: the range is optional.
fn the_range_does_not_match_the_length(headers: &HeaderMap) -> Option<(u64, u64)> {
    let (start, end) = the_content_range(headers)?;
    let length: u64 = headers
        .get(axum::http::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    if end < start || end - start + 1 != length {
        return Some((start, end));
    }
    None
}

/// The `Range` that says how much the upload holds.
///
/// The protocol expresses it as `0-<last byte received>`, and an
/// upload with no bytes says `0-0`.
fn the_range_header(written: u64) -> String {
    if written == 0 {
        "0-0".to_string()
    } else {
        format!("0-{}", written - 1)
    }
}

/// The response of an upload that is still in progress.
fn the_upload_progress(name: &str, id: &str, written: u64) -> Response {
    let mut response = StatusCode::ACCEPTED.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&the_location_of_upload(name, id))
            .expect("the location is a header value"),
    );
    headers.insert(
        header::RANGE,
        HeaderValue::from_str(&the_range_header(written)).expect("the range is a header value"),
    );
    headers.insert(
        UPLOAD_UUID_HEADER,
        HeaderValue::from_str(id).expect("the session id is a header value"),
    );
    headers.insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    response
}

/// The location of a blob.
fn the_location_of_blob(name: &str, digest: &str) -> String {
    format!("/v2/{name}/blobs/sha256:{digest}")
}

/// The location of a manifest.
fn the_location_of_manifest(name: &str, digest: &str) -> String {
    format!("/v2/{name}/manifests/sha256:{digest}")
}

/// The location of an upload session.
fn the_location_of_upload(name: &str, session: &str) -> String {
    format!("/v2/{name}/blobs/uploads/{session}")
}

/// Publishes a blob as an asset of the repository.
///
/// The blob is an asset at `blobs/<digest>`: the row is what makes
/// the bytes **belong to this repository**, and what authorizes serving them.
async fn publish_blob(
    state: &AppState,
    context: &hodei_authz::VerifiedContext,
    name: &str,
    digest: &ContentDigest,
    size_bytes: u64,
    headers: &HeaderMap,
) -> Result<(), OciFailure> {
    let asset_path = format!("blobs/{}", digest.value);
    let publication = Publication {
        repository: name.to_string(),
        asset_path: asset_path.clone(),
        coordinates: PackageCoordinates {
            package_name: asset_path,
            package_version: digest.value.clone(),
        },
        digest: digest.clone(),
        size_bytes,
        tags: Vec::new(),
        actor: context.principal().subject().as_str().to_string(),
        correlation_id: correlation_id(headers, &state.identity),
        // The path carries the digest: republishing the same blob is the
        // same, and there is nothing to replace.
        overwrite: true,
        precondition: None,
        subject: None,
    };
    let (_, writer) = the_catalog(state)?;
    writer
        .publish(context.account().as_str(), publication)
        .await
        .map_err(|error| the_catalog_error(&error))?;
    Ok(())
}

/// An error of the content store, translated to the dialect.
pub(crate) fn the_blob_error(error: &BlobError) -> OciFailure {
    match error {
        BlobError::NotFound => OciFailure::blob_unknown("the content is not there"),
        // The content is not what was declared: it is a client
        // error, and the client code says so.
        BlobError::DigestMismatch { .. } => {
            OciFailure::digest_invalid("the content does not match the declared digest")
        }
        BlobError::LengthMismatch { .. } => {
            OciFailure::size_invalid("the content does not have the declared length")
        }
        // The store is broken: it is not the client's fault.
        BlobError::Corrupted { .. } => {
            OciFailure::unavailable("the content store is not available")
        }
        BlobError::UnsupportedAlgorithm(_) => {
            OciFailure::digest_invalid("the registry does not compute digests with that algorithm")
        }
        BlobError::SessionClosed => OciFailure::blob_upload_invalid("the upload session is closed"),
        BlobError::Unavailable(_) => OciFailure::unavailable("the content store is not available"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real digest: the tests do not have to measure the digest's
    /// length, they have to measure **which** digest is required.
    const A_DIGEST: &str =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const B_DIGEST: &str =
        "sha256:2222222222222222222222222222222222222222222222222222222222222222";
    const C_DIGEST: &str =
        "sha256:3333333333333333333333333333333333333333333333333333333333333333";

    /// The hexadecimal of a real digest.
    fn the_hexadecimal(digest: &str) -> &str {
        digest
            .strip_prefix("sha256:")
            .expect("the digest carries its algorithm")
    }

    /// The range the client declares is read with both of its ends.
    #[test]
    fn the_content_range_is_read_with_both_ends() {
        let mut headers = HeaderMap::new();
        headers.insert("content-range", "0-99".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), Some((0, 99)));

        headers.insert("content-range", "100-199".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), Some((100, 199)));

        headers.insert("content-range", "not-a-range".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), None);
    }

    /// The declared range and the body length have to match.
    ///
    /// The protocol says the `Content-Range` is the chunk's range and
    /// the `Content-Length` is its length: the two say the same.
    #[test]
    fn a_range_that_does_not_match_the_length_is_a_mismatch() {
        let mut headers = HeaderMap::new();
        headers.insert("content-range", "0-99".parse().expect("a header"));
        headers.insert("content-length", "100".parse().expect("a header"));
        assert_eq!(the_range_does_not_match_the_length(&headers), None);

        // The same range with a body of another length.
        headers.insert("content-length", "3".parse().expect("a header"));
        assert_eq!(the_range_does_not_match_the_length(&headers), Some((0, 99)));

        // A reversed range: the end is before the start.
        headers.insert("content-range", "99-0".parse().expect("a header"));
        headers.insert("content-length", "100".parse().expect("a header"));
        assert_eq!(the_range_does_not_match_the_length(&headers), Some((99, 0)));
    }

    /// Without `Content-Range` there is nothing to compare: the range is
    /// optional, and the `Content-Length` alone says nothing.
    #[test]
    fn without_a_range_there_is_nothing_to_compare() {
        let mut headers = HeaderMap::new();
        headers.insert("content-length", "100".parse().expect("a header"));
        assert_eq!(the_range_does_not_match_the_length(&headers), None);
    }

    /// The **non-distributable** layer is the one exempted from being in the
    /// store: its media type says so, and its `urls` say where it is
    /// downloaded from.
    #[test]
    fn a_foreign_layer_is_the_one_that_is_not_distributed() {
        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip",
            "digest": A_DIGEST,
            "urls": ["https://example.invalid/layer"],
        });
        assert!(the_foreign_descriptor(Some(&layer)));

        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": A_DIGEST,
            "urls": ["https://example.invalid/layer"],
        });
        assert!(!the_foreign_descriptor(Some(&layer)));
    }

    /// A normal layer without `urls` is not foreign: it is required in the
    /// store, like any other.
    #[test]
    fn a_normal_layer_is_not_foreign() {
        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": A_DIGEST,
        });
        assert!(!the_foreign_descriptor(Some(&layer)));
    }

    /// The range is read in both spellings: the protocol's (`0-99`) and
    /// HTTP's (`bytes 0-99/200`).
    #[test]
    fn the_content_range_is_read_in_both_spellings() {
        let mut headers = HeaderMap::new();
        headers.insert("content-range", "bytes 0-99/200".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), Some((0, 99)));

        headers.insert("content-range", "bytes 100-199".parse().expect("a header"));
        assert_eq!(the_content_range(&headers), Some((100, 199)));
    }

    /// A range that is there and cannot be read is a malformed range: it is
    /// not overlooked.
    #[test]
    fn a_range_that_cannot_be_read_is_malformed() {
        let mut headers = HeaderMap::new();
        headers.insert("content-range", "not-a-range".parse().expect("a header"));
        assert!(the_range_is_malformed(&headers));

        headers.insert("content-range", "0-99".parse().expect("a header"));
        assert!(!the_range_is_malformed(&headers));

        // Without a header there is nothing malformed: the range is optional.
        assert!(!the_range_is_malformed(&HeaderMap::new()));
    }

    /// The `urls` of the non-distributable layer have to be `https`: the
    /// client is going to follow them, and whoever publishes decides where it goes.
    #[test]
    fn the_urls_of_a_foreign_layer_have_to_be_https() {
        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.nondistributable.v1.tar",
            "digest": A_DIGEST,
            "urls": ["https://store.example.com/blobs/sha256/x"],
        });
        assert!(the_foreign_urls_are_admisible(&layer));

        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.nondistributable.v1.tar",
            "digest": A_DIGEST,
            "urls": ["http://169.254.169.254/latest/meta-data/"],
        });
        assert!(!the_foreign_urls_are_admisible(&layer));

        let layer = serde_json::json!({
            "mediaType": "application/vnd.oci.image.layer.nondistributable.v1.tar",
            "digest": A_DIGEST,
            "urls": ["file:///etc/passwd"],
        });
        assert!(!the_foreign_urls_are_admisible(&layer));
    }

    /// The range the upload states is `0-<last byte>`.
    #[test]
    fn the_range_of_the_upload_is_the_last_byte() {
        assert_eq!(the_range_header(0), "0-0");
        assert_eq!(the_range_header(1), "0-0");
        assert_eq!(the_range_header(100), "0-99");
    }

    /// The required references of an image manifest are its config
    /// and its layers, and nothing else.
    ///
    /// The digest stays **without the algorithm**: it is how the
    /// catalog names it, which stores the blob at `blobs/<digest>`.
    #[test]
    fn the_required_digests_are_the_config_and_the_layers() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "digest": A_DIGEST },
            "layers": [{ "digest": B_DIGEST }, { "digest": C_DIGEST }],
        });
        let required = the_required_digests(&manifest, ManifestKind::Image).expect("the digests");
        assert_eq!(
            required,
            vec![
                (the_hexadecimal(A_DIGEST).to_string(), TheClass::Blob),
                (the_hexadecimal(B_DIGEST).to_string(), TheClass::Blob),
                (the_hexadecimal(C_DIGEST).to_string(), TheClass::Blob),
            ]
        );
    }

    /// A digest without its algorithm is not a digest, and a manifest that
    /// names it cannot be checked.
    #[test]
    fn a_required_digest_without_its_algorithm_is_refused() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "digest": "aaa" },
            "layers": [],
        });
        let error = the_required_digests(&manifest, ManifestKind::Image)
            .expect_err("there is no algorithm");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");

        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": { "digest": "md5:aaa" },
            "layers": [],
        });
        let error = the_required_digests(&manifest, ManifestKind::Image)
            .expect_err("the algorithm is not sha256");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");
    }

    /// A manifest without config is not an image manifest.
    #[test]
    fn a_manifest_without_a_config_is_not_publishable() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "layers": [],
        });
        let error =
            the_required_digests(&manifest, ManifestKind::Image).expect_err("there is no config");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");
    }

    /// An index requires its children, which are **manifests** and not blobs.
    #[test]
    fn an_index_requires_its_children_as_manifests() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [
                { "digest": A_DIGEST, "platform": { "os": "linux", "architecture": "amd64" } },
                { "digest": B_DIGEST, "platform": { "os": "linux", "architecture": "arm64" } }
            ],
        });
        let required = the_required_digests(&manifest, ManifestKind::Index).expect("the digests");
        assert_eq!(
            required,
            vec![
                (the_hexadecimal(A_DIGEST).to_string(), TheClass::Manifest),
                (the_hexadecimal(B_DIGEST).to_string(), TheClass::Manifest),
            ]
        );

        // An index without children is not an index.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
        });
        let error = the_required_digests(&manifest, ManifestKind::Index)
            .expect_err("there are no children");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");
    }

    /// An artifact manifest requires nothing, and what it names are
    /// blobs.
    #[test]
    fn an_artifact_manifest_requires_nothing_and_names_blobs() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "artifactType": "application/vnd.example.sbom.v1",
            "blobs": [{ "digest": A_DIGEST }],
        });
        let required =
            the_required_digests(&manifest, ManifestKind::Artifact).expect("the digests");
        assert_eq!(
            required,
            vec![(the_hexadecimal(A_DIGEST).to_string(), TheClass::Blob)]
        );

        // Without blobs or config, an artifact requires nothing.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "artifactType": "application/vnd.example.sbom.v1",
        });
        let required =
            the_required_digests(&manifest, ManifestKind::Artifact).expect("the digests");
        assert!(required.is_empty());
    }

    /// The `subject` is read, and it is **not** required to exist: the protocol
    /// says a manifest whose `subject` is not yet there is accepted.
    #[test]
    fn the_subject_is_read_and_not_required_to_exist() {
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "artifactType": "application/vnd.example.sbom.v1",
            "subject": { "digest": A_DIGEST },
        });
        let subject = the_subject_of(&manifest).expect("the subject");
        assert_eq!(
            subject.map(|digest| digest.value),
            Some(the_hexadecimal(A_DIGEST).to_string())
        );

        // Without a subject, there is no subject.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
        });
        assert!(the_subject_of(&manifest).expect("no subject").is_none());

        // And a subject that is **not** a digest is not a subject: a
        // half-written digest would be stored in the catalog and would leave
        // the manifest published and unreadable.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "subject": { "digest": "sha256:aaa" },
        });
        let error = the_subject_of(&manifest).expect_err("the subject is not a digest");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");

        // A subject without a digest is not a subject.
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.artifact.manifest.v1+json",
            "subject": { "size": 12 },
        });
        let error = the_subject_of(&manifest).expect_err("there is no digest");
        assert_eq!(error.the_code(), "MANIFEST_INVALID");
    }
}
