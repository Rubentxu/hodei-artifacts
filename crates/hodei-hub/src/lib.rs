//! The trusted artifact hub: where Hodei stops being a catalog
//! and becomes the one that stores and distributes.
//!
//! # The conclusion of SPIKE-010
//!
//! The spike's question was where the **native ownership of the artifact**
//! gives a control-plane value that integration with an external
//! registry does not. And the answer is: **quarantine and promotion**.
//!
//! - An external registry stores bytes and serves them: it does that well, and
//!   Hodei is not going to do it better.
//! - An external registry **does not know** whether an artifact is trustworthy: it does not have
//!   Hodei's trust decision, and cannot have it, because the
//!   decision comes from the evidence graph.
//!
//! That is: Hodei does not compete as a store.
//! It competes as an enforcement point.
//!
//! The OCI subset is the minimum needed to
//! talk to the world, and what Hodei adds is the trust channel.
//!
//! And the non-goals are explicit: **Maven and npm are not**, until there is
//! demand, a protocol spike and a new UAT. That the old code
//! exists is not an argument: the old code is looked at as a source of
//! domain and test knowledge, not as a sunk cost.
//!
//! # The three rules of the milestone
//!
//! 1. **Without a trust decision there is no promotion.** An artifact without a
//!    decision is in quarantine, and quarantine is the DEFAULT
//!    state of a newly admitted artifact.
//! 2. **Promoting does not mutate the bytes.** The digest is the identity of the
//!    content, and promotion changes the channel, not what is inside.
//! 3. **The same content is not stored twice.** The identity of the
//!    content is its digest, and on read it is recomputed and
//!    checked.
//!
//! And what Hodei does **not** do: speak the whole protocol. The supported
//! OCI subset is declared in the code (`oci::SUPPORTED_MANIFESTS`),
//! and what is not on the list is not served.

#![forbid(unsafe_code)]

pub mod blob;
pub mod hub;
pub mod oci;
pub mod reclaim;

pub use blob::{
    BlobError, BlobStore, BlobWriteSession, CommitExpectation, FileBlobStore, FileBlobWriteSession,
    InMemoryBlobStore, InMemoryBlobWriteSession, ReadPolicy, StagingEntry,
};
pub use hub::{
    ArtifactHub, Availability, HubArtifact, PromotionAudit, PromotionError, TrustVerdict,
};
pub use oci::{OciDescriptor, OciError, OciManifest, OciSupport};
pub use reclaim::{
    BlobReclaim, ConditionalBlob, ConditionalOutcome, Reclaimed, StoredBlob, VersionMarker,
};
