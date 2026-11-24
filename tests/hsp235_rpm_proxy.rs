//! HSP-235: el proxy RPM sirve el repodata del upstream **tal cual**.
//!
//! Un repositorio rpm en modo proxy no reconstruye el repodata ajeno: lo
//! trae de su upstream y lo sirve byte a byte, con la firma del upstream
//! y con la clave del upstream. Lo que se mide aqui, contra el servicio
//! de verdad:
//!
//! - el `repomd.xml` que se sirve es **el del upstream**, no un documento
//!   reconstruido;
//! - la firma que se sirve es la del upstream, y la clave que se sirve es
//!   la del upstream, no la del hub;
//! - el metadata se sirve **solo** si cumple el checksum que declara: el
//!   que su propio nombre anuncia, o el que el `repomd` cacheado
//!   declara;
//! - un metadata que no cumple su checksum **no** se sirve, y uno que el
//!   `repomd` no declara tampoco;
//! - una ruta fuera de los scopes del upstream no existe para ese proxy;
//! - sin red, la cache sirve lo ya descargado y no inventa lo demas;
//! - la compuerta de confianza tambien se aplica al camino proxy.
//!
//! El upstream es un servidor local que este fichero controla: el
//! `repomd` que firma, el primary que declara y el metadata corrupto que
//! no cumple lo que su nombre anuncia.

mod support;

use std::io::Write as _;

use hodei_artifacts_api::openpgp::MetadataSigner;
use hodei_catalog::yum;
use serde_json::{Value, json};
use sha2::Digest as _;
use support::lab::{Expectation, LabCase, Scope, describe_registry_lab, run_registry_lab_case};
use support::{Answer, Curl, Deployment, FIRST_ACCOUNT};

/// El repositorio proxy en el canal abierto.
const LAB: &str = "rpm-lab";

/// El repositorio proxy en el canal **trusted**: lo que separa es el
/// canal, y nada mas.
const TRUSTED: &str = "rpm-trusted";

/// El paquete que el upstream publica en su layout habitual.
const PACKAGE_PATH: &str = "Packages/hello-hodei-1.0.0-1.fc40.x86_64.rpm";

/// Los bytes del paquete: **sinteticos**. Lo que se mide en este
/// incremento es el camino HTTP, el checksum y la coherencia de la
/// generacion, no el formato rpm: un paquete de verdad necesita
/// `rpmbuild`, que aqui no esta.
const PACKAGE: &[u8] =
    b"a synthetic rpm: the path, the checksum and the generation are what is measured";

/// La revision del repodata del upstream.
const REVISION: u64 = 1_700_000_000;

// ---------------------------------------------------------------------------
// Los casos
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

/// Los casos que una ejecucion completa tiene que ejecutar.
///
/// Se declaran **antes** de correr: una ejecucion que solo llego a algunos
/// dice cuales no corrieron, y un informe con un caso que no corrio nunca es
/// verde.
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

/// Corre un caso del laboratorio.
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
// El laboratorio
// ---------------------------------------------------------------------------

/// El despliegue del laboratorio: el upstream de mentira, las claves y lo
/// que el upstream publica.
struct Lab {
    deployment: Deployment,
    upstream: support::lab::Upstream,
    /// La clave publica del upstream, en armor: la que el proxy sirve.
    upstream_key: String,
    /// La clave publica del hub, en armor: la que el proxy **no** debe
    /// servir.
    hub_key: String,
    /// La clave privada del hub, blindada: la arranca el servicio.
    hub_armor: String,
    /// El `repomd.xml` del upstream.
    repomd: Vec<u8>,
    /// La firma del `repomd.xml`, hecha por la clave del upstream.
    signature: Vec<u8>,
    /// El primary comprimido del upstream.
    primary_gz: Vec<u8>,
    /// El SHA256 del primary comprimido: el nombre por el que se pide.
    primary_sha: String,
}

/// El sha256 de unos bytes, en hexadecimal.
fn sha256_of(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Los bytes comprimidos de un documento.
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).expect("the gzip writes");
    encoder.finish().expect("the gzip finishes")
}

/// La configuracion `proxy` del repositorio: el upstream, sus scopes y la
/// clave publica con la que el upstream firma.
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

/// El despliegue del laboratorio.
///
/// El upstream firma su `repomd` con una clave que **no** es la del hub:
/// el hub arranca con otra, y por eso el caso de la clave puede
/// afirmar que la que se sirve no es la del hub.
fn the_lab() -> Lab {
    let upstream = support::lab::Upstream::start();
    let upstream_signer =
        MetadataSigner::generate("Upstream <upstream@example.local>").expect("the upstream key");
    let upstream_key = upstream_signer.key_asc().expect("the upstream public key");
    let hub_signer = MetadataSigner::generate("Hub <hub@example.local>").expect("the hub key");
    let hub_key = hub_signer.key_asc().expect("the hub public key");
    let hub_armor = hub_signer.to_armor().expect("the armored hub key");

    // El repodata del upstream: el documento, el primary que declara y la
    // firma que lo cubre.
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
    // Un metadata que el repomd **no** declara.
    upstream.put("repodata/primary.xml.gz", &primary_gz, None);
    // Y un metadata cuyo nombre anuncia un checksum que sus bytes no
    // cumplen.
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

/// El entorno del servicio: el upstream del laboratorio es la unica red, y
/// el hub arranca con una clave que el laboratorio conoce.
fn the_environment(lab: &Lab) -> Vec<(&'static str, String)> {
    vec![
        (
            "HODEI_PROXY__ALLOWED_HOSTS",
            format!("127.0.0.1:{};private;http", lab.upstream.port()),
        ),
        ("HODEI_METADATA_SIGNER_ARMOR", lab.hub_armor.clone()),
    ]
}

/// Arranca el servicio con el entorno del laboratorio.
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

/// Una peticion GET al repositorio, con el token de la primera cuenta.
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

/// Una peticion HEAD al repositorio: lo que se pide es la descripcion.
fn head(port: u16, repository: &str, path: &str) -> Answer {
    let token = support::token(FIRST_ACCOUNT, "alice");
    support::curl_describe(port, &format!("/yum/{repository}/{path}"), &token)
}

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// AC1: lo que se sirve es lo del upstream
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
            // El HEAD declara el tamano real del asset, no cero.
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
// AC2: el checksum y la generacion
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
            // Los bytes corruptos no son lo que se responde.
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
            // El cliente pide el repomd primero: es el que declara.
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
            // Y ni se pregunta al upstream por ella.
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
// AC3: el scope, el offline y la confianza
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

    // Lo ya descargado.
    let first = get(&lab, port, LAB, "repodata/repomd.xml");
    assert_eq!(
        first.status,
        200,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    support::stop(&mut service);
    let before = lab.upstream.total();

    // El servicio entero sin red.
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
            // Lo que nunca se descargó no se inventa.
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
            // Sin concesion, el contenido no existe para quien lee: un 404,
            // no un 403 que diria que el repositorio esta ahi.
            if trusted.status != 404 {
                return LabCase::new(CASE_TRUST, TITLE_TRUST).failed(format!(
                    "the trusted proxy answered {}, and it has to answer 404",
                    trusted.status
                ));
            }
            // El control: el mismo upstream, el mismo contenido y el canal
            // abierto **si** se sirve. Sin esto, el caso no distinguiria la
            // compuerta de un proxy roto.
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
