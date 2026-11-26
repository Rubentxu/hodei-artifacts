//! The content store on top of an Object Store that speaks S3.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use aws_sdk_s3::Client;
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_hub::blob::{
    BlobError, BlobStore, BlobWriteSession, CommitExpectation, ReadPolicy, StagingCleanup,
};
use hodei_hub::reclaim::{
    BlobReclaim, ConditionalBlob, ConditionalOutcome, Reclaimed, StoredBlob, VersionMarker,
};
use sha2::{Digest, Sha256};

/// The prefix under which content lives inside the bucket.
const BLOB_PREFIX: &str = "blobs";

/// The prefix under which staging lives: in-progress multipart uploads and
/// the leftovers that a failure left between the complete and the copy.
const STAGING_PREFIX: &str = "staging";

/// The key under which the store stamps the object's digest. The ETag
/// is no use (in a multipart it is not the body's sha256), and a publication
/// that cannot be told apart from a foreign leftover forces a re-read to deduplicate.
/// The stamp is an aid to avoid rewriting what is already right; what
/// rules, when a serious decision must be made, are the bytes.
const DIGEST_METADATA: &str = "hodei-sha256";

/// How far it is worth re-reading a pre-existing object without a stamp to
/// check whether it is the content: above this, reading it whole
/// costs more than the copy being avoided, and it is repaired by writing.
const MAX_VERIFY_ON_STAMP_MISS: u64 = 64 * 1024 * 1024;

/// What the store constructor declared wrongly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3BlobConfigError {
    /// The part size is below the minimum that S3 accepts
    /// for every part that is not the last: five megabytes.
    PartTooSmall { given: usize, minimum: usize },
    /// There cannot be zero simultaneous in-flight parts.
    NoInflightParts,
}

/// How this store behaves.
#[derive(Clone)]
pub struct S3BlobConfig {
    /// The bucket where the content lives.
    pub bucket: String,
    /// The part size of the multipart upload. In-flight memory
    /// per session is bounded by `part_size * max_inflight_parts`.
    pub part_size: usize,
    /// How many parts a session allows in flight before waiting.
    pub max_inflight_parts: usize,
}

impl S3BlobConfig {
    /// The configuration with the deployment's default values.
    pub fn new(bucket: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            part_size: 8 * 1024 * 1024,
            max_inflight_parts: 4,
        }
    }

    /// The memory ceiling per in-progress session, in bytes.
    pub fn the_memory_budget(&self) -> usize {
        self.part_size * self.max_inflight_parts
    }

    fn check(&self) -> Result<(), S3BlobConfigError> {
        const MINIMUM_PART: usize = 5 * 1024 * 1024;
        if self.part_size < MINIMUM_PART {
            return Err(S3BlobConfigError::PartTooSmall {
                given: self.part_size,
                minimum: MINIMUM_PART,
            });
        }
        if self.max_inflight_parts == 0 {
            return Err(S3BlobConfigError::NoInflightParts);
        }
        Ok(())
    }
}

/// The content store on an S3-compatible Object Store.
///
/// The client arrives built from outside, with the credential chain that
/// the deployment chose: the store sees no secrets and therefore cannot
/// leak them. `Debug` prints the public configuration and the bucket, never
/// the client.
pub struct S3BlobStore {
    client: Client,
    config: S3BlobConfig,
    /// The barrier between publishing a blob and collecting it, the same one
    /// guaranteed in-process by the file store.
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl std::fmt::Debug for S3BlobStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("S3BlobStore")
            .field("bucket", &self.config.bucket)
            .field("part_size", &self.config.part_size)
            .field("max_inflight_parts", &self.config.max_inflight_parts)
            .finish_non_exhaustive()
    }
}

impl S3BlobStore {
    /// The store over an already configured client.
    pub fn new(client: Client, config: S3BlobConfig) -> Result<Self, S3BlobConfigError> {
        config.check()?;
        Ok(Self {
            client,
            config,
            gate: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// The key of a content item.
    pub fn the_key_of(&self, digest: &ContentDigest) -> String {
        blob_key(digest)
    }

    /// The digest that a key states, if the key belongs to a blob of this
    /// store. Anything else is neither listed nor deleted.
    fn digest_of_key(key: &str) -> Option<ContentDigest> {
        let name = key
            .strip_prefix(&format!("{BLOB_PREFIX}/"))?
            .rsplit('/')
            .next()?;
        let valid = name.len() == 64
            && name
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            return None;
        }
        Some(ContentDigest::sha256(name))
    }

    /// The base64 MD5 of a part, the integrity condition that S3
    /// checks on the server before accepting the part as good.
    fn content_md5(chunk: &[u8]) -> String {
        use base64::Engine;
        use md5::Digest;
        base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(chunk).as_slice())
    }

    /// S3's last-modified, as age from now.
    fn age_of(last_modified: Option<&aws_smithy_types::DateTime>) -> Option<Duration> {
        let epoch = std::time::UNIX_EPOCH.elapsed().ok()?;
        let then = Duration::from_secs(last_modified?.secs().max(0) as u64);
        Some(epoch.saturating_sub(then))
    }

    /// The service error to deal with: what is missing is
    /// `NotFound`; everything else means the store was not available.
    fn unavailable(operation: &str, detail: impl std::fmt::Display) -> BlobError {
        BlobError::Unavailable(format!("the object store failed to {operation}: {detail}"))
    }

    /// The raw header of a key, with `None` if it is not there. It is the HEAD
    /// that the no-overwrite decision needs, and it distinguishes "it is not
    /// there" from "I cannot look" (the latter is not disguised as the former).
    async fn head_key(
        &self,
        key: &str,
    ) -> Result<Option<aws_sdk_s3::operation::head_object::HeadObjectOutput>, BlobError> {
        match self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(head) => Ok(Some(head)),
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(Self::unavailable("look up the content", error)),
        }
    }

    /// What to do when a PUT or a conditional copy refuses to write
    /// because the key already exists. This is the point where the CAS stakes its
    /// word: what is there may be exactly the content (a
    /// legitimate deduplication, it is not touched) or it may be something else (a
    /// truncated leftover, an object planted from outside, corruption). The
    /// former is reported; the latter **is not presented as dedupe**:
    /// it is repaired by writing the good content through the normal path.
    ///
    /// The check starts with the metadata stamp (cheap, one
    /// HEAD), and only if the pre-existing object carries no stamp (it was written by
    /// another hand and its provenance is unknown) are the bytes read, with the
    /// ceiling of `MAX_VERIFY_ON_STAMP_MISS`: above it, re-reading costs
    /// more than rewriting, and repair by writing comes free
    /// whatever was there. A pre-existing object verified as
    /// good and without a stamp is renewed with a stamp: the next publication
    /// deduplicates by HEAD, not by reading.
    ///
    /// The trust precondition of the stamp must be spelled out in
    /// full: **the stamp is valid because it is stamped by whoever verified
    /// before writing**, and only the stores of this crate guarantee that.
    /// A bucket shared with hands that write metadata
    /// at will (another service, an `aws s3 cp` with `--metadata`, an
    /// operator with access) can plant a lying stamp and this
    /// decision would accept it without re-reading. The supported topology is the
    /// exclusive bucket of this service (the one Hodei deploys); if
    /// one day a multi-tenant backend enters the contract, the stamp
    /// becomes a suspect optimization and this requires reading
    /// always (the deployer decides that, not a refactor).
    async fn settle_a_refused_write(
        &self,
        digest: &ContentDigest,
        bytes: Option<&[u8]>,
        repair: impl std::future::Future<Output = Result<(), BlobError>> + Send,
    ) -> Result<(), BlobError> {
        let key = self.the_key_of(digest);
        let Some(head) = self.head_key(&key).await? else {
            // The pre-existing object went away between the rejection and the HEAD: the
            // concurrent window. Repairing (writing) is the right
            // path (nobody has published the content yet).
            return repair.await;
        };
        match the_stamp_of(&head) {
            // With its own stamp, the identity of the content is guaranteed by
            // whoever wrote it, verifying before stamping.
            Some(stamped) if stamped == *digest => return Ok(()),
            Some(_) => {
                // It carries the stamp of ANOTHER digest under this one's key: the
                // key and the content disagree. It is repaired by writing
                // what was sent; whoever calls and wants the integrity
                // verdict should ask afterwards with `verify`.
                return repair.await;
            }
            None => {}
        }
        // No stamp: it is not known to be what the key says.
        let length = head.content_length.unwrap_or(0).max(0) as u64;
        let cheap_to_check = length <= MAX_VERIFY_ON_STAMP_MISS;
        if let Some(expected) = bytes.filter(|_| cheap_to_check) {
            if self.read(digest, None).await?.as_slice() == expected {
                // It is the content. It is stamped without moving a byte: the
                // copy onto itself with the REPLACE metadata directive
                // is the only rewrite of the object that does not
                // need to upload the body again.
                self.client
                    .copy_object()
                    .bucket(&self.config.bucket)
                    .key(&key)
                    .copy_source(format!("{}/{}", self.config.bucket, key))
                    .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace)
                    .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
                    .send()
                    .await
                    .map_err(|error| Self::unavailable("stamp the verified blob", error))?;
                return Ok(());
            }
            return repair.await;
        }
        // Either we do not have the bytes (the session path) or the object is
        // large: verifying is more expensive than repairing, and repair is
        // safe (the CAS authority is the digest of what is sent).
        repair.await
    }

    /// The metadata map that stamps an object with its digest.
    fn the_stamp_map(digest: &ContentDigest) -> HashMap<String, String> {
        let mut items = HashMap::new();
        items.insert(DIGEST_METADATA.to_string(), digest.value.clone());
        items
    }

    /// A PUT with the condition that the backend offers. With `only_if_absent`
    /// the server rejects (412) if the key already exists (the conditional
    /// write measured in AC4); without it, it is the usual normal PUT.
    /// In both forms the object goes out stamped with its digest: the
    /// no-overwrite condition is only worth anything if afterwards there is something to prove that
    /// what is there is what was published.
    ///
    /// The error travels boxed: the PUT's `SdkError` weighs hundreds of
    /// bytes, and this function is called on the hot path of every
    /// publication (the wide Result would cost more to move than the error
    /// it describes). Whoever only looks at the code (`is_precondition_failed`)
    /// does not even notice.
    async fn conditional_put(
        &self,
        digest: &ContentDigest,
        bytes: &[u8],
        only_if_absent: bool,
    ) -> Result<
        (),
        Box<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::put_object::PutObjectError>>,
    > {
        let mut request = self
            .client
            .put_object()
            .bucket(&self.config.bucket)
            .key(self.the_key_of(digest))
            .content_md5(Self::content_md5(bytes))
            .set_metadata(Some(Self::the_stamp_map(digest)))
            .body(ByteStream::from(bytes.to_vec()));
        if only_if_absent {
            request = request.if_none_match("*");
        }
        request.send().await.map(|_| ()).map_err(Box::new)
    }

    /// The staging reaper: the leftovers that a failure or a killed
    /// process left between the `create` and the publication, in the S3
    /// bucket. It is the equivalent of the file store's `clean_expired_staging`,
    /// and keeps the same promise: **it does not touch a live upload**.
    ///
    /// Here the liveness test is stronger than the file lock:
    /// a session abandoned without aborting left its `upload_id` in the
    /// store, and what distinguishes it from an in-progress upload is that
    /// nobody writes to it. The date of the last uploaded part is requested
    /// (and that of the `create` itself if there are no parts), and only what carries
    /// more than `max_age` without a new byte is a leftover: it is aborted. The
    /// abort is the only deletion (a half-done multipart is not an
    /// object, and its parts are not listed as such; there is nothing
    /// that `list_blobs` can see and the collector can reclaim).
    ///
    /// An isolated failure while aborting is **not** a failure of the reaping: it is
    /// counted and the pass continues, as in the directory, because what
    /// could not be deleted today is retried at tomorrow's
    /// startup. And an `upload_id` without a usable date is not touched either: without a
    /// clock there is no age of majority, and deleting blindly is worse than
    /// leaving it.
    pub async fn clean_staging(&self, max_age: Duration) -> Result<StagingCleanup, BlobError> {
        let mut cleanup = StagingCleanup::default();
        let mut key_marker: Option<String> = None;
        let mut upload_marker: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_multipart_uploads()
                .bucket(&self.config.bucket)
                .prefix(format!("{STAGING_PREFIX}/"))
                .max_uploads(1000);
            if let Some(marker) = &key_marker {
                request = request.key_marker(marker);
            }
            if let Some(marker) = &upload_marker {
                request = request.upload_id_marker(marker);
            }
            let page = request
                .send()
                .await
                .map_err(|error| Self::unavailable("list the staging uploads", error))?;
            for upload in page.uploads() {
                let (Some(key), Some(upload_id)) = (upload.key(), upload.upload_id()) else {
                    continue;
                };
                // The key must belong to this store: `staging/<pid>-…`.
                // Anything else under the prefix is not ours and
                // is not touched (the same rule as `list_blobs` with keys).
                if !is_a_staging_key(key) {
                    continue;
                }
                // The age may not be obtainable (a server 500,
                // an upload that closes just while we look at it).
                // The reaping's promise is that an isolated failure does **not**
                // abort the pass: it is counted, like the abort that fails,
                // and the next upload is processed. Losing an old
                // upload today is an inconvenience; not reaping anything because a
                // `ListParts` had a bad day would be disobeying the
                // contract of this function.
                let age = match self
                    .the_age_of_the_upload(key, upload_id, upload.initiated())
                    .await
                {
                    Ok(age) => age,
                    Err(error) => {
                        cleanup.failed.push((key.to_string(), error.to_string()));
                        continue;
                    }
                };
                let Some(age) = age else { continue };
                if age < max_age {
                    // Too young to be a leftover: someone may be
                    // writing to it right now.
                    cleanup.alive += 1;
                    continue;
                }
                match self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.config.bucket)
                    .key(key)
                    .upload_id(upload_id)
                    .send()
                    .await
                {
                    Ok(_) => cleanup.removed += 1,
                    // The upload went away between the listing and the abort:
                    // someone (the owner, or the Drop of its session) closed
                    // it. It is not a failure, and it is not counted.
                    Err(error) if is_not_found(&error) => {}
                    Err(error) => cleanup.failed.push((key.to_string(), error.to_string())),
                }
            }
            if page.is_truncated() != Some(true) {
                break;
            }
            key_marker = page.next_key_marker.map(|value| value.to_string());
            upload_marker = page.next_upload_id_marker.map(|value| value.to_string());
            if key_marker.is_none() {
                break;
            }
        }
        // The staging objects that remained between the complete and the
        // copy of an interrupted publication: a normal object under
        // the prefix, already without upload_id, that nobody lists as a blob and that
        // nobody will ever read. Its date is `Last-Modified`.
        let mut token: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.config.bucket)
                .prefix(format!("{STAGING_PREFIX}/"))
                .max_keys(1000);
            if let Some(token) = &token {
                request = request.continuation_token(token.clone());
            }
            let page = request
                .send()
                .await
                .map_err(|error| Self::unavailable("list the staging objects", error))?;
            for object in page.contents() {
                let (Some(key), Some(age)) = (object.key(), Self::age_of(object.last_modified()))
                else {
                    continue;
                };
                if !is_a_staging_key(key) || age < max_age {
                    continue;
                }
                match self
                    .client
                    .delete_object()
                    .bucket(&self.config.bucket)
                    .key(key)
                    .send()
                    .await
                {
                    Ok(_) => cleanup.removed += 1,
                    Err(error) if is_not_found(&error) => {}
                    Err(error) => cleanup.failed.push((key.to_string(), error.to_string())),
                }
            }
            if page.is_truncated() != Some(true) {
                break;
            }
            token = page.next_continuation_token.map(|value| value.to_string());
            if token.is_none() {
                break;
            }
        }
        Ok(cleanup)
    }

    /// The age of an upload: the most recent between its creation and its
    /// last part. A live session updates the clock with every
    /// `upload_part`, and therefore an upload that is hours old from `create` but
    /// minutes old from its last part is not a leftover.
    async fn the_age_of_the_upload(
        &self,
        key: &str,
        upload_id: &str,
        initiated: Option<&aws_smithy_types::DateTime>,
    ) -> Result<Option<Duration>, BlobError> {
        let mut ages: Vec<Duration> = Self::age_of(initiated).into_iter().collect();
        let mut part_marker: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_parts()
                .bucket(&self.config.bucket)
                .key(key)
                .upload_id(upload_id)
                .max_parts(1000);
            if let Some(marker) = &part_marker {
                request = request.part_number_marker(marker.clone());
            }
            let page = match request.send().await {
                Ok(page) => page,
                // The upload closed between the listing and the query:
                // it no longer exists, it is not a leftover to reap.
                Err(error) if is_not_found(&error) => return Ok(Some(Duration::ZERO)),
                Err(error) => {
                    return Err(Self::unavailable(
                        "list the parts of a staging upload",
                        error,
                    ));
                }
            };
            ages.extend(
                page.parts()
                    .iter()
                    .filter_map(|part| Self::age_of(part.last_modified())),
            );
            if page.is_truncated() != Some(true) {
                break;
            }
            part_marker = page.next_part_number_marker.map(|value| value.to_string());
            if part_marker.is_none() {
                break;
            }
        }
        // Without any usable date, no decision is made: unknown age is not
        // age of majority.
        Ok(ages.into_iter().min())
    }
}

/// Whether the service error says the content is not there.
fn is_not_found(error: &aws_sdk_s3::error::SdkError<impl ProvideErrorMetadata>) -> bool {
    matches!(
        ProvideErrorMetadata::code(error),
        Some("NoSuchKey") | Some("NotFound")
    )
}

/// Whether the service error is the no-overwrite precondition: the
/// object was already there when the conditional PUT tried to create it.
fn is_precondition_failed(error: &aws_sdk_s3::error::SdkError<impl ProvideErrorMetadata>) -> bool {
    matches!(
        ProvideErrorMetadata::code(error),
        Some("PreconditionFailed")
    )
}

/// Whether the key is an in-progress write of this store.
///
/// The pattern is set by `new_staging_key`: `staging/<pid>-<nanos:x>-<counter:x>`,
/// the same as the file store's without the `.part`. A key with another
/// shape under the staging prefix does not belong to this store: it is not listed,
/// not aborted and not counted (it could be another application's staging
/// sharing the bucket, and deleting what belongs to others is not part of the contract).
///
/// The discriminator is formal, not ownership-based: another application that
/// chose the same pattern by chance would see its old uploads
/// aborted. It is the same precondition as the digest stamp (the bucket is
/// this service's), and with pid and nanos a real tie requires an identical clock and
/// process number on two machines against the same bucket.
fn is_a_staging_key(key: &str) -> bool {
    let Some(stem) = key.strip_prefix(&format!("{STAGING_PREFIX}/")) else {
        return false;
    };
    let mut parts = stem.split('-');
    let (Some(pid), Some(nanos), Some(counter), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !nanos.is_empty()
        && nanos.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !counter.is_empty()
        && counter.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The digest stamp that this store stamps on each object, or `None`
/// if the object carries no stamp (a pre-existing one from another hand, a leftover
/// that was not yet written conditionally).
fn the_stamp_of(
    head: &aws_sdk_s3::operation::head_object::HeadObjectOutput,
) -> Option<ContentDigest> {
    let name = head
        .metadata()
        .and_then(|items| items.get(DIGEST_METADATA))?
        .to_ascii_lowercase();
    if name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Some(ContentDigest::sha256(name))
    } else {
        None
    }
}

#[async_trait::async_trait]
impl BlobStore for S3BlobStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        // In a CAS the key is the digest of the content being sent, so
        // publishing the same content twice is not an error and does not
        // store twice. What the conditional write adds over
        // the plain one is the guarantee that the object that was already there **is**
        // the content: the PUT only goes in if the key is free, and if the
        // server rejects it, `settle_a_refused_write` checks the
        // pre-existing object instead of trusting a HEAD. What does not add up
        // is repaired, never deduplicated.
        match self.conditional_put(&digest, bytes, true).await {
            Ok(()) => Ok(digest),
            Err(error) if is_precondition_failed(&error) => {
                // The `bytes.to_vec()` is only paid on the
                // rejection path, which in a healthy CAS is the exception (a
                // pre-existing object without a stamp or corrupt), not the dedupe.
                let owned = bytes.to_vec();
                let repair = {
                    let digest = digest.clone();
                    async move {
                        self.conditional_put(&digest, &owned, false)
                            .await
                            .map_err(|error| Self::unavailable("repair the content", error))
                    }
                };
                self.settle_a_refused_write(&digest, Some(bytes), repair)
                    .await?;
                Ok(digest)
            }
            Err(error) => Err(Self::unavailable("store the content", error)),
        }
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await
    }

    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        match self.head(digest).await {
            Ok(_) => Ok(true),
            Err(BlobError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        // The in-progress write is born as a multipart upload on a staging
        // key, outside the blob space: nothing uploaded here
        // is visible until the commit.
        let staging_key = new_staging_key();
        let created = self
            .client
            .create_multipart_upload()
            .bucket(&self.config.bucket)
            .key(&staging_key)
            .send()
            .await
            .map_err(|error| Self::unavailable("open the staging upload", error))?;
        let upload_id = created
            .upload_id()
            .map(|id| id.to_string())
            .unwrap_or_default();
        if upload_id.is_empty() {
            return Err(Self::unavailable(
                "open the staging upload",
                "the object store gave no upload id",
            ));
        }
        Ok(Box::new(S3BlobWriteSession {
            client: self.client.clone(),
            config: self.config.clone(),
            gate: Arc::clone(&self.gate),
            staging_key,
            upload_id,
            buffer: Vec::new(),
            pending: VecDeque::new(),
            completed: Vec::new(),
            part_number: 0,
            hasher: Sha256::new(),
            written: 0,
            open: true,
            durable: false,
            covers_all: true,
        }))
    }

    /// Reopen an in-progress multipart upload from the journal's
    /// handle: `staging/<pid>-<nanos>-<counter>` and its `upload_id`.
    ///
    /// The store is the authority on what is there: the parts are listed and
    /// the **prefix** is taken (ordered by number) whose sum is
    /// exactly the offset that the journal declares. A multipart with
    /// more bytes than declared is not an error as long as that prefix
    /// exists: the parts that the authority did not record are those of a
    /// process that died between the upload and the advance, they stay out of
    /// the complete's list and are never published; the bucket's lifecycle
    /// collects them. If no prefix sums to what was declared (overlapping
    /// bytes from two writers), the resume fails and the upload is
    /// redone: publishing ahead of the authority is forbidden.
    async fn resume(
        &self,
        handle: &str,
        written: u64,
    ) -> Result<Box<dyn hodei_hub::blob::BlobWriteSession>, BlobError> {
        let (key, upload_id) = handle.split_once('\u{0}').ok_or_else(|| {
            BlobError::Unavailable("the session handle is not a multipart".to_owned())
        })?;
        // Only own staging: a foreign key under the prefix is not an
        // upload that this store resumes, and touching it is not part of the
        // contract.
        if !is_a_staging_key(key) {
            return Err(BlobError::Unavailable(
                "the session handle is not a staging upload of this store".to_owned(),
            ));
        }
        let mut listed: Vec<(i32, u64, String)> = Vec::new();
        let mut marker: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_parts()
                .bucket(&self.config.bucket)
                .key(key)
                .upload_id(upload_id)
                .max_parts(1000);
            if let Some(marker) = &marker {
                request = request.part_number_marker(marker.clone());
            }
            let page = match request.send().await {
                Ok(page) => page,
                Err(error) if is_not_found(&error) => {
                    return Err(BlobError::Unavailable(
                        "the staging upload does not exist".to_owned(),
                    ));
                }
                Err(error) => {
                    return Err(Self::unavailable("list the parts of the session", error));
                }
            };
            for part in page.parts() {
                let number = part.part_number().unwrap_or(0);
                let size = part.size().unwrap_or(0) as u64;
                let e_tag = part.e_tag().unwrap_or_default().to_owned();
                listed.push((number, size, e_tag));
            }
            if page.is_truncated() != Some(true) {
                break;
            }
            marker = page.next_part_number_marker.map(|value| value.to_string());
            if marker.is_none() {
                break;
            }
        }
        listed.sort_by_key(|(number, _size, _tag)| *number);
        let mut parts: Vec<CompletedPart> = Vec::new();
        let mut highest = 0i32;
        let mut total = 0u64;
        for (number, size, e_tag) in &listed {
            if total + size > written {
                break;
            }
            total += size;
            highest = *number;
            parts.push(
                CompletedPart::builder()
                    .part_number(*number)
                    .e_tag(e_tag)
                    .build(),
            );
        }
        if total != written {
            return Err(BlobError::Unavailable(format!(
                "the staging upload does not hold a prefix of exactly {written} bytes and the journal declared {written}"
            )));
        }
        Ok(Box::new(S3BlobWriteSession {
            client: self.client.clone(),
            config: self.config.clone(),
            gate: Arc::clone(&self.gate),
            staging_key: key.to_owned(),
            upload_id: upload_id.to_owned(),
            buffer: Vec::new(),
            pending: VecDeque::new(),
            completed: parts,
            part_number: highest,
            hasher: Sha256::new(),
            written,
            open: true,
            // The journal is the staging authority; the local hasher did not
            // see the parts that were already there.
            durable: true,
            covers_all: written == 0,
        }))
    }

    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        let bytes = self.read(digest, None).await?;
        if policy == ReadPolicy::Verify {
            let found = ContentDigest::of_bytes(&bytes);
            if found != *digest {
                return Err(BlobError::Corrupted {
                    expected: digest.clone(),
                    found,
                });
            }
        }
        Ok(bytes)
    }

    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        let head = self.head(digest).await?;
        Ok(head.unwrap_or(0))
    }

    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        // The total length is queried before requesting the range: S3
        // happily returns a truncated range, and a client that asks for
        // 100 bytes and receives 40 cannot know that 60 are missing. The
        // port contract does not allow that treatment.
        let size = self.length(digest).await?;
        if length == 0 {
            return Ok(Vec::new());
        }
        if offset > size {
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: 0,
            });
        }
        let available = size - offset;
        if length > available {
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: available,
            });
        }
        let range = format!("bytes={}-{}", offset, offset + length - 1);
        let bytes = self.read(digest, Some(&range)).await?;
        if bytes.len() as u64 != length {
            // The object changed size between the HEAD and the GET: the requested
            // range is no longer what is there. It says how much there is, it does not
            // return a half range.
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: bytes.len() as u64,
            });
        }
        Ok(bytes)
    }

    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await.map(|_| ())
    }

    fn reclaimer(&self) -> Option<&dyn hodei_hub::reclaim::BlobReclaim> {
        Some(self)
    }

    fn conditional(&self) -> Option<&dyn hodei_hub::reclaim::ConditionalBlob> {
        Some(self)
    }
}

/// Whether the service error is the `If-Match` precondition (412): the
/// current version is not the one the collector marked.
fn is_conditional_mismatch(error: &aws_sdk_s3::error::SdkError<impl ProvideErrorMetadata>) -> bool {
    match ProvideErrorMetadata::code(error) {
        Some("PreconditionFailed") => true,
        // S3 also returns 412 as a pure `If-Match` depending on the endpoint.
        Some(status) => status.contains("Precondition"),
        None => false,
    }
}

#[async_trait::async_trait]
impl ConditionalBlob for S3BlobStore {
    fn supports_conditional_delete(&self) -> bool {
        // The latest S3 generation (and the S3-compatible endpoints
        // that implement it) accepts `If-Match` on DELETE and
        // `x-amz-copy-source-if-match` on COPY. The collector asks
        // here, in the store's type; not in a flag that can be
        // set wrongly. An endpoint that does not support it is detected at the
        // first failed condition: the copy returns 412 and touches
        // nothing, which is exactly the fail-closed contract.
        true
    }

    fn quarantine_key_of(&self, digest: &ContentDigest, run_id: &str) -> String {
        format!("quarantine/{run_id}/{}", digest.value)
    }

    async fn head_marker(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<VersionMarker>, BlobError> {
        let key = self.the_key_of(digest);
        match self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(head) => {
                let Some(marker) = head.e_tag().map(|etag| etag.to_string()) else {
                    return Err(Self::unavailable(
                        "look up the blob version",
                        "the object store served no ETag to condition on",
                    ));
                };
                Ok(Some(VersionMarker {
                    key,
                    marker,
                    length: head.content_length.unwrap_or(0).max(0) as u64,
                }))
            }
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(Self::unavailable("look up the blob version", error)),
        }
    }

    async fn delete_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
    ) -> Result<ConditionalOutcome, BlobError> {
        let key = self.the_key_of(digest);
        match self
            .client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .set_if_match(Some(marker.marker.clone()))
            .send()
            .await
        {
            Ok(_) => Ok(ConditionalOutcome::Applied {
                length: marker.length,
            }),
            Err(error) if is_not_found(&error) => Ok(ConditionalOutcome::Gone),
            Err(error) if is_conditional_mismatch(&error) => Ok(ConditionalOutcome::NotApplied),
            Err(error) => Err(Self::unavailable("delete the blob conditionally", error)),
        }
    }

    async fn quarantine_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
        quarantine_key: &str,
    ) -> Result<ConditionalOutcome, BlobError> {
        if marker.marker.is_empty() {
            return Err(Self::unavailable(
                "quarantine the blob version",
                "the marked version has no ETag to condition on",
            ));
        }
        let source = self.the_key_of(digest);
        // Step 1: COPY conditional on the marked version. A 412 here
        // means that the original key is NOT what the pass saw:
        // nothing is touched. A 404 means it is no longer there.
        let copied = self
            .client
            .copy_object()
            .bucket(&self.config.bucket)
            .copy_source(format!("{}/{}", self.config.bucket, source))
            .set_copy_source_if_match(Some(marker.marker.clone()))
            .key(quarantine_key)
            .send()
            .await;
        match copied {
            Ok(_) => {}
            Err(error) if is_conditional_mismatch(&error) => {
                return Ok(ConditionalOutcome::NotApplied);
            }
            Err(error) if is_not_found(&error) => {
                return Ok(ConditionalOutcome::Gone);
            }
            Err(error) => {
                return Err(Self::unavailable("quarantine the marked version", error));
            }
        }
        // Step 2: DELETE conditional on the same ETag. If between the copy
        // and the delete someone rewrote the key, the delete fails with
        // 412 and the quarantine remains as an exact copy of the marked
        // version: reconcilable, never lost.
        match self
            .client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(&source)
            .set_if_match(Some(marker.marker.clone()))
            .send()
            .await
        {
            Ok(_) => Ok(ConditionalOutcome::Applied {
                length: marker.length,
            }),
            Err(error) if is_not_found(&error) => Ok(ConditionalOutcome::Gone),
            Err(error) if is_conditional_mismatch(&error) => Ok(ConditionalOutcome::NotApplied),
            Err(error) => Err(Self::unavailable("release the quarantined key", error)),
        }
    }
}

impl S3BlobStore {
    /// The blob header: length and date, or `NotFound`.
    async fn head(&self, digest: &ContentDigest) -> Result<Option<u64>, BlobError> {
        match self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(self.the_key_of(digest))
            .send()
            .await
        {
            Ok(head) => Ok(head.content_length.map(|l| l.max(0) as u64)),
            Err(error) if is_not_found(&error) => Err(BlobError::NotFound),
            Err(error) => Err(Self::unavailable("look up the content", error)),
        }
    }

    /// Read the content, whole or by range.
    async fn read(
        &self,
        digest: &ContentDigest,
        range: Option<&str>,
    ) -> Result<Vec<u8>, BlobError> {
        let mut request = self
            .client
            .get_object()
            .bucket(&self.config.bucket)
            .key(self.the_key_of(digest));
        if let Some(range) = range {
            request = request.range(range);
        }
        let got = match request.send().await {
            Ok(got) => got,
            Err(error) if is_not_found(&error) => return Err(BlobError::NotFound),
            Err(error) => return Err(Self::unavailable("read the content", error)),
        };
        let body = got
            .body
            .collect()
            .await
            .map_err(|error| Self::unavailable("stream the content", error))?;
        Ok(body.into_bytes().to_vec())
    }
}

/// The key of a content item inside the blob prefix.
fn blob_key(digest: &ContentDigest) -> String {
    let hex = &digest.value;
    format!("{BLOB_PREFIX}/{}/{}/{}", &hex[..2], &hex[2..4], hex)
}

/// The key of an in-progress write: it lives in the staging prefix,
/// outside the blob space, until the commit publishes it.
fn new_staging_key() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!("staging/{}-{nanos:x}-{counter:x}", std::process::id())
}

/// The result of uploading a part: its number and the etag that the
/// store returned, with which the commit claims it.
type UploadedPart = Result<CompletedPart, BlobError>;

/// An in-progress write over a multipart upload.
///
/// The bytes accumulate in a buffer up to `part_size`; when it fills, the
/// part flies to the store **with its Content-MD5**, so that the
/// integrity condition is checked by the server and not by us at the
/// end. There cannot be more than `max_inflight_parts` parts in flight: that
/// is the session's memory budget, and exceeding it means waiting, not
/// accumulating.
///
/// The commit **completes nothing that does not add up**: it waits for the parts in
/// flight, checks length and digest against what was declared and, if anything
/// differs, aborts the whole upload. An abort deletes the store's staging:
/// no visible reference remains, not even a half object.
pub struct S3BlobWriteSession {
    client: Client,
    config: S3BlobConfig,
    gate: Arc<tokio::sync::Mutex<()>>,
    staging_key: String,
    upload_id: String,
    buffer: Vec<u8>,
    pending: VecDeque<tokio::task::JoinHandle<UploadedPart>>,
    completed: Vec<CompletedPart>,
    part_number: i32,
    hasher: Sha256,
    written: u64,
    open: bool,
    /// The registry's journal is what decides the multipart's destination:
    /// the handle's Drop does **not** abort it.
    durable: bool,
    /// Whether the local hasher covers the whole staging. A multipart reopened
    /// cold does not see again the parts that already flew: its local digest
    /// is partial, and the commit has to check the bytes against the
    /// store, not against the hasher.
    covers_all: bool,
}

impl S3BlobWriteSession {
    /// Launches the complete part that fills the buffer, waiting if there are already
    /// too many in flight.
    async fn flush_part(&mut self, last: bool) -> Result<(), BlobError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        if !last && self.buffer.len() < self.config.part_size {
            return Ok(());
        }
        // The memory budget is enforced by waiting, not by accumulating.
        while self.pending.len() >= self.config.max_inflight_parts {
            let oldest = self
                .pending
                .pop_front()
                .expect("habia trozos en vuelo para esperar");
            self.collect_part(tokio::time::timeout(UPLOAD_TIMEOUT, oldest).await)?;
        }
        let bytes = std::mem::take(&mut self.buffer);
        self.part_number += 1;
        let number = self.part_number;
        let client = self.client.clone();
        let bucket = self.config.bucket.clone();
        let key = self.staging_key.clone();
        let upload_id = self.upload_id.clone();
        let handle = tokio::spawn(async move {
            let md5 = S3BlobStore::content_md5(&bytes);
            let sent = client
                .upload_part()
                .bucket(&bucket)
                .key(&key)
                .upload_id(&upload_id)
                .part_number(number)
                .content_md5(md5)
                .body(ByteStream::from(bytes))
                .send()
                .await
                .map_err(|error| S3BlobStore::unavailable("upload a part", error))?;
            Ok(CompletedPart::builder()
                .part_number(number)
                .set_e_tag(sent.e_tag().map(|etag| etag.to_string()))
                .build())
        });
        self.pending.push_back(handle);
        Ok(())
    }

    /// Collects a finished part and records it for the commit.
    fn collect_part(
        &mut self,
        joined: Result<Result<UploadedPart, tokio::task::JoinError>, tokio::time::error::Elapsed>,
    ) -> Result<(), BlobError> {
        let part = match joined {
            Ok(Ok(part)) => part?,
            Ok(Err(joined)) => {
                return Err(S3BlobStore::unavailable("join the part upload", joined));
            }
            Err(_elapsed) => {
                return Err(S3BlobStore::unavailable(
                    "upload a part",
                    "the object store did not answer within the timeout",
                ));
            }
        };
        self.completed.push(part);
        Ok(())
    }

    /// Discards the in-progress write: aborts the upload and publishes nothing.
    async fn discard(&mut self) {
        self.open = false;
        self.buffer.clear();
        for handle in self.pending.drain(..) {
            handle.abort();
        }
        let client = self.client.clone();
        let bucket = self.config.bucket.clone();
        let key = self.staging_key.clone();
        let upload_id = self.upload_id.clone();
        let _ = tokio::spawn(async move {
            client
                .abort_multipart_upload()
                .bucket(&bucket)
                .key(&key)
                .upload_id(&upload_id)
                .send()
                .await
        })
        .await;
    }

    /// Publishes the validated content under its digest key.
    ///
    /// S3 completes the multipart upload on the key where it was born (the
    /// staging one), so publishing is two atomic server-side steps:
    /// complete, and copy the result to the digest key. The copy
    /// is server-side: the bytes do not pass through the process again. And the
    /// staging is deleted right afterwards: what was not deleted is expired by the
    /// bucket lifecycle, never by the blob collector.
    ///
    /// The copy goes with the no-overwrite condition that the backend
    /// offers (measured, HSP-139: `If-None-Match: *` on the destination in
    /// RustFS and LocalStack; if a backend does not support it, the backend
    /// degrades to the copy without condition: deduplicating by content remains
    /// true, not overwriting does not). The 412 is not a failure: two
    /// concurrent sessions of the same content is exactly the case
    /// that the condition resolves, and the loser checks the winner with
    /// the same honest criterion as the `put` path.
    async fn publish(&self, target: &str, digest: &ContentDigest) -> Result<(), BlobError> {
        if self.completed.is_empty() {
            // Not a single byte: there is nothing to complete, and S3 does not allow
            // empty multipart uploads. Empty content is published
            // with a normal PUT, conditional like the rest.
            let answer = self
                .client
                .put_object()
                .bucket(&self.config.bucket)
                .key(target)
                .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
                .if_none_match("*")
                .body(ByteStream::from_static(&[]))
                .send()
                .await;
            return match answer {
                Ok(_) => Ok(()),
                Err(error) if is_precondition_failed(&error) => {
                    // Someone published earlier on this key, and here the
                    // asymmetry with the `put` path has no defense: the
                    // 412 of an empty body is checked with the same
                    // honest criterion as any other rejection. With
                    // the bytes known (empty) the check is the
                    // cheapest possible, and repairing is repeating the PUT
                    // without condition.
                    let store = S3BlobStore {
                        client: self.client.clone(),
                        config: self.config.clone(),
                        gate: self.gate.clone(),
                    };
                    let repair = async {
                        self.client
                            .put_object()
                            .bucket(&self.config.bucket)
                            .key(target)
                            .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
                            .body(ByteStream::from_static(&[]))
                            .send()
                            .await
                            .map(|_| ())
                            .map_err(|error| {
                                S3BlobStore::unavailable("repair the empty content", error)
                            })
                    };
                    store
                        .settle_a_refused_write(digest, Some(&[]), repair)
                        .await
                }
                Err(error) => Err(S3BlobStore::unavailable("publish the empty content", error)),
            };
        }
        let mut parts = self.completed.clone();
        parts.sort_by_key(CompletedPart::part_number);
        self.client
            .complete_multipart_upload()
            .bucket(&self.config.bucket)
            .key(&self.staging_key)
            .upload_id(&self.upload_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .send()
            .await
            .map_err(|error| S3BlobStore::unavailable("complete the upload", error))?;
        // The honest verification of a cold-reopened session (the
        // handle's `covers_all` flag): its local hasher did not pass through the
        // parts that another replica uploaded, so the commit could not
        // compare a partial digest against what was declared (it would give a false
        // `DigestMismatch` and abort a good upload). The already completed
        // staging object is read and checked against what was
        // declared **before** copying it to the blob key. What
        // does not add up is not published: the staging is deleted and no
        // reference remains.
        if !self.covers_all {
            let staged = self
                .client
                .get_object()
                .bucket(&self.config.bucket)
                .key(&self.staging_key)
                .send()
                .await
                .map_err(|error| S3BlobStore::unavailable("read the staged content", error))?;
            let bytes = staged
                .body
                .collect()
                .await
                .map_err(|error| S3BlobStore::unavailable("stream the staged content", error))?
                .into_bytes()
                .to_vec();
            let found = ContentDigest::of_bytes(&bytes);
            if found != *digest {
                // The content is not the declared one: the staging is deleted
                // and nothing is copied. A blob with a digest that is not
                // its own is not a blob.
                let _ = self
                    .client
                    .delete_object()
                    .bucket(&self.config.bucket)
                    .key(&self.staging_key)
                    .send()
                    .await;
                return Err(BlobError::DigestMismatch {
                    expected: digest.clone(),
                    found,
                });
            }
        }
        let copied = if self.written > MAX_SIMPLE_COPY {
            self.copy_as_multipart(target, digest, true).await
        } else {
            // The copy is server-side: the bytes do not pass through
            // the process again, and it only goes in if the destination is free.
            match self
                .client
                .copy_object()
                .bucket(&self.config.bucket)
                .key(target)
                .copy_source(format!("{}/{}", self.config.bucket, self.staging_key))
                .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
                .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace)
                .if_none_match("*")
                .send()
                .await
            {
                Ok(_) => Ok(true),
                Err(error) if is_precondition_failed(&error) => Ok(false),
                Err(error) => Err(S3BlobStore::unavailable("publish the blob", error)),
            }
        };
        let copied = match copied {
            // The normal path: the copy went in, or the failure is already stated.
            Ok(true) => Ok(()),
            // Someone published first. Whether it is THIS content is
            // checked by the same honest decision as the `put` path:
            // stamp, or read; what does not add up is repaired. Here we do not
            // have the bytes in the process (they are in the store,
            // under the already completed staging key), so the
            // repair is to copy again without condition. The store
            // view is cheap (the client is an Arc) and shares the
            // barrier: the caller holds the gate since the commit.
            Ok(false) => {
                let store = S3BlobStore {
                    client: self.client.clone(),
                    config: self.config.clone(),
                    gate: self.gate.clone(),
                };
                let repair = self.copy_unconditioned(target, digest);
                store.settle_a_refused_write(digest, None, repair).await
            }
            Err(error) => Err(error),
        };
        let staging_cleanup = self
            .client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(&self.staging_key)
            .send()
            .await
            .map_err(|error| S3BlobStore::unavailable("drop the staging object", error));
        copied?;
        staging_cleanup.map(|_| ())
    }

    /// The server-side copy without the condition: the repair path
    /// of a publication, when the pre-existing object is not the content.
    async fn copy_unconditioned(
        &self,
        target: &str,
        digest: &ContentDigest,
    ) -> Result<(), BlobError> {
        if self.written > MAX_SIMPLE_COPY {
            return self
                .copy_as_multipart(target, digest, false)
                .await
                .map(|_| ());
        }
        self.client
            .copy_object()
            .bucket(&self.config.bucket)
            .key(target)
            .copy_source(format!("{}/{}", self.config.bucket, self.staging_key))
            .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
            .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace)
            .send()
            .await
            .map(|_| ())
            .map_err(|error| S3BlobStore::unavailable("repair the published blob", error))
    }

    /// Copy an object of more than five gigabytes: `CopyObject` does not reach
    /// that far, so the copy is done part by part, with ranges over the
    /// staging key. It is the same as what the mature deployer does with
    /// s3-sr-copy and here it cannot be an external step: the commit is all
    /// or nothing.
    ///
    /// The no-overwrite condition goes on the `Complete`: the
    /// destination does not exist until the multipart copy closes, which is the
    /// atomic point that S3 promises. It returns whether the copy wrote:
    /// `false` is the 412, the "it was already there" that the caller resolves.
    async fn copy_as_multipart(
        &self,
        target: &str,
        digest: &ContentDigest,
        only_if_absent: bool,
    ) -> Result<bool, BlobError> {
        let created = self
            .client
            .create_multipart_upload()
            .bucket(&self.config.bucket)
            .key(target)
            .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
            .send()
            .await
            .map_err(|error| S3BlobStore::unavailable("open the copy upload", error))?;
        let upload_id = created
            .upload_id()
            .map(|id| id.to_string())
            .unwrap_or_default();
        if upload_id.is_empty() {
            return Err(S3BlobStore::unavailable(
                "open the copy upload",
                "the object store gave no upload id",
            ));
        }
        let source = format!("{}/{}", self.config.bucket, self.staging_key);
        let mut parts = Vec::new();
        let mut number = 0i32;
        let mut start = 0u64;
        while start < self.written {
            number += 1;
            let end = (start + self.config.part_size as u64 - 1).min(self.written - 1);
            let sent = self
                .client
                .upload_part_copy()
                .bucket(&self.config.bucket)
                .key(target)
                .copy_source(&source)
                .upload_id(&upload_id)
                .part_number(number)
                .copy_source_range(format!("bytes={start}-{end}"))
                .send()
                .await;
            match sent {
                Ok(sent) => parts.push(
                    CompletedPart::builder()
                        .part_number(number)
                        .set_e_tag(
                            sent.copy_part_result()
                                .and_then(|r| r.e_tag())
                                .map(|etag| etag.to_string()),
                        )
                        .build(),
                ),
                Err(error) => {
                    let abort = self
                        .client
                        .abort_multipart_upload()
                        .bucket(&self.config.bucket)
                        .key(target)
                        .upload_id(&upload_id)
                        .send()
                        .await;
                    drop(abort);
                    return Err(S3BlobStore::unavailable("copy a part", error));
                }
            }
            start = end + 1;
        }
        let mut request = self
            .client
            .complete_multipart_upload()
            .bucket(&self.config.bucket)
            .key(target)
            .upload_id(&upload_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            );
        if only_if_absent {
            request = request.if_none_match("*");
        }
        match request.send().await {
            Ok(_) => Ok(true),
            Err(error) if only_if_absent && is_precondition_failed(&error) => {
                // The destination was already there: the copy was not needed. But the
                // multipart that we opened **on the destination key**
                // is still alive on the server, and its key is not
                // a staging one (the reaper does not look at it and `list_blobs` does not
                // see it). It is aborted here, before yielding the verdict to the
                // caller; leaving it would trade a harmless race for a
                // permanent leak of paid-for parts.
                let _ = self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.config.bucket)
                    .key(target)
                    .upload_id(&upload_id)
                    .send()
                    .await;
                Ok(false)
            }
            Err(error) => Err(S3BlobStore::unavailable("finish the copy", error)),
        }
    }
}

/// How long a part may take before the session gives it up for lost.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// What `CopyObject` serves in one go: above it, the publication
/// needs a multipart copy. Five gigabytes, the server maximum.
const MAX_SIMPLE_COPY: u64 = 5 * 1024 * 1024 * 1024;

#[async_trait::async_trait]
impl BlobWriteSession for S3BlobWriteSession {
    async fn append(&mut self, chunk: &[u8]) -> Result<(), BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        self.buffer.extend_from_slice(chunk);
        self.hasher.update(chunk);
        self.written += chunk.len() as u64;
        while self.buffer.len() >= self.config.part_size {
            let rest = self.buffer.split_off(self.config.part_size);
            self.flush_part(false).await?;
            self.buffer = rest;
        }
        Ok(())
    }

    async fn commit(&mut self, expectation: CommitExpectation) -> Result<ContentDigest, BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        if expectation.digest.algorithm != DigestAlgorithm::Sha256 {
            self.discard().await;
            return Err(BlobError::UnsupportedAlgorithm(
                expectation.digest.algorithm.to_string(),
            ));
        }
        // What remains last in the buffer is the final part, and the final
        // part is the only one that may be shorter than the budget.
        self.flush_part(true).await?;
        while let Some(handle) = self.pending.pop_front() {
            self.collect_part(tokio::time::timeout(UPLOAD_TIMEOUT, handle).await)?;
        }

        if self.written != expectation.length {
            self.discard().await;
            return Err(BlobError::LengthMismatch {
                expected: expectation.length,
                found: self.written,
            });
        }

        // The honest digest check. When the handle passed through
        // all the bytes, the local hasher is the truth and is compared
        // here, cheaply. When the session was reopened cold (the journal
        // declared it and another replica wrote the first parts), the
        // local hasher is **partial**: comparing it against what was declared
        // would give a false `DigestMismatch` and abort a good upload.
        // In that case the check is made by `publish` against the
        // already completed staging: the same honest verification, once,
        // at publication.
        let declared = expectation.digest;
        if self.covers_all {
            let found = ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()));
            if found != declared {
                self.discard().await;
                return Err(BlobError::DigestMismatch {
                    expected: declared,
                    found,
                });
            }
        }

        // From here until publication, the collector cannot delete anything.
        let barrier = self.gate.lock().await;
        let target = blob_key(&declared);
        let outcome = self.publish(&target, &declared).await;
        drop(barrier);
        self.open = false;
        outcome?;
        Ok(declared)
    }

    async fn abort(&mut self) -> Result<(), BlobError> {
        if !self.open {
            return Ok(());
        }
        self.open = false;
        self.buffer.clear();
        for handle in self.pending.drain(..) {
            handle.abort();
        }
        if self.completed.is_empty() && self.written == 0 {
            // Nothing uploaded: there is also nothing to abort in the store.
            return Ok(());
        }
        self.client
            .abort_multipart_upload()
            .bucket(&self.config.bucket)
            .key(&self.staging_key)
            .upload_id(&self.upload_id)
            .send()
            .await
            .map_err(|error| S3BlobStore::unavailable("abort the upload", error))?;
        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        self.written
    }

    /// What the store has: bytes in flight are not durable until
    /// the server confirms them, and what remains in the buffer has not left
    /// the process. It is the offset that the journal can declare.
    fn the_durable_offset(&self) -> u64 {
        self.written - self.buffer.len() as u64
    }

    fn the_digest(&self) -> ContentDigest {
        ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()))
    }

    fn the_staging_path(&self) -> Option<&std::path::Path> {
        None
    }

    /// The journal handle: the staging key and the `upload_id`, joined
    /// by a zero that neither of the two can carry.
    fn the_staging_handle(&self) -> Option<String> {
        Some(format!("{}\u{0}{}", self.staging_key, self.upload_id))
    }

    /// A cold-reopened multipart has not seen its parts again: its
    /// local hasher is partial, and the commit has to check the bytes
    /// against the store.
    fn the_digest_covers_all(&self) -> bool {
        self.covers_all
    }

    /// Releases the handle **only if nothing remains in the buffer**.
    ///
    /// The multipart has no intermediate staging to which the buffer's bytes
    /// can return: releasing with bytes inside would lose them, and
    /// the journal's authority did not declare them. With an empty buffer and
    /// the in-flight parts already collected by `flush_durable`, releasing is
    /// closing without aborting: the multipart stays alive and the next `PATCH`
    /// resumes it by its handle, here or on another replica.
    fn detach(&mut self) -> Result<(), BlobError> {
        if !self.buffer.is_empty() {
            return Err(BlobError::Unavailable(
                "a multipart session with bytes in the buffer does not release its writer"
                    .to_owned(),
            ));
        }
        self.open = false;
        for handle in self.pending.drain(..) {
            handle.abort();
        }
        Ok(())
    }

    /// The journal takes ownership of the multipart: the Drop no longer aborts it.
    ///
    /// A released handle or a process that dies does NOT abort another
    /// replica's upload; its destination is decided by the authority: commit, explicit
    /// abort or sweep. The resume checks against `ListParts` that
    /// the store has exactly the bytes that the authority declares
    /// and redoing the upload is the only path if a duplicated part put
    /// the staging ahead of the journal. It is never published ahead
    /// of the authority.
    fn mark_durable(&mut self) {
        self.durable = true;
    }

    /// Collects the in-flight parts and declares the durable offset.
    ///
    /// What the server confirmed is durable; what remains in the
    /// buffer does not leave the process, and the client resends it against the
    /// `Range` that the authority declares.
    async fn flush_durable(&mut self) -> Result<u64, BlobError> {
        while let Some(handle) = self.pending.pop_front() {
            self.collect_part(tokio::time::timeout(UPLOAD_TIMEOUT, handle).await)?;
        }
        Ok(self.written - self.buffer.len() as u64)
    }
}

impl Drop for S3BlobWriteSession {
    fn drop(&mut self) {
        if !self.open {
            return;
        }
        // The session was abandoned without committing or aborting: the staging
        // cannot outlive whoever opened it. The abort goes to the current
        // runtime; if there is no runtime, the bucket lifecycle (or the
        // staging lifecycle policy) collects it.
        self.open = false;
        for handle in self.pending.drain(..) {
            handle.abort();
        }
        // With a journal, the multipart's destination is decided by the authority:
        // a local handle that dies (evicted cache, replica that continues)
        // does NOT abort another replica's upload.
        if self.durable {
            return;
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let client = self.client.clone();
            let bucket = self.config.bucket.clone();
            let key = self.staging_key.clone();
            let upload_id = self.upload_id.clone();
            handle.spawn(async move {
                let _ = client
                    .abort_multipart_upload()
                    .bucket(&bucket)
                    .key(&key)
                    .upload_id(&upload_id)
                    .send()
                    .await;
            });
        }
    }
}

#[async_trait::async_trait]
impl BlobReclaim for S3BlobStore {
    async fn list_blobs(&self) -> Result<Vec<StoredBlob>, BlobError> {
        let mut blobs = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.config.bucket)
                .prefix(format!("{BLOB_PREFIX}/"))
                .max_keys(1000);
            if let Some(token) = &token {
                request = request.continuation_token(token);
            }
            let page = request
                .send()
                .await
                .map_err(|error| Self::unavailable("list the blobs", error))?;
            for object in page.contents() {
                // What has no blob shape of this store is not of
                // this store: it is not listed and, therefore, not deleted.
                let Some(key) = object.key() else { continue };
                let Some(digest) = Self::digest_of_key(key) else {
                    continue;
                };
                // Without a date there is no age, and without age nothing is decided:
                // an object without Last-Modified is not offered to the collector.
                let Some(age) = Self::age_of(object.last_modified()) else {
                    continue;
                };
                blobs.push(StoredBlob {
                    digest,
                    length: object.size.unwrap_or(0).max(0) as u64,
                    age,
                });
            }
            if page.is_truncated() == Some(true) {
                token = page.next_continuation_token.map(|t| t.to_string());
                if token.is_none() {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(blobs)
    }

    async fn delete_if_older_than(
        &self,
        digest: &ContentDigest,
        grace: Duration,
    ) -> Result<Reclaimed, BlobError> {
        if digest.algorithm != DigestAlgorithm::Sha256 || digest.value.len() != 64 {
            return Err(BlobError::UnsupportedAlgorithm(
                digest.algorithm.to_string(),
            ));
        }
        let key = self.the_key_of(digest);
        let _barrier = self.gate.lock().await;
        let head = match self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(head) => head,
            Err(error) if is_not_found(&error) => return Ok(Reclaimed::Gone),
            Err(error) => return Err(Self::unavailable("look up the blob", error)),
        };
        // The age is checked with the barrier held: what is checked here
        // remains true when it is deleted below.
        let Some(age) = Self::age_of(head.last_modified()) else {
            return Err(Self::unavailable(
                "check the blob age",
                "the object store served no last-modified",
            ));
        };
        if age < grace {
            return Ok(Reclaimed::Fresh);
        }
        let length = head.content_length.unwrap_or(0).max(0) as u64;
        match self
            .client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => Ok(Reclaimed::Deleted { length }),
            Err(error) if is_not_found(&error) => Ok(Reclaimed::Gone),
            Err(error) => Err(Self::unavailable("delete the blob", error)),
        }
    }
}

/// The hexadecimal of the bytes.
fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
