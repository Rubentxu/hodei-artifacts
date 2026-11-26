//! HSP-160 AC2: the passport is read and verified with external
//! tools, without the Hodei service.
//!
//! A deliberately separate lane: here it is not Hodei's code that rules but
//! the system's `oras`, `cosign` and `gpg`. A passport that only Hodei
//! knows how to read is a proprietary format by another name, and the AC says the
//! opposite: the manifest is OCI, the provenance is an in-toto statement
//! with a DSSE envelope, the VSA is SLSA, the SBOM is CycloneDX and
//! the signature is detached OpenPGP. What is checked is the output of
//! the binaries, not that of a parser of our own echoing for them: when
//! a tool is missing, the test is skipped with the exact precondition
//! printed and does NOT declare PASS.
//!
//! Real precondition of each lane (probed on this machine):
//!
//! - `oras` 1.2.0: `manifest fetch`, `resolve` and `pull` with
//!   `--oci-layout <dir>:<tag>`. The layers come out named because
//!   they carry `org.opencontainers.image.title` (the export writes them
//!   with a title). In 1.2.0 `discover` does not follow the `subject` field
//!   over oci-layout: the signature is located by its `latest.sig` tag.
//! - `cosign`: key mode `verify-blob --key <PEM> --signature
//!   <base64|DER>`. Without a transparency log, key mode requires
//!   `--insecure-ignore-tlog=true`, and that is declared: the passport does not
//!   pretend to have a tlog. The signature is verified over `sha256(file)`;
//!   the file is the envelope's PAE, which is exactly the material
//!   that DSSE signs.
//! - `gpg`: batch, keyring in a throwaway `GNUPGHOME`; the detached
//!   signature is verified with `gpg --verify <asc> <data>`.
//!
//! The private key is never stored: the fixtures generate it at
//! test time and the scratch is deleted when done. There is no `.pem`
//! or keys in the repository.

use std::path::{Path, PathBuf};
use std::process::Command;

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
use hodei_evidence::standards::{RootRevision, TrustRoots, VerificationKey};
use hodei_hub::BlobStore;
use hodei_hub::blob::FileBlobStore;
use serde_json::json;

use hodei_artifacts_api::bundles::passport::{
    PASSPORT_ARTIFACT_TYPE, PASSPORT_SIGNATURE_ARTIFACT_TYPE, SBOM_MEDIA_TYPE,
    SIGSTORE_BUNDLE_MEDIA_TYPE,
};
use hodei_artifacts_api::bundles::{
    self, Mapping, PassportInputs, PassportPolicy, ReaderExpectations, SignedDocument,
    export_passport, verify_passport_offline,
};
use hodei_artifacts_api::openpgp::MetadataSigner;

// The graph profile of the harness: a REAL Dgraph namespace per site.
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

const UPSTREAM_URL: &str = "https://repo.example.org/raw/";
const ISSUER: &str = "registry.example";

// The site scaffolding is the package contract's (hsp118): the
// passport the external tools verify is exactly the
// one that `bundles::export` and `export_passport` wrote — not a
// hand-made sample copy for the occasion.
include!("support/hsp118_site.rs");
include!("support/hsp160_material.rs");

fn scratch_signer(tag: &str) -> MetadataSigner {
    MetadataSigner::generate(&format!("HSP-160 {tag} <passport@example.local>")).expect("a key")
}

fn read_json(path: &Path) -> serde_json::Value {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    serde_json::from_slice(&raw).expect("the layout holds JSON")
}

/// A package with a decision, a bare route and a withheld route: the capsule and
/// the passport are exported over real content.
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

/// What the external lane needs after exporting: the live site
/// (its `TempDir` holds the files), the signer, the roots and the
/// policy with which the offline reader can look again, the
/// OCI layout, the passport identity and the capsule identity.
struct ExternalCase {
    _site: Site,
    signer: MetadataSigner,
    roots: TrustRoots,
    policy: PassportPolicy,
    layout: PathBuf,
    /// The passport identity that `export_passport` returned: what
    /// `oras resolve` will have to reproduce.
    image_digest: String,
    /// The digest of the capsule image: the `subject` that both the
    /// passport and the signature promise in standard OCI JSON.
    capsule_digest: String,
}

/// Exports a passport over a real package, with provenance signed
/// by the alpha root anchored to `registry.example`.
async fn external_case(name: &str) -> ExternalCase {
    let (site, _) = three_way_site().await;
    let signer = scratch_signer(name);
    let bundle_dir = export_with(&site, name, Some(&signer)).await;
    let capsule_dir = bundle_dir.join("oci");
    let capsule_digest = {
        let index = read_json(&capsule_dir.join("index.json"));
        index["manifests"][0]["digest"]
            .as_str()
            .expect("the capsule index names its image")
            .to_string()
    };

    let alpha = SignedRoot::generate("hsp160-root-alpha-v1", "alpha", ISSUER);
    let revision = alpha.revision(1, hsp160_verified_at() - Duration::days(30));
    let roots: TrustRoots = alpha.trust_roots(revision.clone());
    let policy = hsp160_policy();
    let inputs = PassportInputs {
        bundle_dir: &bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: capsule_digest.clone(),
        provenance: hsp160_provenance(&alpha, &capsule_digest),
        policy: policy.clone(),
        roots: &roots,
        published_revisions: vec![revision],
        verified_at: hsp160_verified_at(),
    };
    let layout = bundle_dir.join(format!("{name}.passport"));
    let export = export_passport(&inputs, &layout, &signer).expect("the passport exports");
    ExternalCase {
        _site: site,
        signer,
        roots,
        policy,
        layout,
        image_digest: export.image_digest,
        capsule_digest,
    }
}

/// If the tool is not there, the exact precondition is printed and the
/// lane withdraws without pretending. Nothing of our own is run in its place.
fn require(tool: &str, version_args: &[&str]) -> Option<String> {
    match Command::new(tool).args(version_args).output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let line = stdout
                .lines()
                .find(|line| !line.trim().is_empty())
                .or_else(|| stderr.lines().find(|line| !line.trim().is_empty()))
                .unwrap_or("")
                .to_string();
            Some(line)
        }
        _ => {
            eprintln!(
                "SKIP AC2 lane: `{tool}` is not available — precondition: install {tool} in PATH (the lane was probed with oras 1.2.0, cosign's key-mode verify-blob and gnupg 2.x). The external verification lane of HSP-160 does NOT emulate it with the in-repo parser: run the tool, then this target again."
            );
            None
        }
    }
}

fn run(program: &str, args: &[&str]) -> (bool, String) {
    match Command::new(program).args(args).output() {
        Ok(output) => (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ),
        Err(error) => (false, format!("the program could not run: {error}")),
    }
}

/// The ECDSA P-256 public key in PEM (SPKI DER + base64) from the
/// uncompressed point that the bundle declares: it is the form that
/// `cosign --key` accepts. Only the public part, and generated in a
/// scratch that is deleted — no `.pem` ends up in the repository.
fn cosign_key_pem(point: &[u8]) -> String {
    assert_eq!(point.len(), 65, "an uncompressed P-256 point");
    // SubjectPublicKeyInfo: SEQUENCE(SEQUENCE(OID ecPublicKey, OID
    // prime256v1), BIT STRING(unused-bits=0 || point)).
    let mut der = vec![0x30, 0x59, 0x30, 0x13, 0x06, 0x07];
    der.extend_from_slice(&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01]); // 1.2.840.10045.2.1
    der.extend_from_slice(&[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]); // 1.2.840.10045.3.1.7
    der.extend_from_slice(&[0x03, 0x42, 0x00]); // BIT STRING, 0 unused bits
    der.extend_from_slice(point);
    let body = BASE64.encode(&der);
    let mut pem = String::from("-----BEGIN PUBLIC KEY-----\n");
    for line in body.as_bytes().chunks(64) {
        pem.push_str(&String::from_utf8_lossy(line));
        pem.push('\n');
    }
    pem.push_str("-----END PUBLIC KEY-----\n");
    pem
}

/// The passport manifest according to the layout index: the first
/// entry is the passport image, the second the signature.
fn passport_manifest(case: &ExternalCase) -> serde_json::Value {
    let hex = case.image_digest.strip_prefix("sha256:").expect("sha256");
    serde_json::from_slice(
        &std::fs::read(case.layout.join("blobs/sha256").join(hex)).expect("the manifest blob"),
    )
    .expect("the manifest is JSON")
}

#[tokio::test]
async fn ac2_oras_reads_the_passport_as_plain_oci() {
    let _version = match require("oras", &["version"]) {
        Some(text) => text,
        None => return,
    };
    let case = external_case("oras").await;

    // 1. The image is resolved by tag with the OCI standard: `oras` knows
    //    nothing about Hodei and, even so, the identity it sees is the one
    //    the export swore (`export.image_digest`).
    let (ok, text) = run(
        "oras",
        &[
            "resolve",
            "--oci-layout",
            &format!("{}:latest", case.layout.display()),
        ],
    );
    assert!(ok, "oras resolves the passport by tag: {text}");
    let resolved = text.trim();
    let resolved = if resolved.len() == 64 {
        format!("sha256:{resolved}")
    } else {
        resolved.to_string()
    };
    assert_eq!(
        resolved, case.image_digest,
        "oras resolves the exported identity byte for byte"
    );

    // 2. The manifest is standard OCI JSON: `oras manifest fetch` prints
    //    it as is and the artifact type and the subject are read without
    //    a single Hodei parser.
    let (ok, text) = run(
        "oras",
        &[
            "manifest",
            "fetch",
            "--oci-layout",
            &format!("{}:latest", case.layout.display()),
        ],
    );
    assert!(ok, "oras fetches the passport manifest: {text}");
    let manifest: serde_json::Value =
        serde_json::from_str(text.trim()).expect("oras prints exactly the manifest JSON");
    assert_eq!(
        manifest["mediaType"],
        "application/vnd.oci.image.manifest.v1+json"
    );
    assert_eq!(
        manifest["artifactType"], PASSPORT_ARTIFACT_TYPE,
        "the external tool reads the passport's artifact type"
    );
    assert_eq!(
        manifest["subject"]["digest"].as_str().expect("a subject"),
        case.capsule_digest,
        "the passport is a referrer of the capsule, and oras sees it"
    );

    // 3. The signature lives as a referrer with its own artifact type and its
    //    `subject` pointing at the passport: `oras 1.2.0` does not discover
    //    referrers by subject over oci-layout, but the tag
    //    `latest.sig` is standard and the tool checks the binding with the
    //    digest it itself resolved above.
    let (ok, sig_text) = run(
        "oras",
        &[
            "manifest",
            "fetch",
            "--oci-layout",
            &format!("{}:latest.sig", case.layout.display()),
        ],
    );
    assert!(ok, "oras fetches the signature referrer by tag: {sig_text}");
    let signature_manifest: serde_json::Value =
        serde_json::from_str(sig_text.trim()).expect("the signature manifest is JSON");
    assert_eq!(
        signature_manifest["artifactType"], PASSPORT_SIGNATURE_ARTIFACT_TYPE,
        "the signature image is named by its artifact type"
    );
    assert_eq!(
        signature_manifest["subject"]["digest"]
            .as_str()
            .expect("the signature names its subject"),
        case.image_digest,
        "the referrer's subject is the passport identity oras resolved"
    );

    // 4. `oras pull` fetches the named layers and verifies their digests
    //    while copying. What a consumer gets are the standard
    //    files, not an opaque package.
    let out = std::env::temp_dir().join(format!("hsp160-oras-{}-pull", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let (ok, text) = run(
        "oras",
        &[
            "pull",
            "--oci-layout",
            &format!("{}:latest", case.layout.display()),
            "-o",
            out.to_str().expect("utf8 path"),
        ],
    );
    assert!(
        ok,
        "oras pulls every titled layer with its digest verified: {text}"
    );
    let names: Vec<String> = std::fs::read_dir(&out)
        .expect("pulled files")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    for expected in [
        "vsa.json",
        "sbom.cdx.json",
        "provenance.json",
        "provenance.bundle.json",
        "policy.json",
        "trust-roots.json",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "the pull carried {expected}: {names:?}"
        );
    }
    // The VSA that oras pulled is an in-toto statement with the SLSA
    // predicate: an external consumer understands it with its own schema.
    let vsa: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("vsa.json")).expect("the pulled VSA"))
            .expect("the VSA is JSON");
    assert_eq!(vsa["_type"], "https://in-toto.io/Statement/v1");
    assert_eq!(
        vsa["predicateType"], "https://slsa.dev/verification_summary/v1",
        "the VSA carries the standard SLSA predicate type"
    );
    assert!(
        vsa["predicate"]["verificationResult"]
            .as_str()
            .expect("a result")
            .starts_with("SLSA_BUILD_LEVEL_"),
        "the verdict is the SLSA vocabulary, read with no Hodei code"
    );
    // The SBOM that oras pulled is CycloneDX, readable by any
    // supply-chain tool.
    let sbom: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("sbom.cdx.json")).expect("the pulled SBOM"))
            .expect("the SBOM is JSON");
    assert!(
        sbom["bomFormat"]
            .as_str()
            .expect("a format")
            .contains("CycloneDX")
    );
    assert_eq!(sbom["specVersion"], "1.7");
    let _ = std::fs::remove_dir_all(&out);
}

#[tokio::test]
async fn ac2_oras_detects_a_tampered_artifact_and_missing_evidence() {
    let _version = match require("oras", &["version"]) {
        Some(text) => text,
        None => return,
    };
    let case = external_case("oras-tamper").await;

    // The SBOM blob is content-addressed: changing one byte without touching
    // the manifest makes oras ITSELF reject it when copying
    // (probed: "mismatched digest"). Hodei does not decide that — the digest
    // verifier of the external tool does.
    let manifest = passport_manifest(&case);
    let sbom_digest = hsp160_layer_digest(&manifest, SBOM_MEDIA_TYPE);
    let original = hsp160_blob(&case.layout, &sbom_digest);
    let mut tampered = original.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x20; // same size: only the digest can give it away
    hsp160_write_blob(&case.layout, &sbom_digest, &tampered);

    let out = std::env::temp_dir().join(format!("hsp160-oras-{}-bad", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let (ok, text) = run(
        "oras",
        &[
            "pull",
            "--oci-layout",
            &format!("{}:latest", case.layout.display()),
            "-o",
            out.to_str().expect("utf8 path"),
        ],
    );
    assert!(!ok, "oras refuses the tampered blob: {text}");
    assert!(
        text.contains("digest"),
        "the refusal names the digest, externally: {text}"
    );

    // Evidence that is missing: removing the blob from the layout is `not found`
    // for oras — the absence is reported by the tool before
    // anyone interprets the VSA.
    hsp160_write_blob(&case.layout, &sbom_digest, &original);
    hsp160_remove_blob(&case.layout, &sbom_digest);
    let out = std::env::temp_dir().join(format!("hsp160-oras-{}-gone", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let (ok, text) = run(
        "oras",
        &[
            "pull",
            "--oci-layout",
            &format!("{}:latest", case.layout.display()),
            "-o",
            out.to_str().expect("utf8 path"),
        ],
    );
    assert!(!ok, "oras reports the missing evidence: {text}");
    assert!(
        text.contains("not found"),
        "oras says not found, externally: {text}"
    );
    let _ = std::fs::remove_dir_all(&out);
}

#[tokio::test]
async fn ac2_cosign_verifies_the_envelope_the_bundle_publishes() {
    let _version = match require("cosign", &["version"]) {
        Some(text) => text,
        None => return,
    };
    let case = external_case("cosign").await;

    // The DSSE envelope comes out of the sigstore bundle layer of the layout
    // — byte for byte, not from a side copy of the test.
    let manifest = passport_manifest(&case);
    let bundle_digest = hsp160_layer_digest(&manifest, SIGSTORE_BUNDLE_MEDIA_TYPE);
    let bundle: serde_json::Value =
        serde_json::from_slice(&hsp160_blob(&case.layout, &bundle_digest))
            .expect("the bundle JSON");
    assert_eq!(
        bundle["mediaType"], "application/vnd.dev.sigstore.bundle.v0.3+json",
        "the bundle is the sigstore v0.3 shape any cosign understands"
    );
    let envelope = &bundle["dsseEnvelope"];
    assert_eq!(envelope["payloadType"], "application/vnd.in-toto+json");
    let payload: serde_json::Value = serde_json::from_slice(
        &BASE64
            .decode(envelope["payload"].as_str().expect("base64 payload"))
            .expect("payload bytes"),
    )
    .expect("the in-toto statement");
    assert_eq!(
        payload["predicateType"], "https://slsa.dev/provenance/v1",
        "the statement under the envelope is real SLSA v1 provenance"
    );

    // cosign verify-blob (key mode, no tlog) verifies the signature over
    // `sha256(file)`. The file given to it is the PAE bytes of the
    // envelope — the exact material that DSSE signs: `ring` signed
    // `ECDSA(SHA256(PAE))` and cosign expects that form. The PEM key is
    // built from the point that the bundle itself declares: if the bundle
    // lied about its key, cosign would say so.
    let signature = BASE64
        .decode(
            envelope["signatures"][0]["sig"]
                .as_str()
                .expect("base64 sig"),
        )
        .expect("signature bytes");
    let point_hex = bundle["verificationMaterial"]["publicKey"]["ecdsaP256UncompressedPointHex"]
        .as_str()
        .expect("the point hex")
        .to_string();
    let point: Vec<u8> = (0..point_hex.len() / 2)
        .map(|i| u8::from_str_radix(&point_hex[2 * i..2 * i + 2], 16).expect("hex byte"))
        .collect();

    let work = std::env::temp_dir().join(format!("hsp160-cosign-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).expect("a scratch dir");
    let pae = hsp160_pae_bytes(&payload);
    std::fs::write(work.join("pae"), &pae).expect("the PAE bytes");
    std::fs::write(work.join("key.pem"), cosign_key_pem(&point)).expect("the public PEM");
    std::fs::write(work.join("signature.b64"), BASE64.encode(&signature)).expect("the signature");

    let verify = |message: &str| {
        run(
            "cosign",
            &[
                "verify-blob",
                "--key",
                work.join("key.pem").to_str().expect("utf8"),
                "--signature",
                work.join("signature.b64").to_str().expect("utf8"),
                "--insecure-ignore-tlog=true",
                message,
            ],
        )
    };

    let (ok, text) = verify(work.join("pae").to_str().expect("utf8"));
    assert!(
        ok,
        "cosign verifies the envelope with its own key mode over the PAE: {text}"
    );
    assert!(
        text.contains("Verified OK"),
        "cosign itself signs off: {text}"
    );

    // And the same cosign rejects the altered message: one byte of the PAE and
    // the envelope signature no longer corresponds. Detecting the
    // tampering is the tool's doing, not an exception of ours.
    let mut tampered = pae.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    std::fs::write(work.join("pae-tampered"), &tampered).expect("the tamper lands");
    let (ok, text) = verify(work.join("pae-tampered").to_str().expect("utf8"));
    assert!(!ok, "cosign refuses the tampered PAE: {text}");

    // And with the wrong key: another key generated right here. The
    // bundle cannot point at a root that did not sign.
    let other = SignedRoot::generate("hsp160-root-omega-v1", "omega", ISSUER);
    std::fs::write(work.join("other.pem"), cosign_key_pem(&other.point)).expect("the wrong PEM");
    let (ok, text) = run(
        "cosign",
        &[
            "verify-blob",
            "--key",
            work.join("other.pem").to_str().expect("utf8"),
            "--signature",
            work.join("signature.b64").to_str().expect("utf8"),
            "--insecure-ignore-tlog=true",
            work.join("pae").to_str().expect("utf8"),
        ],
    );
    assert!(!ok, "cosign refuses a key that did not sign: {text}");
    let _ = std::fs::remove_dir_all(&work);
}

#[tokio::test]
async fn ac2_gpg_verifies_the_detached_signature_over_the_manifest() {
    let _version = match require("gpg", &["--version"]) {
        Some(text) => text,
        None => return,
    };
    let case = external_case("gpg").await;

    // The detached signature lives in the `latest.sig` referrer layer; the
    // signed material is the exact bytes of the passport
    // manifest. A consumer with the layout and the hub's public key
    // verifies this with their usual gnupg.
    let index = read_json(&case.layout.join("index.json"));
    let entry = index["manifests"]
        .as_array()
        .expect("manifests")
        .iter()
        .find(|item| item["annotations"]["org.opencontainers.image.ref.name"] == "latest.sig")
        .expect("the signature referrer is tagged");
    let signature_manifest: serde_json::Value = serde_json::from_slice(&hsp160_blob(
        &case.layout,
        entry["digest"].as_str().expect("a digest"),
    ))
    .expect("the signature manifest");
    assert_eq!(
        signature_manifest["artifactType"],
        PASSPORT_SIGNATURE_ARTIFACT_TYPE
    );
    let signature_doc: serde_json::Value = serde_json::from_slice(&hsp160_blob(
        &case.layout,
        signature_manifest["layers"][0]["digest"]
            .as_str()
            .expect("the signature blob"),
    ))
    .expect("the signature document");
    assert_eq!(signature_doc["format"], "openpgp-detached");
    assert_eq!(
        signature_doc["signer"].as_str().expect("a fingerprint"),
        case.signer.fingerprint_hex(),
        "the fingerprint the layout declares is the signer's own"
    );
    let armor = signature_doc["signature"]
        .as_str()
        .expect("the armored signature");

    let signed = std::fs::read(
        case.layout
            .join("blobs/sha256")
            .join(case.image_digest.strip_prefix("sha256:").expect("sha256")),
    )
    .expect("the signed manifest bytes");

    let work = std::env::temp_dir().join(format!("hsp160-gpg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("gnupg")).expect("a scratch gpg home");
    std::fs::write(work.join("manifest"), &signed).expect("manifest bytes");
    std::fs::write(work.join("manifest.asc"), armor).expect("the detached signature");
    std::fs::write(
        work.join("key.asc"),
        case.signer.key_asc().expect("the public key in armor"),
    )
    .expect("the public key");

    let gpg_home = work.join("gnupg");
    // LC_ALL=C: the lane needs gpg's technical phrase ("Good
    // signature"), not the system's translation. And the home at 0700,
    // since gpg complains about insecure permissions.
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&gpg_home, std::fs::Permissions::from_mode(0o700))
        .expect("a private gpg home");
    let mut import = Command::new("gpg");
    import.env("GNUPGHOME", &gpg_home).env("LC_ALL", "C").args([
        "--batch",
        "--quiet",
        "--import",
        work.join("key.asc").to_str().expect("utf8"),
    ]);
    let output = import.output().expect("gpg import runs");
    assert!(output.status.success(), "gpg imports the hub's public key");

    let mut verify = Command::new("gpg");
    verify.env("GNUPGHOME", &gpg_home).env("LC_ALL", "C").args([
        "--batch",
        "--quiet",
        "--verify",
        work.join("manifest.asc").to_str().expect("utf8"),
        work.join("manifest").to_str().expect("utf8"),
    ]);
    let output = verify.output().expect("gpg verify runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "gpg verifies the detached signature: {text}"
    );
    assert!(
        text.contains("Good signature"),
        "gpg says it itself: {text}"
    );

    // The same signature over other bytes is rejected by gpg: what it
    // authenticated was THIS manifest, not any manifest.
    std::fs::write(work.join("manifest"), b"{}").expect("other bytes land");
    let mut verify = Command::new("gpg");
    verify.env("GNUPGHOME", &gpg_home).env("LC_ALL", "C").args([
        "--batch",
        "--quiet",
        "--verify",
        work.join("manifest.asc").to_str().expect("utf8"),
        work.join("manifest").to_str().expect("utf8"),
    ]);
    let output = verify.output().expect("gpg verify runs");
    assert!(
        !output.status.success(),
        "gpg refuses the forged manifest bytes"
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[tokio::test]
async fn ac2_the_layout_the_external_tools_read_is_the_one_hodei_reads_offline() {
    let _version = match require("oras", &["version"]) {
        Some(text) => text,
        None => return,
    };
    let case = external_case("cross").await;

    // The external lane and the offline read of the same layout are not
    // two sources of truth: `oras resolve` reproduces the identity that
    // `verify_passport_offline` accepts, with the same roots and the same
    // policy, without touching the network.
    let (ok, text) = run(
        "oras",
        &[
            "resolve",
            "--oci-layout",
            &format!("{}:latest", case.layout.display()),
        ],
    );
    assert!(ok, "the external lane resolves the identity: {text}");
    let resolved = text.trim();
    let resolved = if resolved.len() == 64 {
        format!("sha256:{resolved}")
    } else {
        resolved.to_string()
    };

    let reading = verify_passport_offline(
        &case.layout,
        &case.signer,
        &ReaderExpectations {
            roots: &case.roots,
            policy: &case.policy,
            issuer: Some(ISSUER.to_string()),
            online_freshness: None,
        },
        hsp160_verified_at(),
    )
    .expect("the same bytes the tools read, the offline reader accepts");
    assert_eq!(
        reading.passport_image_digest, resolved,
        "external and offline agree on identity"
    );
    assert!(reading.cryptographic_holds, "and on the verdict");
}
