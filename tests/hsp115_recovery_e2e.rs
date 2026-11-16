//! Copia, restauracion y scrub contra el binario de verdad (HSP-115 / HAR-21).
//!
//! La biblioteca de recuperacion esta probada por dentro. Esto prueba lo que
//! esa prueba no puede: el servicio real publicando con un `curl` real, el
//! proceso muerto de verdad, el comando `hodei-recovery` sobre los
//! directorios que el servicio dejo, y otro servicio arrancado sobre lo
//! restaurado que sirve **los mismos bytes**, comprobados con `sha256sum`.
//!
//! Casos de la UAT de operaciones (`uat/UAT-OPERATIONS.md`, RECOVERY):
//! REC-01 (copia y restauracion en instalacion limpia), REC-02 (scrub:
//! corrupto, referencia rota y huerfano), REC-03 (reparacion desde la copia
//! sin inventar bytes), REC-04 (una copia danada o de formato futuro se
//! rechaza), REC-05 (destino no limpio) y REC-06 (caida a mitad de una
//! subida y restauracion).

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;
use support::{Deployment, FIRST_ACCOUNT};

/// El comando de recuperacion.
const RECOVERY: &str = env!("CARGO_BIN_EXE_hodei-recovery");

/// El seeder del catalogo (repositorios por cuenta).
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Lo que dijo el comando.
struct Outcome {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Outcome {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|error| panic!("not JSON ({error}):\n{}\n{}", self.stdout, self.stderr))
    }
}

/// Ejecuta `hodei-recovery` sobre un despliegue, con el servicio parado.
fn recovery(deployment: &Deployment, arguments: &[&str]) -> Outcome {
    let output = Command::new(RECOVERY)
        .env_remove("RUN_MODE")
        .args(["--db", &deployment.db, "--cas", &deployment.cas, "--json"])
        .args(arguments)
        .output()
        .expect("the recovery command runs");
    Outcome {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// La ruta de un contenido dentro del repositorio.
fn path_of(name: &str) -> String {
    format!("/repository/releases/{name}")
}

/// Unos bytes que no son texto y que dependen de la semilla.
fn content(seed: u8, length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| ((index as u32 * 31 + seed as u32 * 7) % 251) as u8)
        .collect()
}

fn digest_of(bytes: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(bytes).value
}

fn blob_path(cas: &str, digest: &str) -> PathBuf {
    Path::new(cas)
        .join(&digest[..2])
        .join(&digest[2..4])
        .join(digest)
}

/// Publica un contenido con el `curl` real y devuelve el digest de sus bytes.
fn publish(deployment: &Deployment, port: u16, name: &str, bytes: &[u8]) -> String {
    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, bytes).expect("the source is written");
    let token = support::token(FIRST_ACCOUNT, "alice");
    let answer = support::curl_upload(port, &path_of(name), &token, &source, None);
    assert_eq!(
        answer.status,
        201,
        "the publication of {name} failed:\n{}",
        deployment.the_log()
    );
    digest_of(bytes)
}

/// Descarga con el `curl` real y devuelve el sha256 que dice `sha256sum`.
fn download_sha(deployment: &Deployment, port: u16, name: &str) -> (u16, String) {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let into = deployment.directory.path().join("downloaded.bin");
    let _ = std::fs::remove_file(&into);
    let answer = support::curl_download(port, &path_of(name), &token, Some(&into));
    let sha = if into.exists() {
        support::sha256_of(&into)
    } else {
        String::new()
    };
    (answer.status, sha)
}

fn sha_of_bytes(deployment: &Deployment, bytes: &[u8]) -> String {
    let file = deployment.directory.path().join("expected.bin");
    std::fs::write(&file, bytes).expect("the expected bytes are written");
    support::sha256_of(&file)
}

/// Un despliegue con tres contenidos publicados, el servicio parado y
/// una copia hecha. Devuelve tambien los digests.
struct Prepared {
    deployment: Deployment,
    backup: PathBuf,
    contents: Vec<(String, Vec<u8>, String)>,
}

fn prepared() -> Prepared {
    let deployment = Deployment::new();
    deployment.seed();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let mut contents = Vec::new();
    for (index, name) in ["a/one.bin", "a/two.bin", "b/three.bin"].iter().enumerate() {
        let bytes = content(index as u8 + 1, 20_000 + index * 3_000);
        let digest = publish(&deployment, port, name, &bytes);
        contents.push((name.to_string(), bytes, digest));
    }
    support::stop(&mut service);

    let backup = deployment.directory.path().join("backups").join("first");
    std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
    let started = Instant::now();
    let outcome = recovery(&deployment, &["backup", "--to", &backup.to_string_lossy()]);
    assert_eq!(outcome.code, 0, "the backup failed: {}", outcome.stderr);
    eprintln!(
        "backup measured: {} ms with the process",
        started.elapsed().as_millis()
    );
    Prepared {
        deployment,
        backup,
        contents,
    }
}

fn findings(report: &Value) -> Vec<(String, String)> {
    report["findings"]
        .as_array()
        .expect("findings is a list")
        .iter()
        .map(|finding| {
            (
                finding["kind"].as_str().unwrap().to_string(),
                finding["digest"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// REC-01: la copia se restaura en una instalacion limpia y un servicio
/// nuevo sirve los mismos bytes que el primero.
#[test]
fn a_backup_restores_into_a_clean_installation_and_serves_the_same_bytes() {
    let source = prepared();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(source.backup.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["format"], 1);
    assert_eq!(manifest["blobs"].as_array().unwrap().len(), 3);

    let verified = recovery(
        &source.deployment,
        &["verify", &source.backup.to_string_lossy()],
    );
    assert_eq!(verified.code, 0, "{}", verified.stderr);
    assert!(verified.json()["problems"].as_array().unwrap().is_empty());

    // Una instalacion limpia: ni base ni almacen existen todavia.
    let clean = Deployment::new();
    assert!(!Path::new(&clean.db).exists() && !Path::new(&clean.cas).exists());
    let started = Instant::now();
    let restored = recovery(
        &clean,
        &["restore", "--from", &source.backup.to_string_lossy()],
    );
    assert_eq!(restored.code, 0, "{}\n{}", restored.stderr, restored.stdout);
    let report = restored.json();
    assert_eq!(report["blobs_restored"], 3);
    assert!(findings(&report["scrub"]).is_empty(), "{report}");
    eprintln!(
        "RTO measured: restore {} ms in the command, {} ms with the process",
        report["elapsed_ms"],
        started.elapsed().as_millis()
    );

    // Y un servicio real sobre lo restaurado sirve los bytes de antes.
    let mut service = clean.start(&[]);
    for (name, bytes, _) in &source.contents {
        let (status, sha) = download_sha(&clean, service.port, name);
        assert_eq!(status, 200, "{name} is not served:\n{}", clean.the_log());
        assert_eq!(sha, sha_of_bytes(&clean, bytes), "{name} changed");
    }
    support::stop(&mut service);
}

/// REC-02 y REC-03: el scrub encuentra un corrupto, una referencia rota y
/// un huerfano, y la reparacion solo usa bytes de la copia.
#[test]
fn the_scrub_finds_the_three_kinds_of_damage_and_the_repair_never_invents_bytes() {
    let source = prepared();
    let deployment = &source.deployment;
    let sound = recovery(deployment, &["scrub"]);
    assert_eq!(sound.code, 0, "{}\n{}", sound.stderr, sound.stdout);
    assert!(findings(&sound.json()).is_empty());

    // Un contenido publicado **despues** de la copia: la copia no lo tiene.
    let mut service = deployment.start(&[]);
    let late_bytes = content(9, 5_000);
    let late = publish(deployment, service.port, "late/new.bin", &late_bytes);
    support::stop(&mut service);

    let corrupt = &source.contents[0].2;
    let broken = &source.contents[1].2;
    // Bytes distintos con el mismo nombre: corrupcion de verdad.
    std::fs::write(blob_path(&deployment.cas, corrupt), b"bit rot").unwrap();
    std::fs::remove_file(blob_path(&deployment.cas, broken)).unwrap();
    std::fs::remove_file(blob_path(&deployment.cas, &late)).unwrap();
    let orphan_bytes = b"nobody refers to this".to_vec();
    let orphan = digest_of(&orphan_bytes);
    let orphan_path = blob_path(&deployment.cas, &orphan);
    std::fs::create_dir_all(orphan_path.parent().unwrap()).unwrap();
    std::fs::write(&orphan_path, &orphan_bytes).unwrap();

    let damaged = recovery(deployment, &["scrub"]);
    assert_eq!(damaged.code, 1, "{}\n{}", damaged.stderr, damaged.stdout);
    let report = damaged.json();
    let found = findings(&report);
    assert!(
        found.contains(&("corrupt".into(), format!("sha256:{corrupt}"))),
        "{found:?}"
    );
    assert!(
        found.contains(&("broken_reference".into(), format!("sha256:{broken}"))),
        "{found:?}"
    );
    assert!(
        found.contains(&("broken_reference".into(), format!("sha256:{late}"))),
        "{found:?}"
    );
    assert!(
        found.contains(&("orphan".into(), format!("sha256:{orphan}"))),
        "{found:?}"
    );
    assert_eq!(report["summary"]["corrupt"], 1);
    assert_eq!(report["summary"]["broken_references"], 2);

    // El scrub no repara nada: lo dañado sigue igual.
    assert_eq!(
        std::fs::read(blob_path(&deployment.cas, corrupt)).unwrap(),
        b"bit rot"
    );
    assert!(!blob_path(&deployment.cas, broken).exists());

    let repaired = recovery(
        deployment,
        &["repair", "--from", &source.backup.to_string_lossy()],
    );
    // Queda una referencia rota que la copia no puede arreglar: no se inventa.
    assert_eq!(repaired.code, 1, "{}\n{}", repaired.stderr, repaired.stdout);
    let repair = repaired.json();
    assert_eq!(repair["repair"]["restored"].as_array().unwrap().len(), 2);
    assert_eq!(
        repair["repair"]["not_in_backup"],
        serde_json::json!([format!("sha256:{late}")])
    );
    assert_eq!(repair["repair"]["quarantined"].as_array().unwrap().len(), 1);
    assert!(
        !blob_path(&deployment.cas, &late).exists(),
        "bytes were invented"
    );
    let after = findings(&repair["after"]);
    assert!(
        after.iter().all(|(kind, digest)| {
            (kind == "broken_reference" && digest == &format!("sha256:{late}")) || kind == "orphan"
        }),
        "{after:?}"
    );

    // El corrupto esta apartado, no borrado.
    let quarantined: Vec<_> = std::fs::read_dir(Path::new(&deployment.cas).join("quarantine"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(quarantined.len(), 1);
    assert_eq!(std::fs::read(&quarantined[0]).unwrap(), b"bit rot");

    // Y los clientes reales reciben otra vez los bytes originales.
    let mut service = deployment.start(&[]);
    for index in 0..2 {
        let (name, bytes, _) = &source.contents[index];
        let (status, sha) = download_sha(deployment, service.port, name);
        assert_eq!(status, 200, "{name}:\n{}", deployment.the_log());
        assert_eq!(sha, sha_of_bytes(deployment, bytes), "{name}");
    }
    support::stop(&mut service);
}

/// REC-04: una copia con un blob danado o de un formato futuro se rechaza
/// antes de escribir nada.
#[test]
fn a_damaged_or_future_backup_is_refused_before_anything_is_written() {
    let source = prepared();
    let backup = &source.backup;

    // Formato futuro.
    let manifest_path = backup.join("manifest.json");
    let original = std::fs::read_to_string(&manifest_path).unwrap();
    let mut manifest: Value = serde_json::from_str(&original).unwrap();
    manifest["format"] = serde_json::json!(99);
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();
    let clean = Deployment::new();
    let refused = recovery(&clean, &["restore", "--from", &backup.to_string_lossy()]);
    assert_eq!(refused.code, 2, "{}", refused.stdout);
    assert!(refused.stderr.contains("99"), "{}", refused.stderr);
    assert!(!Path::new(&clean.cas).exists(), "nothing is written");
    std::fs::write(&manifest_path, original).unwrap();

    // Un blob con otros bytes.
    let digest = &source.contents[2].2;
    let victim = blob_path(&backup.join("blobs").to_string_lossy(), digest);
    let good = std::fs::read(&victim).unwrap();
    let mut bad = good.clone();
    bad[10] ^= 0xff;
    std::fs::write(&victim, bad).unwrap();
    let verified = recovery(&source.deployment, &["verify", &backup.to_string_lossy()]);
    assert_eq!(verified.code, 1, "{}", verified.stderr);
    assert_eq!(verified.json()["problems"].as_array().unwrap().len(), 1);
    let refused = recovery(&clean, &["restore", "--from", &backup.to_string_lossy()]);
    assert_eq!(refused.code, 2);
    assert!(!Path::new(&clean.cas).exists(), "nothing is written");

    // Y no repara nada desde una copia danada.
    let repaired = recovery(
        &source.deployment,
        &["repair", "--from", &backup.to_string_lossy()],
    );
    assert_eq!(repaired.code, 2);
}

/// REC-05: la restauracion no se mezcla con datos vivos.
#[test]
fn a_restore_over_existing_data_is_refused_and_leaves_it_untouched() {
    let source = prepared();
    let live = &source.deployment;
    let before = std::fs::read(blob_path(&live.cas, &source.contents[0].2)).unwrap();
    let refused = recovery(
        live,
        &["restore", "--from", &source.backup.to_string_lossy()],
    );
    assert_eq!(refused.code, 2, "{}", refused.stdout);
    assert!(refused.stderr.contains("not clean"), "{}", refused.stderr);
    assert_eq!(
        std::fs::read(blob_path(&live.cas, &source.contents[0].2)).unwrap(),
        before
    );
    // Un backup no pisa otro backup.
    let again = recovery(live, &["backup", "--to", &source.backup.to_string_lossy()]);
    assert_eq!(again.code, 2);
}

/// REC-06: el servicio muere a mitad de una subida; la copia no lleva nada
/// a medias y lo restaurado sirve lo que estaba publicado.
#[test]
fn a_crash_in_the_middle_of_an_upload_leaves_a_sound_backup_and_restore() {
    let deployment = Deployment::new();
    deployment.seed();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let kept = content(3, 30_000);
    publish(&deployment, port, "kept/file.bin", &kept);

    let big = deployment.directory.path().join("big.bin");
    std::fs::write(&big, content(5, 8 * 1024 * 1024)).unwrap();
    let mut upload = support::curl_upload_in_background(
        port,
        &path_of("cut/file.bin"),
        &token,
        &big,
        Some("512K"),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while deployment.staging_bytes() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(deployment.staging_bytes() > 0, "{}", deployment.the_log());
    support::kill(&mut service);
    let _ = upload.wait();

    // Tras la caida el almacen esta sano: lo a medias es staging, no contenido.
    let scrub = recovery(&deployment, &["scrub"]);
    assert_eq!(scrub.code, 0, "{}\n{}", scrub.stderr, scrub.stdout);
    assert!(findings(&scrub.json()).is_empty());

    let backup = deployment.directory.path().join("after-crash");
    let made = recovery(&deployment, &["backup", "--to", &backup.to_string_lossy()]);
    assert_eq!(made.code, 0, "{}", made.stderr);
    assert_eq!(
        made.json()["manifest"]["blobs"].as_array().unwrap().len(),
        1
    );

    let clean = Deployment::new();
    let restored = recovery(&clean, &["restore", "--from", &backup.to_string_lossy()]);
    assert_eq!(restored.code, 0, "{}\n{}", restored.stderr, restored.stdout);
    let mut service = clean.start(&[]);
    let (status, sha) = download_sha(&clean, service.port, "kept/file.bin");
    assert_eq!(status, 200);
    assert_eq!(sha, sha_of_bytes(&clean, &kept));
    let (status, _) = download_sha(&clean, service.port, "cut/file.bin");
    assert_eq!(status, 404, "the interrupted upload must not exist");
    support::stop(&mut service);
}

/// El esquema del despliegue, con la superficie administrativa.
fn the_schema() -> String {
    let mut schema: Value = serde_json::from_str(support::SCHEMA).expect("the schema parses");
    schema["Admin"] = serde_json::json!({
        "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
        "actions": {
            "AccessAdminApi": {
                "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] }
            }
        }
    });
    schema.to_string()
}

/// REC-02 por HTTP: el servicio vivo informa del dano sin escribir nada.
#[test]
fn the_running_service_reports_the_damage_and_changes_nothing() {
    let deployment = Deployment::new();
    std::fs::write(
        deployment.directory.path().join("schema.json"),
        the_schema(),
    )
    .unwrap();
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        format!(
            "{}\npermit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Admin::Action::\"AccessAdminApi\", resource);\n",
            support::the_policy()
        ),
    )
    .unwrap();
    deployment.seed();
    let mut service = deployment.start(&[("HODEI_SECURITY__ADMIN_API_ENABLED", "true")]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let bytes = content(2, 12_000);
    let digest = publish(&deployment, port, "live/file.bin", &bytes);

    let ask = |token: &str| -> (u16, Value) {
        let response = deployment.directory.path().join("scrub.json");
        let _ = std::fs::remove_file(&response);
        let answer = support::Curl::call(
            port,
            "GET",
            "/api/v1/scrub",
            token,
            None,
            None,
            Some(&response),
        )
        .run();
        let body = std::fs::read(&response).unwrap_or_default();
        (
            answer.status,
            serde_json::from_slice(&body).unwrap_or(Value::Null),
        )
    };

    let (status, report) = ask(&token);
    assert_eq!(status, 200, "{}", deployment.the_log());
    assert_eq!(report["blobs_checked"], 1);
    assert!(findings(&report).is_empty(), "{report}");

    std::fs::write(blob_path(&deployment.cas, &digest), b"rotten").unwrap();
    let (status, report) = ask(&token);
    assert_eq!(status, 200);
    assert_eq!(
        findings(&report),
        vec![("corrupt".to_string(), format!("sha256:{digest}"))]
    );
    // El informe no toca nada: los bytes dañados siguen donde estaban.
    assert_eq!(
        std::fs::read(blob_path(&deployment.cas, &digest)).unwrap(),
        b"rotten"
    );

    // Sin credencial no hay informe.
    let (status, _) = ask("");
    assert!(status == 401 || status == 403, "{status}");
    support::stop(&mut service);
}
