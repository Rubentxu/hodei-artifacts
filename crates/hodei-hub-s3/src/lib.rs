//! The S3-compatible backend of the content store.
//!
//! Hodei owns the bytes; the place where it holds them may be a directory
//! or an Object Store that speaks the S3 protocol. This crate does **not** add a
//! third store: it implements the single port [`hodei_hub::blob::BlobStore`]
//! on top of `aws-sdk-s3`, so that the domain never knows whether behind it there is
//! AWS, MinIO, R2 or Ceph.
//!
//! # The backend decisions
//!
//! - **The key of a blob is its digest**: `blobs/<ab>/<cd>/<hex>`, the same
//!   layout as the file store. In a CAS the key is the identity,
//!   and the identity is the content: publishing the same content twice
//!   corrupts nothing, because the bytes of the last are the bytes of the first.
//! - **The write session is a multipart upload**: the parts upload as
//!   soon as they fill `part_size` and with their `Content-MD5`, and the process
//!   memory is bounded by `part_size * max_inflight_parts`. The commit
//!   **completes nothing that does not add up**: if the length or the digest differ
//!   from what was declared, the upload is aborted and no visible reference
//!   remains. That is the integrity condition of finalization.
//! - **The collector lives off `Last-Modified`**: `list_blobs` walks the
//!   paginated prefix and `delete_if_older_than` checks the age before deleting,
//!   with the same barrier between publishing and collecting as the file
//!   store. The leases and referenced tags are respected by the GC
//!   service, which is the one that decides; this backend only does its part: not deleting
//!   what was published a moment ago.
//! - **Credentials do not pass through here**: the store receives a client
//!   already built by the deployment, with the SDK's secure provider
//!   chain (environment variables, shared file, instance role).
//!   Nothing in the store keeps, derives or prints a secret, and the errors
//!   it produces do not either: the store's `Debug` shows the public
//!   configuration and never the client.
//!
//! # The memory budget and the benchmark
//!
//! The budget is declared, not discovered: a session in progress may
//! have `max_inflight_parts` parts of `part_size` bytes simultaneously
//! in transmission, and that product is the memory ceiling per session. With
//! the default values (8 MiB x 4) it is 32 MiB, versus the `put` path,
//! which receives the whole content in memory by contract of the
//! port. `tests/hsp116_s3_bench.rs` checks the ceiling with a blob six
//! times larger than the budget, measuring the resident memory of the
//! process —the test S3 server lives in another process, in a
//! LocalStack container— and leaves comparative numbers against the file
//! store.

mod store;

pub use store::{S3BlobConfig, S3BlobConfigError, S3BlobStore};
