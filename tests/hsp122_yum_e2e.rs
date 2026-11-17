//! APT hosted de HSP-122, lado RPM/yum, a nivel HTTP.
//!
//! Se prueba: la subida de un `.rpm` con la gramatica del pool dictada
//! por su header, el ensamblado del indice `primary.xml.gz` por
//! contenido, el `repomd.xml` reproducible que fija la suma exacta del
//! gz servido, su firma `repomd.xml.asc` verificada contra la `key.asc`
//! con el **oraculo `gpgv`**, el ETag de las lecturas condicionales y
//! la inmutabilidad de la version.
//!
//! El cliente real (`dnf`) vive en `hsp122_yum_client.rs`.

mod support;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::io::Read as _;
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn call(
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&[u8]>,
    extra: &[(&str, &str)],
) -> Answer {
    if method == "HEAD" {
        return Curl::describe(port, path, token).run();
    }
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let mut headers: Vec<(&str, &str)> = extra.to_vec();
    if sent.is_some() {
        headers.push(("content-type", "application/octet-stream"));
    }
    Curl::call_with_headers(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        &headers,
        Some(&into),
    )
    .run()
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

// ---------------------------------------------------------------------------
// Un .rpm minimo pero fiel al formato: lead de 96, header de firma
// vacio alineado a 8 y header principal con el index y el store. La
// estructura es la que escribe rpm-build; cambia solo que el payload
// no exista: aqui lo que se indexa es el header.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Field {
    Str(&'static str),
    U32(u64),
}

fn build_rpm(fields: &[(i32, Field)]) -> Vec<u8> {
    let mut store: Vec<u8> = Vec::new();
    let mut index: Vec<u8> = Vec::new();
    for (tag, value) in fields {
        let (offset, kind, count) = match value {
            Field::Str(text) => {
                let offset = store.len() as u32;
                store.extend_from_slice(text.as_bytes());
                store.push(0);
                (offset, 6u32, 1u32)
            }
            Field::U32(number) => {
                let pad = (4 - store.len() % 4) % 4;
                store.extend(std::iter::repeat_n(0, pad));
                let offset = store.len() as u32;
                store.extend_from_slice(&(*number as u32).to_be_bytes());
                (offset, 4u32, 1u32)
            }
        };
        index.extend_from_slice(&tag.to_be_bytes());
        index.extend_from_slice(&kind.to_be_bytes());
        index.extend_from_slice(&offset.to_be_bytes());
        index.extend_from_slice(&count.to_be_bytes());
    }
    let mut header = vec![0x8e, 0xad, 0xe8, 1];
    header.extend_from_slice(&[0u8; 4]);
    header.extend_from_slice(&(fields.len() as u32).to_be_bytes());
    header.extend_from_slice(&(store.len() as u32).to_be_bytes());
    header.extend_from_slice(&index);
    header.extend_from_slice(&store);

    let mut lead = vec![0u8; 96];
    lead[0..4].copy_from_slice(b"\xed\xab\xee\xdb");
    let mut rpm = lead;
    // Header de firma vacio.
    rpm.extend_from_slice(&[0x8e, 0xad, 0xe8, 1]);
    rpm.extend_from_slice(&[0u8; 4]);
    rpm.extend_from_slice(&0u32.to_be_bytes());
    rpm.extend_from_slice(&0u32.to_be_bytes());
    let pad = (8 - rpm.len() % 8) % 8;
    rpm.extend(std::iter::repeat_n(0, pad));
    rpm.extend(header);
    rpm
}

fn hello_rpm() -> Vec<u8> {
    build_rpm(&[
        (1000, Field::Str("hello-hodei")),
        (1001, Field::Str("1.0.0")),
        (1002, Field::Str("1.fc40")),
        (1004, Field::Str("A tiny hello package")),
        (1005, Field::Str("Installed through the real dnf client.")),
        (1006, Field::U32(1_700_000_000)),
        (1009, Field::U32(4096)),
        (1014, Field::Str("MIT")),
        (1022, Field::Str("x86_64")),
    ])
}

fn tools_rpm() -> Vec<u8> {
    build_rpm(&[
        (1000, Field::Str("hodei-tools")),
        (1001, Field::Str("0.2.0")),
        (1002, Field::Str("1.fc40")),
        (1004, Field::Str("Architecture independent tools")),
        (1005, Field::Str("No binaries, no architecture.")),
        (1006, Field::U32(1_700_000_500)),
        (1009, Field::U32(1024)),
        (1022, Field::Str("noarch")),
    ])
}

const HELLO_PATH: &str = "pool/hello-hodei-1.0.0-1.fc40.x86_64.rpm";
const TOOLS_PATH: &str = "pool/hodei-tools-0.2.0-1.fc40.noarch.rpm";

#[test]
fn an_rpm_is_published_indexed_and_its_metadata_signature_verifies() {
    let deployment = Deployment::new();
    deployment.seed_yum(&[("fedora-hosted", serde_json::Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let hello = hello_rpm();
    let hello_sha = sha256_hex(&hello);

    // La ruta la dicta el header: subir al sitio que apetezca es un no.
    let misplaced = call(
        port,
        "PUT",
        "/yum/fedora-hosted/pool/otra-cosa-1.0-1.x86_64.rpm",
        &token,
        Some(&hello),
        &[],
    );
    assert_eq!(misplaced.status, 409, "{}", text_of(&misplaced));
    assert!(
        text_of(&misplaced).contains(HELLO_PATH),
        "the conflict must name the path the grammar demands: {}",
        text_of(&misplaced)
    );

    // Un cuerpo que no es un `.rpm` no entra en el pool.
    let junk = call(
        port,
        "PUT",
        &format!("/yum/fedora-hosted/{HELLO_PATH}"),
        &token,
        Some(b"this is not an rpm"),
        &[],
    );
    assert_eq!(junk.status, 400, "{}", text_of(&junk));

    let published = call(
        port,
        "PUT",
        &format!("/yum/fedora-hosted/{HELLO_PATH}"),
        &token,
        Some(&hello),
        &[],
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // El binario se sirve byte a byte por la ruta del indice.
    let download = call(
        port,
        "GET",
        &format!("/yum/fedora-hosted/{HELLO_PATH}"),
        &token,
        None,
        &[],
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
    assert_eq!(download.body, hello);

    // El repomd fija el primary con la suma exacta del gz que se sirve.
    let repomd = call(
        port,
        "GET",
        "/yum/fedora-hosted/repodata/repomd.xml",
        &token,
        None,
        &[],
    );
    assert_eq!(repomd.status, 200, "{}", text_of(&repomd));
    let repomd_text = text_of(&repomd);
    assert!(repomd_text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<repomd xmlns=\"http://linux.duke.edu/metadata/repo\""));
    assert!(repomd_text.contains("<revision>"));

    // La ruta del primary es su propio digest: se pide la del repomd.
    let location = repomd_text
        .split("<location href=\"")
        .nth(1)
        .and_then(|tail| tail.split('"').next())
        .expect("the repomd names the primary")
        .to_string();
    assert!(location.starts_with("repodata/"));
    assert!(location.ends_with("-primary.xml.gz"));

    let primary = call(
        port,
        "GET",
        &format!("/yum/fedora-hosted/{location}"),
        &token,
        None,
        &[],
    );
    assert_eq!(primary.status, 200, "{}", text_of(&primary));
    let gz_sha = sha256_hex(&primary.body);
    assert_eq!(
        location,
        format!("repodata/{gz_sha}-primary.xml.gz"),
        "the name is the content"
    );
    assert!(
        repomd_text.contains(&format!("<checksum type=\"sha256\">{gz_sha}</checksum>")),
        "the repomd must pin the exact bytes of the served gz:\n{repomd_text}"
    );
    assert!(repomd_text.contains(&format!("<size>{}</size>", primary.body.len())));

    // El texto detras del gzip describe el paquete con la suma y el
    // tamano que dnf verificara tras la descarga.
    let mut xml = String::new();
    GzDecoder::new(&primary.body[..])
        .read_to_string(&mut xml)
        .expect("the index is gzip");
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<metadata xmlns=\"http://linux.duke.edu/metadata/common\" xmlns:rpm=\"http://linux.duke.edu/metadata/rpm\" packages=\"1\">"));
    assert!(xml.contains("<name>hello-hodei</name>"));
    assert!(xml.contains("<arch>x86_64</arch>"));
    assert!(xml.contains("<version ver=\"1.0.0\" rel=\"1.fc40\"/>"));
    assert!(xml.contains(&format!(
        "<checksum type=\"sha256\" pkgid=\"YES\">{hello_sha}</checksum>"
    )));
    assert!(xml.contains(&format!("<location href=\"{HELLO_PATH}\"/>")));
    assert!(xml.contains(&format!(
        "<size package=\"{}\" installed=\"4096\"",
        hello.len()
    )));
    assert!(xml.contains("<time file=\"1700000000\" build=\"1700000000\"/>"));
    assert!(xml.contains("<rpm:license>MIT</rpm:license>"));
    assert!(xml.ends_with("</metadata>\n"));

    // Reproducible: la misma metadata, dos lecturas, mismos bytes.
    let again = call(
        port,
        "GET",
        "/yum/fedora-hosted/repodata/repomd.xml",
        &token,
        None,
        &[],
    );
    assert_eq!(again.body, repomd.body, "repomd must not drift");
    let etag = again.header("etag").expect("the repomd carries an etag");

    // Y condicional: si no cambio, 304.
    let not_modified = call(
        port,
        "GET",
        "/yum/fedora-hosted/repodata/repomd.xml",
        &token,
        None,
        &[("if-none-match", etag.as_str())],
    );
    assert_eq!(not_modified.status, 304);

    // Una ruta primary con otro digest no existe: el nombre manda.
    let wrong = call(
        port,
        "GET",
        &format!(
            "/yum/fedora-hosted/repodata/{}-primary.xml.gz",
            "cd".repeat(32)
        ),
        &token,
        None,
        &[],
    );
    assert_eq!(wrong.status, 404);

    // La clave publica y la firma: `gpgv`, el verificador tras gpgme,
    // abre repomd.xml.asc contra la key.asc servida.
    if which("gpgv").is_none() {
        eprintln!("skipping the signature check: gpgv is not installed");
    } else {
        let key = call(port, "GET", "/yum/fedora-hosted/key.asc", &token, None, &[]);
        assert_eq!(key.status, 200, "{}", text_of(&key));
        assert!(text_of(&key).contains("BEGIN PGP PUBLIC KEY BLOCK"));

        let signature = call(
            port,
            "GET",
            "/yum/fedora-hosted/repodata/repomd.xml.asc",
            &token,
            None,
            &[],
        );
        assert_eq!(signature.status, 200, "{}", text_of(&signature));
        assert!(text_of(&signature).contains("BEGIN PGP SIGNATURE"));

        let dir = next("gpgv-yum");
        std::fs::create_dir_all(&dir).expect("a scratch dir");
        std::fs::write(dir.join("repomd.xml"), &repomd.body).expect("write repomd");
        std::fs::write(dir.join("repomd.xml.asc"), &signature.body).expect("write signature");
        std::fs::write(dir.join("keyring.bin"), dearmor(&text_of(&key))).expect("write keyring");
        let output = std::process::Command::new("gpgv")
            .arg("--keyring")
            .arg(dir.join("keyring.bin"))
            .arg(dir.join("repomd.xml.asc"))
            .arg(dir.join("repomd.xml"))
            .env("LC_ALL", "C")
            .output()
            .expect("gpgv runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stderr.contains("Good signature"),
            "gpgv said: {stderr}"
        );

        // La firma no abre un repomd alterado: es firma de estos bytes.
        std::fs::write(dir.join("tampered"), b"<repomd/>\n").expect("write tampered");
        let tampered = std::process::Command::new("gpgv")
            .arg("--keyring")
            .arg(dir.join("keyring.bin"))
            .arg(dir.join("repomd.xml.asc"))
            .arg(dir.join("tampered"))
            .env("LC_ALL", "C")
            .output()
            .expect("gpgv runs");
        assert!(!tampered.status.success(), "a forged repomd must not open");
    }

    // Inmutabilidad: otros bytes en la misma ruta son un no con motivo.
    let other = build_rpm(&[
        (1000, Field::Str("hello-hodei")),
        (1001, Field::Str("1.0.0")),
        (1002, Field::Str("1.fc40")),
        (1004, Field::Str("a different build of the same version")),
        (1005, Field::Str("Still the same NVR.")),
        (1006, Field::U32(1_700_000_000)),
        (1009, Field::U32(2048)),
        (1022, Field::Str("x86_64")),
    ]);
    let conflict = call(
        port,
        "PUT",
        &format!("/yum/fedora-hosted/{HELLO_PATH}"),
        &token,
        Some(&other),
        &[],
    );
    assert_eq!(conflict.status, 409, "{}", text_of(&conflict));

    // La republicacion identica es el eco: 200 sin tocar nada.
    let echo = call(
        port,
        "PUT",
        &format!("/yum/fedora-hosted/{HELLO_PATH}"),
        &token,
        Some(&hello),
        &[],
    );
    assert_eq!(echo.status, 200, "{}", text_of(&echo));

    // Una publicacion nueva mueve el indice y, con el, el repomd: el
    // noarch entra y el contador de paquetes sube.
    let tools = tools_rpm();
    let published_tools = call(
        port,
        "PUT",
        &format!("/yum/fedora-hosted/{TOOLS_PATH}"),
        &token,
        Some(&tools),
        &[],
    );
    assert_eq!(published_tools.status, 200, "{}", text_of(&published_tools));
    let repomd_after = call(
        port,
        "GET",
        "/yum/fedora-hosted/repodata/repomd.xml",
        &token,
        None,
        &[],
    );
    assert_ne!(
        repomd_after.header("etag").as_deref(),
        Some(etag.as_str()),
        "the repomd must move when the state moves"
    );
    let location_after = text_of(&repomd_after)
        .split("<location href=\"")
        .nth(1)
        .and_then(|tail| tail.split('"').next())
        .expect("the repomd names the primary")
        .to_string();
    let primary_after = call(
        port,
        "GET",
        &format!("/yum/fedora-hosted/{location_after}"),
        &token,
        None,
        &[],
    );
    let mut xml_after = String::new();
    GzDecoder::new(&primary_after.body[..])
        .read_to_string(&mut xml_after)
        .expect("the index is gzip");
    assert!(xml_after.contains("packages=\"2\""));
    assert!(xml_after.contains("<name>hodei-tools</name>"));
    // El orden es el de ruta: dos lecturas del mismo estado, mismos bytes.
    let primary_again = call(
        port,
        "GET",
        &format!("/yum/fedora-hosted/{location_after}"),
        &token,
        None,
        &[],
    );
    assert_eq!(primary_again.body, primary_after.body);
}

/// El contenido binario de un bloque armor OpenPGP: las lineas base64
/// entre los marcadores, sin la linea CRC que `gpg` escribe y nadie lee.
fn dearmor(armored: &str) -> Vec<u8> {
    let body: String = armored
        .lines()
        .skip_while(|line| !line.starts_with("-----BEGIN"))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END"))
        .filter(|line| !(line.trim().is_empty() || line.starts_with('=')))
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(&body)
        .expect("the armored key is base64")
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(program);
            candidate.is_file().then_some(candidate)
        })
    })
}
