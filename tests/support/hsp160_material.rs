// El andamiaje de HSP-160: una procedencia SLSA v1 firmada de verdad.
//
// El pasaporte no puede probarse con un envelope inventado: cada
// veredicto del VSA nace de `cosign_verify` sobre bytes firmados con
// una clave que el lector tambien conoce. Este modulo firma en tiempo
// de prueba con `ring` (ECDSA P-256/SHA-256 ASN.1, el algoritmo que
// verifica `verify_envelope` en hodei-evidence) y no escribe nunca la
// parte privada: de `SignedRoot` solo salen el punto publico y el
// envelope. Si una herramienta externa necesitara la clave privada, el
// propio test la genera y la borra al terminar — aqui no hay `.pem`.
//
// Los helpers se incluyen con `include!` dentro del test que los usa
// (el patron de hsp118_site): en este fichero no hay `use` propios.

/// Un root de confianza con su clave viva solo dentro del proceso del test.
struct SignedRoot {
    /// El id de clave que el envelope declarara.
    key_id: &'static str,
    /// El linaje del root.
    lineage: &'static str,
    /// El emisor al que el deployment ancla este root.
    issuer: &'static str,
    /// El punto publico sin comprimir (65 bytes, empieza con `0x04`).
    point: Vec<u8>,
    /// El par de claves con el que se firma; nunca sale del proceso.
    pair: ring::signature::EcdsaKeyPair,
}

impl SignedRoot {
    /// Una raiz nueva, generada en este instante.
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

    /// La revision del root como la entienden `cosign_verify` y el pasaporte.
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

    /// Las raices con esta clave publicada: el ring del verificador.
    fn trust_roots(&self, revision: RootRevision) -> TrustRoots {
        let mut roots = TrustRoots::new();
        roots.activate(revision, "hrn:hodei:iam:acme:User/ada", "first publication");
        roots
    }

    /// Firma un envelope DSSE cuyo material firmado es exactamente `pae_bytes`.
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

/// El builder que la procedencia declara y la politica confia.
const HSP160_BUILDER: &str = "https://hodei.dev/builder/hsp160-builder@v1";

/// Los instantes fijos de la construccion: el fixture no lee el reloj.
const HSP160_BUILD_STARTED: &str = "2026-03-04T10:00:00Z";
const HSP160_BUILD_FINISHED: &str = "2026-03-04T10:12:00Z";

/// El payload de una procedencia SLSA v1 sobre un sujeto. Los campos van
/// en el orden exacto que la re-serializacion canonica del reader repite
/// byte a byte, de modo que el digest que el VSA nombra es recalculable.
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

/// Los bytes PAE exactos de un payload: `DSSEv1 <len> <type> <len> <payload>`.
fn hsp160_pae_bytes(payload: &serde_json::Value) -> Vec<u8> {
    let payload_bytes = serde_json::to_vec(payload).expect("the statement serializes");
    hodei_evidence::standards::dsse::pae("application/vnd.in-toto+json", &payload_bytes)
}

/// La procedencia firmada sobre el digest de capsula (con prefijo
/// `sha256:`), lista para `PassportInputs::provenance`. El envelope entra
/// como JSON compacto de `to_string` — el mismo que el reader re-seraliza.
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

/// La politica de verificacion con la que se ejercita el pasaporte: ancla
/// el emisor registry.example y declara el builder.
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

/// El instante fijo en el que el pasaporte se exporta y se re-lee.
fn hsp160_verified_at() -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-03-04T12:00:00Z")
        .expect("a fixed instant")
        .with_timezone(&Utc)
}

/// El digest hex del blob `sha256:<hex>` de la capa con ese media type.
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

/// Lee un blob del layout por su digest `sha256:<hex>`.
fn hsp160_blob(layout: &Path, digest: &str) -> Vec<u8> {
    let hex = digest.strip_prefix("sha256:").expect("a sha256 digest");
    std::fs::read(layout.join("blobs/sha256").join(hex)).expect("the blob is in the layout")
}

/// Reescribe un blob del layout sin tocar el manifiesto: el digest que el
/// manifiesto anuncia deja de corresponderse con los bytes. Es la
/// manipulacion que llega al verificador como tampered-blob (el blob
/// miente) o, si el manifiesto ancla su digest, como el digest prometido
/// que ya no cuadra — nunca como una verificacion limpia.
fn hsp160_write_blob(layout: &Path, digest: &str, bytes: &[u8]) {
    let hex = digest.strip_prefix("sha256:").expect("a sha256 digest");
    std::fs::write(layout.join("blobs/sha256").join(hex), bytes).expect("the blob is writable");
}

/// Quita un blob del disco: la evidencia que falta, no la que miente.
fn hsp160_remove_blob(layout: &Path, digest: &str) {
    let hex = digest.strip_prefix("sha256:").expect("a sha256 digest");
    std::fs::remove_file(layout.join("blobs/sha256").join(hex)).expect("the blob was there");
}

/// El digest de la imagen de pasaporte (no la de capsula) buscandolo por
/// su artifactType en el index: las exportaciones no garantizan el orden.
#[allow(dead_code)] // compartido: el carril externo localiza la imagen por su tag
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
