//! HSP-235: the RPM proxy serves the upstream's repodata **as is**.
//!
//! An rpm repository in proxy mode does not rebuild foreign repodata: it
//! fetches it from its upstream and serves it byte for byte, with the upstream's signature
//! and with the upstream's key. What is measured here, against the real
//! service:
//!
//! - the `repomd.xml` that is served is **the upstream's**, not a rebuilt
//! document;
//! - the signature that is served is the upstream's, and the key that is served is
//! the upstream's, not the hub's;
//! - the metadata is served **only** if it meets the checksum it declares: the
//! one its own name announces, or the one the cached `repomd`
//! declares;
//! - a metadata that does not meet its checksum is **not** served, and neither is one that the
//! `repomd` does not declare;
//! - a path outside the upstream's scopes does not exist for that proxy;
//! - without network, the cache serves what was already downloaded and does not invent the rest;
//! - the trust gate is also applied to the proxy path.
//!
//! The upstream is a local server that this file controls: the
//! `repomd` it signs, the primary it declares and the corrupt metadata that
//! does not meet what its name announces.

mod support;

use std::io::Write as _;

use hodei_artifacts_api::openpgp::MetadataSigner;
use hodei_catalog::yum;
use serde_json::{Value, json};
use sha2::Digest as _;
use support::lab::{Expectation, LabCase, Scope, describe_registry_lab, run_registry_lab_case};
use support::{Answer, Curl, Deployment, FIRST_ACCOUNT};

/// The proxy repository on the open channel.
const LAB: &str = "rpm-lab";

/// The proxy repository on the **trusted** channel: what separates them is the
/// channel, and nothing else.
const TRUSTED: &str = "rpm-trusted";

/// The package the upstream publishes in its usual layout.
const PACKAGE_PATH: &str = "Packages/hello-hodei-1.0.0-1.fc40.x86_64.rpm";

/// The package's bytes: **synthetic**. What is measured in this
/// increment is the HTTP path, the checksum and the generation's
/// coherence, not the rpm format: a real package needs
/// `rpmbuild`, which is not here.
const PACKAGE: &[u8] =
    b"a synthetic rpm: the path, the checksum and the generation are what is measured";

/// The revision of the upstream's repodata.
const REVISION: u64 = 1_700_000_000;

// ---------------------------------------------------------------------------
// The cases
// ---------------------------------------------------------------------------

const CASE_REPOMD: &str = "ac1-the-repomd-of-the-proxy-is-the-one-of-the-upstream";
const TITLE_REPOMD: &str = "el repomd que sirve el proxy es el del upstream, byte a byte";
const CASE_SIGNATURE: &str = "ac1-the-signature-served-is-the-one-of-the-upstream";
const TITLE_SIGNATURE: &str = "la firma que se sirve es la del upstream, no la del hub";
const CASE_KEY: &str = "ac1-the-key-served-is-the-key-of-the-upstream";
const TITLE_KEY: &str = "la clave que se sirve es la del upstream, no la del hub";
const CASE_METADATA: &str = "ac1-the-metadata-of-the-upstream-is-served-with-its-checksum";
const TITLE_METADATA: &str =
    "el metadata del upstream se sirve con el checksum que su nombre declara";
const CASE_CORRUPT: &str = "ac2-metadata-that-does-not-match-its-checksum-is-not-served";
const TITLE_CORRUPT: &str = "un metadata que no cumple el checksum de su nombre no se sirve";
const CASE_UNDECLARED: &str = "ac2-metadata-that-the-repomd-does-not-declare-is-not-served";
const TITLE_UNDECLARED: &str = "un metadata que el repomd no declara no se sirve";
const CASE_PACKAGE: &str = "ac2-the-package-of-the-upstream-is-served";
const TITLE_PACKAGE: &str = "el paquete del upstream se sirve con sus bytes";
const CASE_SCOPE: &str = "ac3-a-path-out-of-the-scope-is-not-served";
const TITLE_SCOPE: &str = "una ruta fuera de los scopes del upstream no se sirve";
const CASE_OFFLINE: &str = "ac3-offline-serves-the-cached-copy-and-not-more";
const TITLE_OFFLINE: &str = "sin red, la cache sirve lo descargado y no inventa lo demas";
const CASE_TRUST: &str = "ac3-the-trust-gate-applies-to-the-proxy-content";
const TITLE_TRUST: &str = "un proxy en el canal trusted no sirve lo que no tiene concesion";

/// The cases a complete run has to execute.
///
/// They are declared **before** running: a run that only reached some
/// says which did not run, and a report with a case that never ran is
/// green.
const THE_CASES: &[(&str, &str)] = &[
    (CASE_REPOMD, TITLE_REPOMD),
    (CASE_SIGNATURE, TITLE_SIGNATURE),
    (CASE_KEY, TITLE_KEY),
    (CASE_METADATA, TITLE_METADATA),
    (CASE_CORRUPT, TITLE_CORRUPT),
    (CASE_UNDECLARED, TITLE_UNDECLARED),
    (CASE_PACKAGE, TITLE_PACKAGE),
    (CASE_SCOPE, TITLE_SCOPE),
    (CASE_OFFLINE, TITLE_OFFLINE),
    (CASE_TRUST, TITLE_TRUST),
];

/// Runs a lab case.
fn a_case(
    id: &str,
    title: &str,
    scope: Scope,
    expectation: Expectation,
    body: impl FnOnce() -> LabCase,
) {
    describe_registry_lab(
        "Laboratorio del proxy rpm — informe",
        &[
            "- **Un upstream, no una red.** El upstream de distribucion es el servidor local del laboratorio: lo que se mide es el camino del proxy rpm con su checksum y su generacion, no la red de una distribucion real.",
            "- **El paquete es sintetico.** Los bytes del `.rpm` no son un paquete instalable: lo que se mide en este incremento es el camino HTTP, el checksum y la coherencia de la generacion. El checksum del **paquete** es cosa del incremento siguiente.",
            "- **El cliente real `dnf` no participa.** No hay `createrepo_c` en la maquina, asi que el `repomd` lo fabrica el propio laboratorio con los constructores del repo. El cliente `dnf` de verdad es cosa del incremento siguiente.",
        ],
    );
    run_registry_lab_case(THE_CASES, id, title, scope, expectation, body);
}

// ---------------------------------------------------------------------------
// The lab
// ---------------------------------------------------------------------------

/// The lab's deployment: the fake upstream, the keys and what
/// the upstream publishes.
struct Lab {
    deployment: Deployment,
    upstream: support::lab::Upstream,
    /// The upstream's public key, in armor: the one the proxy serves.
    upstream_key: String,
    /// The hub's public key, in armor: the one the proxy must **not**
    /// serve.
    hub_key: String,
    /// The hub's private key, armored: the service boots with it.
    hub_armor: String,
    /// The upstream's `repomd.xml`.
    repomd: Vec<u8>,
    /// The `repomd.xml` signature, made by the upstream's key.
    signature: Vec<u8>,
    /// The upstream's compressed primary.
    primary_gz: Vec<u8>,
    /// The SHA256 of the compressed primary: the name it is requested by.
    primary_sha: String,
}

/// The sha256 of some bytes, in hexadecimal.
fn sha256_of(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The compressed bytes of a document.
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).expect("the gzip writes");
    encoder.finish().expect("the gzip finishes")
}

/// The repository's `proxy` configuration: the upstream, its scopes and the
/// public key the upstream signs with.
fn proxy_of(upstream: &support::lab::Upstream, key: &str) -> Value {
    json!({
        "upstream": {
            "id": "rpm-lab",
            "url": upstream.base(),
            "scopes": ["repodata/", "Packages/"],
            "gpg_key_armor": key,
        },
        "cache": {},
    })
}

/// The lab's deployment.
///
/// The upstream signs its `repomd` with a key that is **not** the hub's:
/// the hub boots with another, and that is why the key case can
/// assert that the one served is not the hub's.
fn the_lab() -> Lab {
    let upstream = support::lab::Upstream::start();
    let upstream_signer =
        MetadataSigner::generate("Upstream <upstream@example.local>").expect("the upstream key");
    let upstream_key = upstream_signer.key_asc().expect("the upstream public key");
    let hub_signer = MetadataSigner::generate("Hub <hub@example.local>").expect("the hub key");
    let hub_key = hub_signer.key_asc().expect("the hub public key");
    let hub_armor = hub_signer.to_armor().expect("the armored hub key");

    // The upstream's repodata: the document, the primary it declares and the
    // signature that covers it.
    let primary_xml = yum::primary_document(&[]);
    let primary_gz = gzip(primary_xml.as_bytes());
    let primary_sha = sha256_of(&primary_gz);
    let location = format!("repodata/{primary_sha}-primary.xml.gz");
    let entry = yum::RepodataEntry {
        kind: "primary",
        location: &location,
        checksum: &primary_sha,
        size: primary_gz.len() as u64,
        open_checksum: &sha256_of(primary_xml.as_bytes()),
        open_size: primary_xml.len() as u64,
        timestamp: REVISION,
    };
    let repomd = yum::repomd_document(REVISION, &[entry]).into_bytes();
    let signature = upstream_signer
        .sign_detached(&repomd)
        .expect("the upstream signs its repomd");

    upstream.put("repodata/repomd.xml", &repomd, None);
    upstream.put("repodata/repomd.xml.asc", &signature, None);
    upstream.put(&location, &primary_gz, None);
    upstream.put(PACKAGE_PATH, PACKAGE, None);
    // A metadata that the repomd does **not** declare.
    upstream.put("repodata/primary.xml.gz", &primary_gz, None);
    // And a metadata whose name announces a checksum that its bytes do not
    // meet.
    upstream.put(
        &format!("repodata/{}-primary.xml.gz", "cd".repeat(32)),
        b"these bytes are not the ones the name announces",
        None,
    );

    let deployment = Deployment::new();
    let proxy = proxy_of(&upstream, &upstream_key);
    deployment.seed_yum_proxies(&[TRUSTED], &[(LAB, proxy.clone()), (TRUSTED, proxy)]);
    Lab {
        deployment,
        upstream,
        upstream_key,
        hub_key,
        hub_armor,
        repomd,
        signature,
        primary_gz,
        primary_sha,
    }
}

/// The service's environment: the lab's upstream is the only network, and
/// the hub boots with a key the lab knows.
fn the_environment(lab: &Lab) -> Vec<(&'static str, String)> {
    vec![
        (
            "HODEI_PROXY__ALLOWED_HOSTS",
            format!("127.0.0.1:{};private;http", lab.upstream.port()),
        ),
        ("HODEI_METADATA_SIGNER_ARMOR", lab.hub_armor.clone()),
    ]
}

/// Starts the service with the lab's environment.
fn start(lab: &Lab, extra: &[(&str, &str)]) -> support::Service {
    let environment = the_environment(lab);
    let mut pairs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    for (key, value) in extra {
        pairs.push((*key, *value));
    }
    lab.deployment.start(&pairs)
}

/// A GET request to the repository, with the first account's token.
fn get(lab: &Lab, port: u16, repository: &str, path: &str) -> Answer {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let into = lab.deployment.directory.path().join("response.bin");
    let _ = std::fs::remove_file(&into);
    Curl::call(
        port,
        "GET",
        &format!("/yum/{repository}/{path}"),
        &token,
        None,
        None,
        Some(&into),
    )
    .run()
}

/// A HEAD request to the repository: what is requested is the description.
fn head(port: u16, repository: &str, path: &str) -> Answer {
    let token = support::token(FIRST_ACCOUNT, "alice");
    support::curl_describe(port, &format!("/yum/{repository}/{path}"), &token)
}

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// AC1: what is served is the upstream's
// ---------------------------------------------------------------------------

#[test]
fn the_proxy_serves_the_repomd_the_signature_and_the_key_of_the_upstream() {
    let lab = the_lab();
    let mut service = start(&lab, &[]);
    let port = service.port;

    a_case(
        CASE_REPOMD,
        TITLE_REPOMD,
        Scope::Product,
        Expectation::Succeeds,
        || {
            let repomd = get(&lab, port, LAB, "repodata/repomd.xml");
            if repomd.status != 200 {
                return LabCase::new(CASE_REPOMD, TITLE_REPOMD)
                    .failed(format!("the repomd was not served: {}", repomd.status));
            }
            if repomd.body != lab.repomd {
                return LabCase::new(CASE_REPOMD, TITLE_REPOMD)
                    .failed("the repomd served is not the one of the upstream");
            }
            LabCase::new(CASE_REPOMD, TITLE_REPOMD).passed()
        },
    );

    a_case(
        CASE_SIGNATURE,
        TITLE_SIGNATURE,
        Scope::Product,
        Expectation::Succeeds,
        || {
            let signature = get(&lab, port, LAB, "repodata/repomd.xml.asc");
            if signature.status != 200 {
                return LabCase::new(CASE_SIGNATURE, TITLE_SIGNATURE).failed(format!(
                    "the signature was not served: {}",
                    signature.status
                ));
            }
            if signature.body != lab.signature {
                return LabCase::new(CASE_SIGNATURE, TITLE_SIGNATURE)
                    .failed("the signature served is not the one of the upstream");
            }
            LabCase::new(CASE_SIGNATURE, TITLE_SIGNATURE).passed()
        },
    );

    a_case(
        CASE_KEY,
        TITLE_KEY,
        Scope::Product,
        Expectation::Succeeds,
        || {
            let key = get(&lab, port, LAB, "key.asc");
            if key.status != 200 {
                return LabCase::new(CASE_KEY, TITLE_KEY)
                    .failed(format!("the key was not served: {}", key.status));
            }
            let served = String::from_utf8_lossy(&key.body).into_owned();
            if served != lab.upstream_key {
                return LabCase::new(CASE_KEY, TITLE_KEY)
                    .failed("the key served is not the key of the upstream");
            }
            if served == lab.hub_key {
                return LabCase::new(CASE_KEY, TITLE_KEY)
                    .failed("the key served is the key of the hub");
            }
            LabCase::new(CASE_KEY, TITLE_KEY).passed()
        },
    );

    support::stop(&mut service);
}

#[test]
fn the_proxy_serves_the_metadata_of_the_upstream_with_its_checksum() {
    let lab = the_lab();
    let mut service = start(&lab, &[]);
    let port = service.port;
    let path = format!("repodata/{}-primary.xml.gz", lab.primary_sha);

    a_case(
        CASE_METADATA,
        TITLE_METADATA,
        Scope::Product,
        Expectation::Succeeds,
        || {
            let metadata = get(&lab, port, LAB, &path);
            if metadata.status != 200 {
                return LabCase::new(CASE_METADATA, TITLE_METADATA)
                    .failed(format!("the metadata was not served: {}", metadata.status));
            }
            if metadata.body != lab.primary_gz {
                return LabCase::new(CASE_METADATA, TITLE_METADATA)
                    .failed("the metadata served is not the one of the upstream");
            }
            if sha256_of(&metadata.body) != lab.primary_sha {
                return LabCase::new(CASE_METADATA, TITLE_METADATA)
                    .failed("the metadata served does not meet the checksum its name declares");
            }
            // The HEAD declares the asset's real size, not zero.
            let description = head(port, LAB, &path);
            let length = description
                .header("content-length")
                .and_then(|value| value.parse::<usize>().ok());
            if length != Some(lab.primary_gz.len()) {
                return LabCase::new(CASE_METADATA, TITLE_METADATA).failed(format!(
                    "the HEAD of the metadata declared {length:?} bytes, and the asset weighs {}",
                    lab.primary_gz.len()
                ));
            }
            LabCase::new(CASE_METADATA, TITLE_METADATA).passed()
        },
    );

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC2: the checksum and the generation
// ---------------------------------------------------------------------------

#[test]
fn the_proxy_refuses_the_metadata_that_does_not_match_its_checksum() {
    let lab = the_lab();
    let mut service = start(&lab, &[]);
    let port = service.port;
    let path = format!("repodata/{}-primary.xml.gz", "cd".repeat(32));

    a_case(
        CASE_CORRUPT,
        TITLE_CORRUPT,
        Scope::Product,
        Expectation::Refused,
        || {
            let metadata = get(&lab, port, LAB, &path);
            if metadata.status == 200 {
                return LabCase::new(CASE_CORRUPT, TITLE_CORRUPT)
                    .failed("the metadata that does not meet its checksum was served");
            }
            if metadata.status != 502 {
                return LabCase::new(CASE_CORRUPT, TITLE_CORRUPT).failed(format!(
                    "the corrupt metadata answered {}, and it has to answer 502",
                    metadata.status
                ));
            }
            // The corrupt bytes are not what is answered.
            if metadata.body == b"these bytes are not the ones the name announces" {
                return LabCase::new(CASE_CORRUPT, TITLE_CORRUPT)
                    .failed("the corrupt bytes were served");
            }
            LabCase::new(CASE_CORRUPT, TITLE_CORRUPT).refused(format!(
                "the corrupt metadata was refused with {}",
                metadata.status
            ))
        },
    );

    support::stop(&mut service);
}

#[test]
fn the_proxy_refuses_the_metadata_that_the_repomd_does_not_declare() {
    let lab = the_lab();
    let mut service = start(&lab, &[]);
    let port = service.port;

    a_case(
        CASE_UNDECLARED,
        TITLE_UNDECLARED,
        Scope::Product,
        Expectation::Refused,
        || {
            // The client requests the repomd first: it is the one that declares.
            let repomd = get(&lab, port, LAB, "repodata/repomd.xml");
            if repomd.status != 200 {
                return LabCase::new(CASE_UNDECLARED, TITLE_UNDECLARED)
                    .failed(format!("the repomd was not served: {}", repomd.status));
            }
            let undeclared = get(&lab, port, LAB, "repodata/primary.xml.gz");
            if undeclared.status != 404 {
                return LabCase::new(CASE_UNDECLARED, TITLE_UNDECLARED).failed(format!(
                    "the metadata that the repomd does not declare answered {}, and it has to answer 404",
                    undeclared.status
                ));
            }
            // And the upstream is not even asked for it.
            if lab.upstream.hits("repodata/primary.xml.gz") != 0 {
                return LabCase::new(CASE_UNDECLARED, TITLE_UNDECLARED)
                    .failed("the upstream was asked for metadata that nobody declares");
            }
            LabCase::new(CASE_UNDECLARED, TITLE_UNDECLARED)
                .refused("the undeclared metadata was refused with 404")
        },
    );

    support::stop(&mut service);
}

#[test]
fn the_proxy_serves_the_package_of_the_upstream() {
    let lab = the_lab();
    let mut service = start(&lab, &[]);
    let port = service.port;

    a_case(
        CASE_PACKAGE,
        TITLE_PACKAGE,
        Scope::Product,
        Expectation::Succeeds,
        || {
            let package = get(&lab, port, LAB, PACKAGE_PATH);
            if package.status != 200 {
                return LabCase::new(CASE_PACKAGE, TITLE_PACKAGE)
                    .failed(format!("the package was not served: {}", package.status));
            }
            if package.body != PACKAGE {
                return LabCase::new(CASE_PACKAGE, TITLE_PACKAGE)
                    .failed("the package served is not the one of the upstream");
            }
            LabCase::new(CASE_PACKAGE, TITLE_PACKAGE).passed()
        },
    );

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC3: the scope, offline and trust
// ---------------------------------------------------------------------------

#[test]
fn a_path_out_of_the_scope_is_not_served() {
    let lab = the_lab();
    let mut service = start(&lab, &[]);
    let port = service.port;

    a_case(
        CASE_SCOPE,
        TITLE_SCOPE,
        Scope::Product,
        Expectation::Refused,
        || {
            let outside = get(&lab, port, LAB, "pool/otra-cosa-1.0-1.x86_64.rpm");
            if outside.status != 404 {
                return LabCase::new(CASE_SCOPE, TITLE_SCOPE).failed(format!(
                    "the path out of the scope answered {}, and it has to answer 404",
                    outside.status
                ));
            }
            if lab.upstream.hits("pool/otra-cosa-1.0-1.x86_64.rpm") != 0 {
                return LabCase::new(CASE_SCOPE, TITLE_SCOPE)
                    .failed("the upstream was asked for a path out of the scope");
            }
            LabCase::new(CASE_SCOPE, TITLE_SCOPE).refused("the path out of the scope answered 404")
        },
    );

    support::stop(&mut service);
}

#[test]
fn offline_serves_the_cached_copy_and_not_more() {
    let lab = the_lab();
    let mut service = start(&lab, &[]);
    let port = service.port;

    // What was already downloaded.
    let first = get(&lab, port, LAB, "repodata/repomd.xml");
    assert_eq!(
        first.status,
        200,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    support::stop(&mut service);
    let before = lab.upstream.total();

    // The whole service without network.
    let mut service = start(&lab, &[("HODEI_PROXY__OFFLINE", "true")]);
    let port = service.port;

    a_case(
        CASE_OFFLINE,
        TITLE_OFFLINE,
        Scope::Product,
        Expectation::Succeeds,
        || {
            let cached = get(&lab, port, LAB, "repodata/repomd.xml");
            if cached.status != 200 {
                return LabCase::new(CASE_OFFLINE, TITLE_OFFLINE)
                    .failed(format!("the cached copy was not served: {}", cached.status));
            }
            if cached.body != lab.repomd {
                return LabCase::new(CASE_OFFLINE, TITLE_OFFLINE)
                    .failed("the cached copy is not the repomd of the upstream");
            }
            if cached.header("x-hodei-cache").as_deref() != Some("offline") {
                return LabCase::new(CASE_OFFLINE, TITLE_OFFLINE).failed(format!(
                    "the cached copy was served without saying that it came from the cache: {:?}",
                    cached.header("x-hodei-cache")
                ));
            }
            // What was never downloaded is not invented.
            let missing = get(&lab, port, LAB, "Packages/nunca-descargado.rpm");
            if missing.status != 503 {
                return LabCase::new(CASE_OFFLINE, TITLE_OFFLINE).failed(format!(
                    "the content that was never downloaded answered {}, and it has to answer 503",
                    missing.status
                ));
            }
            if lab.upstream.total() != before {
                return LabCase::new(CASE_OFFLINE, TITLE_OFFLINE)
                    .failed("the offline service touched the network");
            }
            LabCase::new(CASE_OFFLINE, TITLE_OFFLINE).passed()
        },
    );

    support::stop(&mut service);
}

#[test]
fn the_trust_gate_applies_to_the_proxy_content() {
    let lab = the_lab();
    let mut service = start(&lab, &[]);
    let port = service.port;

    a_case(
        CASE_TRUST,
        TITLE_TRUST,
        Scope::Product,
        Expectation::Refused,
        || {
            let trusted = get(&lab, port, TRUSTED, "repodata/repomd.xml");
            if trusted.status == 200 {
                return LabCase::new(CASE_TRUST, TITLE_TRUST)
                    .failed("the trusted proxy served content that was not granted");
            }
            // Without a grant, the content does not exist for the reader: a 404,
            // not a 403 that would say the repository is there.
            if trusted.status != 404 {
                return LabCase::new(CASE_TRUST, TITLE_TRUST).failed(format!(
                    "the trusted proxy answered {}, and it has to answer 404",
                    trusted.status
                ));
            }
            // The control: the same upstream, the same content and the open channel
            // **does** get served. Without this, the case would not distinguish the
            // gate from a broken proxy.
            let open = get(&lab, port, LAB, "repodata/repomd.xml");
            if open.status != 200 {
                return LabCase::new(CASE_TRUST, TITLE_TRUST)
                    .failed("the open proxy did not serve the same content");
            }
            LabCase::new(CASE_TRUST, TITLE_TRUST)
                .refused("the trusted proxy did not serve the content: 404")
        },
    );

    support::stop(&mut service);
}
