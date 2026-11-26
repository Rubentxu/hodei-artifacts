// HSP-160 scaffolding: a genuinely signed SLSA v1 provenance.
//
// The passport cannot be tested with an invented envelope: every
// VSA verdict comes from `cosign_verify` over bytes signed with
// a key the reader also knows. This module signs at test time
// with `ring` (ECDSA P-256/SHA-256 ASN.1, the algorithm that
// `verify_envelope` in hodei-evidence verifies) and never writes the
// private half: from `SignedRoot` only the public point and the
// envelope come out. If an external tool needed the private key, the
// test itself generates it and deletes it when done — there is no `.pem` here.
//
// The helpers are pulled in with `include!` inside the test that uses them
// (the hsp118_site pattern): this file has no `use` of its own.

/// A trust root with its live key only inside the test process.
struct SignedRoot {
    /// The key id the envelope will declare.
    key_id: &'static str,
    /// The root's lineage.
    lineage: &'static str,
    /// The issuer to which the deployment anchors this root.
    issuer: &'static str,
    /// The uncompressed public point (65 bytes, starts with `0x04`).
    point: Vec<u8>,
    /// The key pair used for signing; it never leaves the process.
    pair: ring::signature::EcdsaKeyPair,
}

impl SignedRoot {
    /// A fresh root, generated at this instant.
    fn generate(key_id: &'static str, lineage: &'static str, issuer: &'static str) -> Self {
        use ring::signature::KeyPair as _;
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::EcdsaKeyPair::generate_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            &rng,
        )
        .expect("a throwaway P-256 key can be generated");
        let pair = ring::signature::EcdsaKeyPair::from_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            pkcs8.as_ref(),
            &rng,
        )
        .expect("the generated PKCS#8 is readable");
        let point = pair.public_key().as_ref().to_vec();
        assert_eq!(point.len(), 65, "an uncompressed P-256 point is 65 bytes");
        assert_eq!(point[0], 0x04, "and starts with 0x04");
        SignedRoot {
            key_id,
            lineage,
            issuer,
            point,
            pair,
        }
    }

    /// The root revision as understood by `cosign_verify` and the passport.
    fn revision(&self, revision: u32, activated_at: chrono::DateTime<Utc>) -> RootRevision {
        RootRevision {
            lineage: self.lineage.to_string(),
            revision,
            key: VerificationKey {
                key_id: self.key_id.to_string(),
                point: self.point.clone(),
            },
            issuer: Some(self.issuer.to_string()),
            activated_at,
            retired_at: None,
            revoked_at: None,
        }
    }

    /// The roots with this key published: the verifier's ring.
    fn trust_roots(&self, revision: RootRevision) -> TrustRoots {
        let mut roots = TrustRoots::new();
        roots.activate(revision, "hrn:hodei:iam:acme:User/ada", "first publication");
        roots
    }

    /// Signs a DSSE envelope whose signed material is exactly `pae_bytes`.
    fn sign_pae(&self, payload: serde_json::Value, pae_bytes: &[u8]) -> String {
        let rng = ring::rand::SystemRandom::new();
        let signature = self
            .pair
            .sign(&rng, pae_bytes)
            .expect("the throwaway key signs the PAE");
        let envelope = serde_json::json!({
            "payloadType": "application/vnd.in-toto+json",
            "payload": BASE64.encode(serde_json::to_vec(&payload).expect("statement JSON")),
            "signatures": [{"keyid": self.key_id, "sig": BASE64.encode(signature.as_ref())}],
        });
        serde_json::to_string(&envelope).expect("the envelope is JSON")
    }
}

/// The builder the provenance declares and the policy trusts.
const HSP160_BUILDER: &str = "https://hodei.dev/builder/hsp160-builder@v1";

/// The fixed instants of the build: the fixture does not read the clock.
const HSP160_BUILD_STARTED: &str = "2026-03-04T10:00:00Z";
const HSP160_BUILD_FINISHED: &str = "2026-03-04T10:12:00Z";

/// The payload of a SLSA v1 provenance over a subject. The fields go
/// in the exact order that the reader's canonical re-serialization repeats
/// byte for byte, so the digest the VSA names is recomputable.
fn hsp160_statement(subject_name: &str, subject_sha256_hex: &str) -> serde_json::Value {
    serde_json::json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": [{
            "name": subject_name,
            "digest": { "sha256": subject_sha256_hex },
        }],
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
            "buildDefinition": {
                "buildType": HSP160_BUILDER,
                "resolvedDependencies": [],
            },
            "runDetails": {
                "builder": { "id": HSP160_BUILDER },
                "metadata": {
                    "invocationId": "hsp160-invocation-01",
                    "startedOn": HSP160_BUILD_STARTED,
                    "finishedOn": HSP160_BUILD_FINISHED,
                },
            },
        },
    })
}

/// The exact PAE bytes of a payload: `DSSEv1 <len> <type> <len> <payload>`.
fn hsp160_pae_bytes(payload: &serde_json::Value) -> Vec<u8> {
    let payload_bytes = serde_json::to_vec(payload).expect("the statement serializes");
    hodei_evidence::standards::dsse::pae("application/vnd.in-toto+json", &payload_bytes)
}

/// The provenance signed over the capsule digest (with the
/// `sha256:` prefix), ready for `PassportInputs::provenance`. The envelope enters
/// as compact JSON from `to_string` — the same one the reader re-serializes.
fn hsp160_provenance(root: &SignedRoot, capsule_digest: &str) -> SignedDocument {
    let hex = capsule_digest.strip_prefix("sha256:").expect("a sha256 capsule digest");
    let statement = hsp160_statement("capsule", hex);
    let envelope_json = root.sign_pae(statement.clone(), &hsp160_pae_bytes(&statement));
    SignedDocument {
        envelope_json,
        key_id: root.key_id.to_string(),
        public_point: root.point.clone(),
    }
}

/// The verification policy used to exercise the passport: it anchors
/// the registry.example issuer and declares the builder.
fn hsp160_policy() -> PassportPolicy {
    PassportPolicy {
        uri: "https://policies.example/hsp160/trust-passport".to_string(),
        version: "1".to_string(),
        trusted_builders: vec![HSP160_BUILDER.to_string()],
        pinned_issuer: Some("registry.example".to_string()),
        builder_identity_proven: true,
        hermetic_declared_complete: true,
    }
}

/// The fixed instant at which the passport is exported and re-read.
fn hsp160_verified_at() -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-03-04T12:00:00Z")
        .expect("a fixed instant")
        .with_timezone(&Utc)
}

/// The hex digest of the `sha256:<hex>` blob of the layer with that media type.
fn hsp160_layer_digest(manifest: &serde_json::Value, media_type: &str) -> String {
    manifest["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|layer| layer["mediaType"] == media_type)
        .unwrap_or_else(|| panic!("there is a {media_type} layer"))["digest"]
        .as_str()
        .expect("a digest")
        .to_string()
}

/// Reads a blob from the layout by its `sha256:<hex>` digest.
fn hsp160_blob(layout: &Path, digest: &str) -> Vec<u8> {
    let hex = digest.strip_prefix("sha256:").expect("a sha256 digest");
    std::fs::read(layout.join("blobs/sha256").join(hex)).expect("the blob is in the layout")
}

/// Rewrites a blob in the layout without touching the manifest: the digest the
/// manifest advertises no longer matches the bytes. This is the
/// tampering that reaches the verifier as tampered-blob (the blob
/// lies) or, if the manifest anchors its digest, as the promised digest
/// no longer matching — never as a clean verification.
fn hsp160_write_blob(layout: &Path, digest: &str, bytes: &[u8]) {
    let hex = digest.strip_prefix("sha256:").expect("a sha256 digest");
    std::fs::write(layout.join("blobs/sha256").join(hex), bytes).expect("the blob is writable");
}

/// Removes a blob from disk: the evidence that is missing, not the one that lies.
fn hsp160_remove_blob(layout: &Path, digest: &str) {
    let hex = digest.strip_prefix("sha256:").expect("a sha256 digest");
    std::fs::remove_file(layout.join("blobs/sha256").join(hex)).expect("the blob was there");
}

/// The digest of the passport image (not the capsule one), found by
/// its artifactType in the index: exports do not guarantee the order.
#[allow(dead_code)] // shared: the external lane locates the image by its tag
fn hsp160_passport_digest(layout: &Path) -> String {
    let index = read_json(&layout.join("index.json"));
    for entry in index["manifests"].as_array().expect("manifests") {
        let digest = entry["digest"].as_str().expect("a digest");
        let manifest: serde_json::Value =
            serde_json::from_slice(&hsp160_blob(layout, digest)).expect("a manifest");
        if manifest["artifactType"] == PASSPORT_ARTIFACT_TYPE {
            return digest.to_string();
        }
    }
    panic!("the layout carries no passport image");
}
