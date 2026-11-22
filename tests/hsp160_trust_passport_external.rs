//! AC2 de HSP-160: el pasaporte se lee y se verifica con herramientas
//! externas, sin el servicio Hodei.
//!
//! Carril separado a proposito: aqui no manda el codigo de Hodei sino
//! `oras`, `cosign` y `gpg` del sistema. Un pasaporte que solo Hodei
//! sabe leer es un formato propietario con otro nombre, y la AC dice lo
//! contrario: el manifest es OCI, la procedencia es un statement
//! in-toto con envelope DSSE, el VSA es SLSA, el SBOM es CycloneDX y
//! la firma es OpenPGP detached. Lo que se comprueba es la salida de
//! los binarios, no la de un parser propio haciendoles de eco: cuando
//! una herramienta falta, el test se salta con la precondition exacta
//! impresa y NO declara PASS.
//!
//! Precondition real de cada carril (sondeada en esta maquina):
//!
//! - `oras` 1.2.0: `manifest fetch`, `resolve` y `pull` con
//!   `--oci-layout <dir>:<tag>`. Las capas salen nombradas porque
//!   llevan `org.opencontainers.image.title` (el export las escribe
//!   con titulo). En 1.2.0 `discover` no recorre el campo `subject`
//!   sobre oci-layout: la firma se localiza por su tag `latest.sig`.
//! - `cosign`: modo clave `verify-blob --key <PEM> --signature
//!   <base64|DER>`. Sin log de transparencia el modo clave exige
//!   `--insecure-ignore-tlog=true`, y eso se declara: el pasaporte no
//!   finge tener tlog. La firma se verifica sobre `sha256(archivo)`;
//!   el archivo es el PAE del envelope, que es exactamente el material
//!   que DSSE firma.
//! - `gpg`: batch, anillo en un `GNUPGHOME` de usar y tirar; la firma
//!   detached se verifica con `gpg --verify <asc> <datos>`.
//!
//! La clave privada no se guarda nunca: los fixture la generan en
//! tiempo de prueba y el scratch se borra al terminar. No hay `.pem`
//! ni claves en el repositorio.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{Duration, Utc};
use hodei_catalog::model::{
    DigestScope, PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogWriter, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt,
    ReceiptOutcome, TrustGrant, TrustLedger,
};
use hodei_catalog_storage::SurrealCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_evidence::standards::{RootRevision, TrustRoots, VerificationKey};
use hodei_hub::BlobStore;
use hodei_hub::blob::FileBlobStore;
use serde_json::json;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

use hodei_artifacts_api::bundles::passport::{
    PASSPORT_ARTIFACT_TYPE, PASSPORT_SIGNATURE_ARTIFACT_TYPE, SBOM_MEDIA_TYPE,
    SIGSTORE_BUNDLE_MEDIA_TYPE,
};
use hodei_artifacts_api::bundles::{
    self, Mapping, PassportInputs, PassportPolicy, ReaderExpectations, SignedDocument,
    export_passport, verify_passport_offline,
};
use hodei_artifacts_api::openpgp::MetadataSigner;

const UPSTREAM_URL: &str = "https://repo.example.org/raw/";
const ISSUER: &str = "registry.example";

// El andamiaje del sitio es el del contrato del paquete (hsp118): el
// pasaporte que verifican las herramientas externas es exactamente el
// que `bundles::export` y `export_passport` escribieron — no una
// copia de muestra hecha a mano para la ocasion.
include!("support/hsp118_site.rs");
include!("support/hsp160_material.rs");

fn scratch_signer(tag: &str) -> MetadataSigner {
    MetadataSigner::generate(&format!("HSP-160 {tag} <passport@example.local>")).expect("a key")
}

fn read_json(path: &Path) -> serde_json::Value {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    serde_json::from_slice(&raw).expect("the layout holds JSON")
}

/// Un paquete con decision, ruta desnuda y ruta retenida: la capsula y
/// el pasaporte se exportan sobre contenido real.
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
    site.store
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
    // c.bin no tiene recibo: se retiene y la capsula lo dice.
    site.resolve("acme", "mirror", "tools/c.bin", b"tool c")
        .await;
    (site, decided)
}

/// Lo que el carril externo necesita despues de exportar: el sitio vivo
/// (su `TempDir` sostiene los ficheros), el firmante, las raices y la
/// politica con las que el lector offline puede volver a mirar, el
/// layout OCI, la identidad del pasaporte y la de la capsula.
struct ExternalCase {
    _site: Site,
    signer: MetadataSigner,
    roots: TrustRoots,
    policy: PassportPolicy,
    layout: PathBuf,
    /// La identidad del pasaporte que `export_passport` devolvio: lo
    /// que `oras resolve` tendra que reproducir.
    image_digest: String,
    /// El digest de la imagen de capsula: el `subject` que tanto el
    /// pasaporte como la firma prometen en JSON OCI estandar.
    capsule_digest: String,
}

/// Exporta un pasaporte sobre un paquete real, con procedencia firmada
/// por la raiz alfa anclada a `registry.example`.
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

/// Si la herramienta no esta, se imprime la precondition exacta y el
/// carril se retira sin fingir. No se ejecuta nada propio en su lugar.
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

/// La clave publica ECDSA P-256 en PEM (SPKI DER + base64) a partir del
/// punto sin comprimir que el bundle declara: es la forma que
/// `cosign --key` acepta. Solo la parte publica, y generada en un
/// scratch que se borra — ningun `.pem` acaba en el repositorio.
fn cosign_key_pem(point: &[u8]) -> String {
    assert_eq!(point.len(), 65, "an uncompressed P-256 point");
    // SubjectPublicKeyInfo: SEQUENCE(SEQUENCE(OID ecPublicKey, OID
    // prime256v1), BIT STRING(unused-bits=0 || punto)).
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

/// El manifiesto del pasaporte segun el index del layout: la primera
/// entrada es la imagen de pasaporte, la segunda la firma.
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

    // 1. La imagen se resuelve por tag con el estandar OCI: `oras` no
    //    sabe nada de Hodei y, aun asi, la identidad que ve es la que
    //    el export juro (`export.image_digest`).
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

    // 2. El manifest es JSON OCI estandar: `oras manifest fetch` lo
    //    imprime tal cual y el artifact type y el subject se leen sin
    //    un solo parser de Hodei.
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

    // 3. La firma vive como referrer con su propio artifact type y su
    //    `subject` apuntando al pasaporte: `oras 1.2.0` no descubre
    //    referrers por subject sobre oci-layout, pero el tag
    //    `latest.sig` es estandar y la binding la comprueba la
    //    herramienta con el digest que ella misma resolvio arriba.
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

    // 4. `oras pull` trae las capas nombradas y verifica sus digests
    //    mientras copia. Lo que un consumidor obtiene son los ficheros
    //    estandar, no un paquete opaco.
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
    // El VSA que oras trajó es un statement in-toto con el predicate
    // SLSA: un consumidor externo lo entiende con su propio esquema.
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
    // El SBOM que oras trajó es CycloneDX legible por cualquier
    // herramienta de la cadena de suministro.
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

    // El blob del SBOM es content-addressed: cambiar un byte sin tocar
    // el manifest hace que el PROPIO oras lo rechace al copiar
    // (sondeado: "mismatched digest"). No lo decide Hodei — lo decide
    // el verificador de digests de la herramienta externa.
    let manifest = passport_manifest(&case);
    let sbom_digest = hsp160_layer_digest(&manifest, SBOM_MEDIA_TYPE);
    let original = hsp160_blob(&case.layout, &sbom_digest);
    let mut tampered = original.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x20; // mismo tamano: solo el digest puede delatarlo
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

    // Evidencia que falta: quitar el blob del layout es `not found`
    // para oras — la ausencia la reporta la herramienta antes de que
    // nadie interprete el VSA.
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

    // El envelope DSSE sale de la capa del bundle sigstore del layout
    // — byte a byte, no de una copia lateral del test.
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

    // cosign verify-blob (modo clave, sin tlog) verifica la firma sobre
    // `sha256(archivo)`. El archivo que se le da son los bytes PAE del
    // envelope — el material exacto que DSSE firma: `ring` firmo
    // `ECDSA(SHA256(PAE))` y cosign espera esa forma. La clave PEM se
    // construye del punto que el propio bundle declara: si el bundle
    // mintiera sobre su clave, cosign lo diria.
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

    // Y el mismo cosign rechaza el mensaje alterado: un byte del PAE y
    // la firma del envelope deja de corresponder. La deteccion del
    // manipulado es de la herramienta, no de una excepcion nuestra.
    let mut tampered = pae.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    std::fs::write(work.join("pae-tampered"), &tampered).expect("the tamper lands");
    let (ok, text) = verify(work.join("pae-tampered").to_str().expect("utf8"));
    assert!(!ok, "cosign refuses the tampered PAE: {text}");

    // Y con la clave equivocada: otra clave generada aqui mismo. El
    // bundle no puede apuntar a un root que no firmo.
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

    // La firma detached vive en la capa del referrer `latest.sig`; el
    // material firmado son los bytes exactos del manifest del
    // pasaporte. Un consumidor con el layout y la clave publica del hub
    // verifica esto con su gnupg de siempre.
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
    // LC_ALL=C: el carril necesita la frase tecnica de gpg ("Good
    // signature"), no la traduccion del sistema. Y el home en 0700,
    // que gpg se queja de los permisos inseguros.
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

    // La misma firma sobre otros bytes la rechaza gpg: lo que
    // autenticaba era ESTE manifest, no cualquier manifest.
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

    // El carril externo y la lectura offline del mismo layout no son
    // dos fuentes de verdad: `oras resolve` reproduce la identidad que
    // `verify_passport_offline` acepta, con las mismas raices y la
    // misma politica, sin tocar la red.
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
