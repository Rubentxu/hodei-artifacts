//! The trust passport as a capsule extension (HSP-160).
//!
//! What is checked here is not that the passport *declares* a
//! verification, but that it *truly contains* it: the VSA
//! verdicts come from `cosign_verify` and the SLSA ladder run over the
//! signed bytes, not from a self-assertion. Each acceptance
//! condition has a test that can fail:
//!
//! - AC1 a VSA with real verdicts: a foreign key produces an
//!   honest `FAILED`, never a laundered `PASSED`; a reader that brings
//!   expectations the material does not confirm is rejected with
//!   `verdict-mismatch`, not reinterpreted.
//! - AC3 an isolated import keeps the origin and the instants of the
//!   record, and revocation freshness offline is
//!   `UnknownOffline` — never "no revocation, therefore valid today".
//! - AC4 two exports of the same inputs reproduce the
//!   manifest identity byte for byte; the signatures are declared
//!   outside that identity.
//!
//! The AC2 condition (external tools: `oras`, `cosign`, `gpg`)
//! lives in its own target, `hsp160_trust_passport_external.rs`, because
//! it needs real system binaries and runs in a separate lane.

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{Duration, Utc};
use hodei_catalog::model::{
    DigestScope, PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog::{
    CatalogWriter, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
    TrustGrant, TrustLedger,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_evidence::domain::standards::attestation::DsseEnvelope;
use hodei_evidence::standards::{
    FreshnessNow, RootRevision, SlsaBuildLevel, TrustRoots, VerificationKey,
};
use hodei_hub::BlobStore;
use hodei_hub::blob::FileBlobStore;
use serde_json::json;

use hodei_artifacts_api::bundles::passport::{
    PASSPORT_ARTIFACT_TYPE, PassportProblemCode, PassportReading, SIGSTORE_BUNDLE_MEDIA_TYPE,
    SignedDocument, VSA_MEDIA_TYPE, export_passport, verify_passport_offline,
};
use hodei_artifacts_api::bundles::{
    self, BundleError, Mapping, TrustState, passport::PassportInputs, passport::PassportPolicy,
    passport::ReaderExpectations,
};
use hodei_artifacts_api::openpgp::MetadataSigner;

// The graph profile of the harness: a REAL Dgraph namespace per site.
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

const UPSTREAM_URL: &str = "https://repo.example.org/raw/";
const ISSUER: &str = "registry.example";

// The site scaffolding is the package contract's (hsp118): a
// capsule that is not exported from a real package proves nothing.
include!("support/hsp118_site.rs");
// The signed provenance material scaffolding (HSP-160).
include!("support/hsp160_material.rs");

/// A package with the three forms a path can take: confirmed
/// with a decision, confirmed without a decision, and withheld. The same scaffolding
/// as the capsule contract (hsp126): the passport is exported over a
/// package that has already gone through those three routes.
async fn three_way_site() -> (Site, ContentDigest) {
    let site = Site::new().await;
    site.create_proxy("acme", "mirror", "central").await;
    let decided = site
        .resolve("acme", "mirror", "tools/a.bin", b"tool a")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/a.bin",
        &decided,
        6,
        ReceiptOutcome::Fetched,
        "central",
        1,
    )
    .await;
    site.trust
        .grant(&[TrustGrant {
            account: "acme".to_string(),
            repository: "mirror".to_string(),
            digest: decided.clone(),
            decision_id: "dec-1".to_string(),
            actor: "hrn:hodei:iam:acme:User/carol".to_string(),
            granted_at: Utc::now() - Duration::days(1),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: None,
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: None,
            risk_version: None,
            valid_until: Some(Utc::now() + Duration::hours(1)),
        }])
        .await
        .expect("the grant exists");
    let bare = site
        .resolve("acme", "mirror", "tools/b.bin", b"tool b")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/b.bin",
        &bare,
        6,
        ReceiptOutcome::Fetched,
        "central",
        1,
    )
    .await;
    // c.bin has no receipt: it is withheld and the capsule says so.
    site.resolve("acme", "mirror", "tools/c.bin", b"tool c")
        .await;
    (site, decided)
}

fn scratch_signer(tag: &str) -> MetadataSigner {
    MetadataSigner::generate(&format!("HSP-160 {tag} <passport@example.local>")).expect("a key")
}

fn read_json(path: &Path) -> serde_json::Value {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    serde_json::from_slice(&raw).expect("the layout holds JSON")
}

/// The hex of a `sha256:<hex>` digest, to open its blob in the layout.
fn blob_hex(digest: &str) -> &str {
    digest.strip_prefix("sha256:").expect("a sha256 digest")
}

/// The digest the capsule index announces first: its image.
fn capsule_digest(capsule_dir: &Path) -> String {
    let index = read_json(&capsule_dir.join("index.json"));
    index["manifests"][0]["digest"]
        .as_str()
        .expect("a digest")
        .to_string()
}

/// What a test needs after exporting a passport: the live site
/// (its `TempDir` holds the files), the roots, the policy and
/// the signer. `layout` is the OCI directory of the freshly
/// written passport.
struct PassportCase {
    _site: Site,
    roots: TrustRoots,
    policy: PassportPolicy,
    signer: MetadataSigner,
    layout: std::path::PathBuf,
}

/// Exports a passport over a real package, with provenance signed
/// by the alpha root anchored to `registry.example`.
async fn export_good(name: &str) -> PassportCase {
    let (site, _) = three_way_site().await;
    let signer = scratch_signer(name);
    let bundle_dir = export_with(&site, name, Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let revision = alpha.revision(1, hsp160_verified_at() - Duration::days(30));
    let roots = alpha.trust_roots(revision.clone());
    let policy = hsp160_policy();
    let inputs = PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: hsp160_provenance(&alpha, &digest),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: vec![revision],
        verified_at: hsp160_verified_at(),
    };
    let layout = bundle_dir.join(format!("{name}.passport"));
    export_passport(&inputs, &layout, &signer).expect("the passport exports");
    PassportCase {
        _site: site,
        roots,
        policy,
        signer,
        layout,
    }
}

fn read_with(
    case: &PassportCase,
    issuer: Option<String>,
    online_freshness: Option<Duration>,
) -> Result<PassportReading, BundleError> {
    verify_passport_offline(
        &case.layout,
        &case.signer,
        &ReaderExpectations {
            roots: &case.roots,
            policy: &case.policy,
            issuer,
            online_freshness,
        },
        hsp160_verified_at(),
    )
}

/// Isolated offline read with the correct expectations of the good case.
fn read_ok(case: &PassportCase) -> PassportReading {
    read_with(case, Some(ISSUER.to_string()), None)
        .expect("a passport built over real pinned provenance reads")
}

/// The rejection code with which the passport answered. `PassportProblem`
/// writes the code in `Debug` form between brackets; the structural
/// layout rejections that `tag_missing_and_tampered` labels
/// carry serde's kebab form (`[missing-evidence]`,
/// `[tampered-blob]`), so both are tested.
fn code_of(error: &BundleError) -> PassportProblemCode {
    let text = error.to_string();
    [
        PassportProblemCode::MissingEvidence,
        PassportProblemCode::TamperedBlob,
        PassportProblemCode::PolicyMismatch,
        PassportProblemCode::VerdictMismatch,
        PassportProblemCode::SignerRevoked,
        PassportProblemCode::SubjectMismatch,
        PassportProblemCode::Malformed,
        PassportProblemCode::SignatureInvalid,
    ]
    .into_iter()
    .find(|code| {
        text.contains(&format!("{code:?}"))
            || text.contains(
                &serde_json::to_value(code)
                    .expect("a code serializes")
                    .as_str()
                    .expect("a kebab code")
                    .to_string(),
            )
    })
    .unwrap_or_else(|| panic!("the error names no passport code: {text}"))
}

#[tokio::test]
async fn ac1_the_vsa_carries_verdicts_the_material_actually_produces() {
    let case = export_good("good").await;
    let reading = read_ok(&case);

    // AC1: the VSA result is the level the ladder actually
    // asserts over these bytes, not a hand-set label. With the
    // trusted builder, the anchored issuer and the two policy
    // establishments declared, the material yields L3.
    assert_eq!(
        reading.vsa.predicate.verification_result,
        "SLSA_BUILD_LEVEL_3"
    );
    assert_eq!(reading.recorded.asserted_level(), Some(SlsaBuildLevel::L3));
    assert!(
        reading.recorded.gaps().is_empty(),
        "an L3 passport has no gaps: {:?}",
        reading.recorded.gaps()
    );
    // The anti-self-assertion gate: what was recorded and what was recomputed are
    // equal requirement by requirement (the read only passed because of that).
    assert_eq!(reading.recorded.findings, reading.recomputed.findings);
    assert!(
        reading.cryptographic_holds,
        "the alpha root signature holds"
    );
    assert_eq!(reading.root, Some(("alpha".to_string(), 1)));
}

#[tokio::test]
async fn ac1_a_foreign_key_lands_as_an_honest_failed_never_a_washed_pass() {
    // The provenance is signed by a foreign root (beta) that the export's
    // roots do not know. The export does NOT launder it into a pass: it runs
    // `cosign_verify`, sees the unknown key and writes a FAILED.
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("foreign");
    let bundle_dir = export_with(&site, "foreign", Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let revision = alpha.revision(1, hsp160_verified_at() - Duration::days(30));
    let roots = alpha.trust_roots(revision.clone());
    let policy = hsp160_policy();
    let beta = SignedRoot::generate("hsp160-root-beta-v1", "beta", "other-registry.example");

    let layout = bundle_dir.join("foreign.passport");
    let inputs = PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: hsp160_provenance(&beta, &digest),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: vec![revision],
        verified_at: hsp160_verified_at(),
    };
    let export =
        export_passport(&inputs, &layout, &signer).expect("a foreign key still exports as FAILED");
    assert!(!export.cryptographic_holds, "a foreign key does not hold");

    let reading = verify_passport_offline(
        &layout,
        &signer,
        &ReaderExpectations {
            roots: &roots,
            policy: &policy,
            issuer: Some(ISSUER.to_string()),
            online_freshness: None,
        },
        hsp160_verified_at(),
    )
    .expect("a FAILED reads as honestly as a PASSED");
    // The record says FAILED and the material recomputes to FAILED: the
    // real verification overrides any hand-set label.
    assert_eq!(reading.vsa.predicate.verification_result, "FAILED");
    assert_eq!(reading.recorded, reading.recomputed);
    assert!(
        !reading.recorded.unmet_at(SlsaBuildLevel::L1).is_empty(),
        "a FAILED has at least one unmet floor requirement"
    );
    assert!(!reading.cryptographic_holds);
    // The ladder asserts no level over an unknown key.
    assert_eq!(reading.recorded.asserted_level(), None);
}

#[tokio::test]
async fn ac1_a_reader_with_wrong_expectations_is_refused_not_reinterpreted() {
    // The export anchored the registry.example issuer and came out L3. A reader
    // that anchors no issuer re-runs the chain without a pin: the
    // ladder marks the pin as `NotEvaluated` (a gap) and drops to L1. The
    // anti-self-assertion gate sees that the record says 3 and the material
    // 1, and rejects with verdict-mismatch instead of re-substituting the gap.
    let case = export_good("mismatch").await;
    let error = read_with(&case, None, None)
        .expect_err("a reader without an anchor cannot re-read an anchored record");
    assert_eq!(code_of(&error), PassportProblemCode::VerdictMismatch);
    let text = error.to_string();
    assert!(
        text.contains("SLSA_BUILD_LEVEL_3") && text.contains("SLSA_BUILD_LEVEL_1"),
        "the rejection compares what was recorded with what was recomputed: {text}"
    );
}

#[tokio::test]
async fn ac1_a_foreign_key_cannot_be_laundered_into_the_record() {
    // The export guard: if the key that signed is a root published
    // in the ring but is not among the revisions the passport
    // would declare, the export is rejected — a record cannot document
    // some roots and vouch with others.
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("launder");
    let bundle_dir = export_with(&site, "launder", Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let r1 = alpha.revision(1, hsp160_verified_at() - Duration::days(30));
    // Beta shares the issuer so the chain reaches Verified (a
    // broken pin does not trigger the signature guard); only its lineage changes.
    let beta = SignedRoot::generate("hsp160-root-beta-v1", "beta", ISSUER);
    let b1 = beta.revision(1, hsp160_verified_at() - Duration::days(30));
    // The export's ring knows alpha and beta; the passport would only publish alpha.
    let mut roots = alpha.trust_roots(r1.clone());
    roots.activate(b1, "eve", "a lineage the record would not publish");
    let policy = hsp160_policy();

    let inputs = PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: hsp160_provenance(&beta, &digest),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: vec![r1],
        verified_at: hsp160_verified_at(),
    };
    let error = export_passport(&inputs, &bundle_dir.join("launder.passport"), &signer)
        .expect_err("the signing key cannot be left out of the record");
    assert!(
        error.to_string().contains("not among the revisions"),
        "the rejection names the unpublished revision: {error}"
    );
}

#[tokio::test]
async fn ac1_a_self_written_vsa_cannot_become_a_pass() {
    // Self-assertion: rewriting the VSA layer so it proclaims another
    // verdict does not fool the reader. The VSA blob is content-
    // addressed: changing its bytes breaks the digest the manifest
    // announces, and `read_declared_blobs` rejects it before reading it.
    let case = export_good("selfwritten").await;
    let passport = read_json(
        &case
            .layout
            .join("blobs/sha256")
            .join(blob_hex(&hsp160_passport_digest(&case.layout))),
    );
    let vsa_digest = hsp160_layer_digest(&passport, VSA_MEDIA_TYPE);

    // Forge: same shape, result and findings changed.
    let mut forged: serde_json::Value =
        serde_json::from_slice(&hsp160_blob(&case.layout, &vsa_digest)).unwrap();
    forged["predicate"]["verificationResult"] = json!("SLSA_BUILD_LEVEL_0");
    hsp160_write_blob(
        &case.layout,
        &vsa_digest,
        &serde_json::to_vec_pretty(&forged).expect("forge serializes"),
    );

    let error = read_with(&case, Some(ISSUER.to_string()), None)
        .expect_err("a tampered VSA is not read as good");
    assert_eq!(
        code_of(&error),
        PassportProblemCode::TamperedBlob,
        "the VSA digest is no longer the one the manifest promises: {error}"
    );
}

#[tokio::test]
async fn ac1_a_tampered_envelope_is_refused_at_its_digest() {
    // Tampering with the envelope (the sigstore bundle layer) breaks the digest
    // the VSA names in `inputAttestations`: verdict-mismatch, because
    // the record evaluated other bytes. The envelope is NOT anchored in the
    // manifest, so the layer is read; it is the VSA's digest that
    // reveals that the bytes are no longer the verified ones.
    let case = export_good("envelope-tamper").await;
    let passport = read_json(
        &case
            .layout
            .join("blobs/sha256")
            .join(blob_hex(&hsp160_passport_digest(&case.layout))),
    );
    let bundle_digest = hsp160_layer_digest(&passport, SIGSTORE_BUNDLE_MEDIA_TYPE);
    let mut bundle: serde_json::Value =
        serde_json::from_slice(&hsp160_blob(&case.layout, &bundle_digest)).unwrap();
    // Change the signature inside the bundle without touching the VSA digest.
    bundle["dsseEnvelope"]["signatures"][0]["sig"] = json!(BASE64.encode([0u8; 8]));
    hsp160_write_blob(
        &case.layout,
        &bundle_digest,
        &serde_json::to_vec_pretty(&bundle).expect("bundle serializes"),
    );

    // The tampering changes the bytes of the bundle blob; the manifest
    // descriptor still announces the old digest -> the blob lies.
    let error = read_with(&case, Some(ISSUER.to_string()), None)
        .expect_err("a tampered envelope does not pass");
    assert!(
        code_of(&error) == PassportProblemCode::TamperedBlob
            || code_of(&error) == PassportProblemCode::VerdictMismatch,
        "either the bundle blob lies or the VSA digest gives it away: {error}"
    );
}

#[tokio::test]
async fn ac3_isolated_import_keeps_origin_and_temporal_validity() {
    let case = export_good("origin").await;
    let reading = read_ok(&case);

    // AC3: the package origin survives a read without a service.
    assert_eq!(reading.manifest.origin.account, "acme");
    assert_eq!(reading.manifest.origin.repository, "mirror");
    assert_eq!(reading.trust_state.origin, reading.manifest.origin);
    // The record's temporal validity is the one frozen at export.
    assert_eq!(reading.verified_at, hsp160_verified_at());
    // The trust capsule keeps the original decision and the absence.
    let decided = reading
        .trust_state
        .entries
        .iter()
        .find(|e| e.asset_path.ends_with("tools/a.bin"))
        .expect("the decided path");
    assert_eq!(
        decided.decision.as_ref().expect("a decision").decision_id,
        "dec-1",
        "the original decision is kept"
    );
    assert_eq!(reading.trust_state.entries.len(), 2);
    assert_eq!(
        reading.trust_state.withheld.len(),
        1,
        "the absence is still counted"
    );
    // The passport's TrustState is derivable from the manifest it carries.
    assert_eq!(reading.trust_state, TrustState::of(&reading.manifest));
}

#[tokio::test]
async fn ac3_offline_never_claims_fresh_revocation_state() {
    let case = export_good("fresh").await;
    // Offline read: even if the read instant is the record's own,
    // revocation freshness is NOT Fresh — it is UnknownOffline.
    let reading = read_ok(&case);
    assert_eq!(reading.freshness, FreshnessNow::UnknownOffline);
    assert!(reading.cryptographic_holds, "the bytes do hold");
    assert!(
        !reading.proves_current(),
        "offline, freshness is not asserted: UnknownOffline is not a green light"
    );
    // The bound is declared inside the VSA itself, not only in the read.
    assert_eq!(
        reading
            .vsa
            .predicate
            .hodei_offline_limits
            .revocation_freshness,
        "unknown-offline-today"
    );
    assert!(
        reading
            .vsa
            .predicate
            .hodei_offline_limits
            .no_transparency_log
    );
    assert_eq!(
        reading.vsa.predicate.hodei_offline_limits.issuer_kind,
        "hodei-own-verification"
    );
}

#[tokio::test]
async fn ac3_a_revoked_signing_key_is_refused_although_the_signature_holds() {
    // The reader broke the root and revoked the revision that signed: the signature
    // is still valid over the bytes, but trust withdrew it.
    // The read rejects it with signer-revoked, not as a signature failure.
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("revoked");
    let bundle_dir = export_with(&site, "revoked", Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let r1 = alpha.revision(1, hsp160_verified_at() - Duration::days(60));
    let roots = alpha.trust_roots(r1.clone());
    let policy = hsp160_policy();
    let layout = bundle_dir.join("revoked.passport");
    let inputs = PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: hsp160_provenance(&alpha, &digest),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: vec![r1],
        verified_at: hsp160_verified_at(),
    };
    export_passport(&inputs, &layout, &signer).expect("while the key is alive, it exports");

    // Revoke revision 1 by rotating to a 2 and marking 1 revoked
    // before the read instant.
    let mut roots = roots;
    let second = RootRevision {
        lineage: "alpha".to_string(),
        revision: 2,
        key: VerificationKey {
            key_id: "hsp160-root-alpha-v2".to_string(),
            point: alpha.point.clone(),
        },
        issuer: Some(ISSUER.to_string()),
        activated_at: hsp160_verified_at(),
        retired_at: None,
        revoked_at: None,
    };
    roots
        .rotate(
            "alpha",
            second,
            true,
            hsp160_verified_at() - Duration::days(1),
            "eve",
            "the key leaked",
        )
        .expect("the rotation revokes revision 1 before the reading");

    let error = verify_passport_offline(
        &layout,
        &signer,
        &ReaderExpectations {
            roots: &roots,
            policy: &policy,
            issuer: Some(ISSUER.to_string()),
            online_freshness: None,
        },
        hsp160_verified_at(),
    )
    .expect_err("a revoked key does not vouch for the passport");
    assert_eq!(code_of(&error), PassportProblemCode::SignerRevoked);
    assert!(
        error.to_string().contains("revoked"),
        "the rejection names the revocation, not a signature failure: {error}"
    );
}

#[tokio::test]
async fn ac4_two_exports_of_the_same_inputs_reproduce_the_manifest_identity() {
    let (site, _) = three_way_site().await;
    let signer = scratch_signer("repro");
    let bundle_dir = export_with(&site, "repro", Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let r1 = alpha.revision(1, hsp160_verified_at() - Duration::days(30));
    let roots = alpha.trust_roots(r1.clone());
    let published = vec![r1];
    let policy = hsp160_policy();

    // The provenance is ONE canonical input: it is signed only once. Two
    // ECDSA signatures of the same message are not the same envelope (the nonce
    // is random), and the reproducible identity is sworn over the
    // same inputs, not over re-made signatures.
    let provenance = hsp160_provenance(&alpha, &digest);
    let build_inputs = || PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: provenance.clone(),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: published.clone(),
        verified_at: hsp160_verified_at(),
    };
    let a = export_passport(&build_inputs(), &bundle_dir.join("a.passport"), &signer).expect("a");
    let b = export_passport(&build_inputs(), &bundle_dir.join("b.passport"), &signer).expect("b");

    // AC4: the manifest identity is identical byte for byte.
    println!("AC4 passport identity a: {}", a.image_digest);
    println!("AC4 passport identity b: {}", b.image_digest);
    println!("AC4 vsa digest: {}", a.vsa_digest);
    assert_eq!(
        a.image_digest, b.image_digest,
        "misma identidad reproducible"
    );
    assert_eq!(
        a.signed_manifest, b.signed_manifest,
        "the same signed bytes"
    );
    assert_eq!(a.vsa_digest, b.vsa_digest);
    assert_eq!(a.asserted_level, Some(SlsaBuildLevel::L3));

    // The signatures and timestamps are declared outside the
    // identity, not silently excluded.
    let manifest: serde_json::Value = serde_json::from_slice(&a.signed_manifest).unwrap();
    assert_eq!(
        manifest["annotations"]["hodei.passport.signatures"],
        json!("detached-openpgp-referrer-excluded-from-identity")
    );
    assert_eq!(
        manifest["annotations"]["hodei.passport.timestamps"],
        json!("input-declared-in-layers; no clock in manifest")
    );

    // And the isolated re-read of the first copy confirms the same
    // manifest identity the export announced.
    let case_a = PassportCase {
        _site: site,
        roots,
        policy,
        signer,
        layout: a.layout_dir.clone(),
    };
    let reading = read_ok(&case_a);
    assert_eq!(reading.passport_image_digest, a.image_digest);
    assert!(reading.cryptographic_holds);
    assert_eq!(reading.freshness, FreshnessNow::UnknownOffline);
}

#[tokio::test]
async fn ac4_missing_evidence_is_named_as_absent_not_as_invalid() {
    // Removing a blob from the layout is missing evidence (missing-evidence),
    // distinct from evidence that lies (tampered-blob). A consumer without a
    // service must be able to say which absence it had.
    let case = export_good("missing").await;
    let passport = read_json(
        &case
            .layout
            .join("blobs/sha256")
            .join(blob_hex(&hsp160_passport_digest(&case.layout))),
    );
    let sbom_digest = hsp160_layer_digest(&passport, "application/vnd.cyclonedx+json");
    hsp160_remove_blob(&case.layout, &sbom_digest);

    let error = read_with(&case, Some(ISSUER.to_string()), None)
        .expect_err("a passport missing a blob is not read");
    assert!(
        error.to_string().contains("missing-evidence"),
        "the absence is named an absence, not an invalidation: {error}"
    );
}

#[tokio::test]
async fn the_envelope_the_record_carries_is_the_envelope_the_signature_said() {
    // Invariant: the sigstore bundle layer carries the DSSE envelope that
    // the alpha root signed over the capsule digest, and it re-verifies without
    // the service.
    let case = export_good("envelope").await;
    let passport = read_json(
        &case
            .layout
            .join("blobs/sha256")
            .join(blob_hex(&hsp160_passport_digest(&case.layout))),
    );
    let bundle_digest = hsp160_layer_digest(&passport, SIGSTORE_BUNDLE_MEDIA_TYPE);
    let bundle: serde_json::Value =
        serde_json::from_slice(&hsp160_blob(&case.layout, &bundle_digest)).unwrap();
    assert_eq!(
        bundle["mediaType"],
        json!("application/vnd.dev.sigstore.bundle.v0.3+json")
    );
    let envelope: DsseEnvelope =
        serde_json::from_value(bundle["dsseEnvelope"].clone()).expect("a DSSE envelope inside");
    assert_eq!(envelope.payload_type, "application/vnd.in-toto+json");
    assert_eq!(envelope.signatures[0].keyid, "hsp160-root-alpha-v1");

    let reading = read_ok(&case);
    assert_eq!(reading.root.expect("a known root").0, "alpha");
    assert_eq!(reading.signer_fingerprint, case.signer.fingerprint_hex());
}
