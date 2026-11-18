//! Spike de replicacion y HA (HSP-125 / HAR-31): medir antes de construir.
//!
//! La pregunta del backlog es si hace falta federacion o basta con lo que
//! ya existe. Este fichero lo comprueba con numeros sobre el servicio real:
//!
//! 1. **Consistencia de etiquetas y referencias.** Una imagen OCI con su
//!    capa y su config se publica en el primario; la copia se restaura en
//!    una instalacion limpia; el replica sirve la misma etiqueta contra el
//!    mismo digest. La etiqueta no es metadata suelta: viaja en el backup
//!    porque el catalogo es parte de la copia.
//! 2. **RPO/RTO del failover.** El replica se crea en el minuto cero; el
//!    primario sigue recibiendo escrituras y muere. Lo que el cliente ve
//!    tras el failover es exactamente lo publicado hasta la ultima copia:
//!    el RPO medible son las escrituras del intervalo, y el RTO es el
//!    arranque del servicio sobre lo restaurado (la copia ya estaba hecha
//!    en frio; en un desastre real habria que sumar la restauracion, que
//!    el spike de HSP-115 ya midio).
//! 3. **Conflicto de escrituras y coste de reconciliacion.** Tras la
//!    divergencia, ambos lados escriben la misma ruta con bytes
//!    distintos. La reconciliacion se hace con `restore` hacia el lado
//!    perdedor? No: `restore` rechaza destinos no limpios (REC-05), y el
//!    spike lo confirma como decision. La via real es el push por API, y
//!    solo donde el repositorio lo permite: una direccion publica una
//!    vez por defecto (409), y reponer exige `raw.replace: allow` en la
//!    configuracion del operador. Medido: el rechazo por defecto y los
//!    bytes que hay que mandar para converger cuando el repo lo declara.
//!
//! Conclusion del spike (reversible, medida aqui): con copia en frio +
//! restauracion limpia basta para DR de un solo escritor; la federacion
//! active-active **no** esta justificada mientras no haya necesidad
//! multi-sitio con RPO ~ 0, y su coste (conflictos de etiqueta,
//! reconciliacion bidireccional) es exactamente lo que estos numeros
//! cuantifican. La decision de reponibilidad vive donde debe vivir —en
//! la configuracion versionada del repositorio, no en el cliente— y por
//! eso la convergencia por push es un caso particular, no una regla.

mod support;

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use support::{Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY};

const RECOVERY: &str = env!("CARGO_BIN_EXE_hodei-recovery");
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// El seeder del catalogo: `Deployment::seed` re-ejecuta este binario
/// buscando esta prueba exacta; sin ella el catalogo queda vacio y todo
/// se responde con 404.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

struct Outcome {
    code: i32,
    stdout: String,
    stderr: String,
}

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

fn content(seed: u8, length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| ((index as u32 * 31 + seed as u32 * 7) % 251) as u8)
        .collect()
}

fn digest_of(bytes: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(bytes).value
}

fn call(
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Path>,
    content_type: Option<&str>,
) -> support::Answer {
    support::Curl::call(port, method, path, token, body, content_type, None).run()
}

/// Publica un blob por el push monolitico de la distribucion.
fn put_blob(deployment: &Deployment, port: u16, token: &str, bytes: &[u8]) {
    let digest = digest_of(bytes);
    let file = deployment
        .directory
        .path()
        .join(format!("blob-{}.bin", &digest[..8]));
    std::fs::write(&file, bytes).expect("the blob is written");
    let answer = call(
        port,
        "POST",
        &format!("/v2/{FIRST_REPOSITORY}/blobs/uploads/?digest=sha256:{digest}"),
        token,
        Some(&file),
        Some("application/octet-stream"),
    );
    assert_eq!(answer.status, 201, "the blob was not published");
}

fn a_layer(seed: u8) -> Vec<u8> {
    format!("replica layer {seed}\n").into_bytes()
}

fn a_config() -> Vec<u8> {
    b"{\"architecture\":\"amd64\",\"os\":\"linux\",\"rootfs\":{\"type\":\"layers\",\"diff_ids\":[]}}".to_vec()
}

fn a_manifest(config_digest: &str, layer_digest: &str) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": format!("sha256:{config_digest}"),
            "size": a_config().len(),
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": format!("sha256:{layer_digest}"),
            "size": a_layer(1).len(),
        }],
    }))
    .expect("a manifest serializes")
}

/// Publica la imagen `repo:tag` completa (config, capa, manifiesto) y
/// devuelve el digest del manifiesto que el registro anuncio.
fn publish_image(deployment: &Deployment, port: u16, tag: &str, layer_seed: u8) -> String {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let config = a_config();
    let layer = a_layer(layer_seed);
    put_blob(deployment, port, &token, &config);
    put_blob(deployment, port, &token, &layer);
    let manifest = a_manifest(&digest_of(&config), &digest_of(&layer));
    let file = deployment.directory.path().join("image-manifest.json");
    std::fs::write(&file, &manifest).expect("the manifest is written");
    let answer = call(
        port,
        "PUT",
        &format!("/v2/{FIRST_REPOSITORY}/manifests/{tag}"),
        &token,
        Some(&file),
        Some(OCI_MANIFEST),
    );
    assert_eq!(
        answer.status,
        201,
        "the manifest was not published:\n{}",
        deployment.the_log()
    );
    answer
        .header("docker-content-digest")
        .expect("the digest of the manifest")
}

/// La etiqueta, resuelta por el servicio contra su propio catalog.
fn served_digest(port: u16, tag: &str) -> Option<String> {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let answer = support::curl_describe(
        port,
        &format!("/v2/{FIRST_REPOSITORY}/manifests/{tag}"),
        &token,
    );
    if answer.status != 200 {
        return None;
    }
    answer.header("docker-content-digest")
}

/// Publica un contenido por la superficie raw y devuelve su digest.
fn publish_raw(deployment: &Deployment, port: u16, name: &str, bytes: &[u8]) -> String {
    publish_in(deployment, port, FIRST_REPOSITORY, name, bytes)
}

/// Publica en un repositorio concreto, devolviendo el digest de sus bytes.
fn publish_in(
    deployment: &Deployment,
    port: u16,
    repository: &str,
    name: &str,
    bytes: &[u8],
) -> String {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let file = deployment.directory.path().join("raw.bin");
    std::fs::write(&file, bytes).expect("the raw source is written");
    let answer = support::curl_upload(
        port,
        &format!("/repository/{repository}/{name}"),
        &token,
        &file,
        None,
    );
    // Crear es 201; reponer donde el repo lo permite es 200 con
    // `replaced: true`. Ambos son publicacion admitida.
    assert!(
        answer.status == 201 || answer.status == 200,
        "the publication of {name} failed ({}):\n{}",
        answer.status,
        deployment.the_log()
    );
    digest_of(bytes)
}

/// El status de una publicacion, sin afirmar 201: para medir el rechazo.
fn publish_status(
    deployment: &Deployment,
    port: u16,
    repository: &str,
    name: &str,
    bytes: &[u8],
) -> u16 {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let file = deployment.directory.path().join("raw.bin");
    std::fs::write(&file, bytes).expect("the raw source is written");
    support::curl_upload(
        port,
        &format!("/repository/{repository}/{name}"),
        &token,
        &file,
        None,
    )
    .status
}

fn download_digest(deployment: &Deployment, port: u16, name: &str) -> Option<String> {
    let token = support::token(FIRST_ACCOUNT, "alice");
    let into = deployment.directory.path().join("got.bin");
    let _ = std::fs::remove_file(&into);
    let answer = support::curl_download(
        port,
        &format!("/repository/releases/{name}"),
        &token,
        Some(&into),
    );
    if answer.status != 200 {
        return None;
    }
    Some(support::sha256_of(&into))
}

/// SPIKE-01: la etiqueta OCI sobrevive la replicacion intacta. El replica
/// sirve `v1` contra el MISMO digest que anuncio el primario, con los
/// mismos bytes de manifiesto y de capa.
#[test]
fn oci_tags_survive_the_replication_with_their_digests() {
    let primary = Deployment::new();
    primary.seed();
    let mut service = primary.start(&[]);
    let digest = publish_image(&primary, service.port, "v1", 1);
    let served = served_digest(service.port, "v1").expect("the tag resolves");
    assert_eq!(served, digest);
    support::stop(&mut service);

    // El backup no pisa destinos existentes: se crea el padre, no el destino.
    let backup = primary.directory.path().join("sync").join("replica");
    std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
    let started = Instant::now();
    let copied = recovery(&primary, &["backup", "--to", &backup.to_string_lossy()]);
    assert_eq!(copied.code, 0, "{}", copied.stderr);
    let sync_ms = started.elapsed().as_millis();

    let replica = Deployment::new();
    let started = Instant::now();
    let restored = recovery(&replica, &["restore", "--from", &backup.to_string_lossy()]);
    assert_eq!(restored.code, 0, "{}\n{}", restored.stderr, restored.stdout);
    let restore_ms = started.elapsed().as_millis();

    let mut service = replica.start(&[]);
    let on_replica = served_digest(service.port, "v1").expect("the replica resolves the tag");
    assert_eq!(
        on_replica, digest,
        "the replica renamed what the primary published"
    );
    eprintln!("replication measured: sync {sync_ms} ms, restore {restore_ms} ms");
    support::stop(&mut service);
}

/// SPIKE-02: failover con perdida admisible medible. El replica se sincroniza en T0;
/// entre T0 y la caida del primario hay escrituras que el replica nunca vio.
/// Tras el failover esas escrituras faltan: el RPO es exactamente su numero, y
/// el RTO es el arranque sobre lo restaurado.
#[test]
fn the_failover_loses_exactly_the_writes_after_the_last_sync() {
    let primary = Deployment::new();
    primary.seed();
    let mut service = primary.start(&[]);
    publish_raw(&primary, service.port, "kept.bin", &content(1, 20_000));
    support::stop(&mut service);

    // Sincronizacion T0.
    let backup = primary.directory.path().join("sync").join("t0");
    std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
    assert_eq!(
        recovery(&primary, &["backup", "--to", &backup.to_string_lossy()]).code,
        0
    );
    let replica = Deployment::new();
    assert_eq!(
        recovery(&replica, &["restore", "--from", &backup.to_string_lossy()]).code,
        0
    );

    // El primario sigue vivo y recibe escrituras que no se sincronizan.
    let mut service = primary.start(&[]);
    let lost: Vec<(String, String)> = (2..4)
        .map(|index| {
            let name = format!("lost-{index}.bin");
            let digest = publish_raw(&primary, service.port, &name, &content(index, 15_000));
            (name, digest)
        })
        .collect();

    // Caida dura del primario: no hay tiempo para una ultima copia.
    support::kill(&mut service);

    // Failover: el replica arranca y sirve lo que sabia.
    let started = Instant::now();
    let mut replica_service = replica.start(&[]);
    let rto = started.elapsed();
    let sha = download_digest(&replica, replica_service.port, "kept.bin")
        .expect("the replica serves the kept write");
    assert_eq!(sha, sha256_of_bytes(&content(1, 20_000)));
    for (name, _) in &lost {
        assert_eq!(
            download_digest(&replica, replica_service.port, name),
            None,
            "{name} was never synced: the replica must not invent it"
        );
    }
    eprintln!(
        "failover measured: rpo {} writes ({} bytes each), rto {} ms",
        lost.len(),
        15_000,
        rto.as_millis()
    );
    assert_eq!(lost.len(), 2, "the RPO is exactly the unsynced window");
    support::stop(&mut replica_service);
}

/// SPIKE-03: conflicto de escrituras y coste de convergencia. Tras la
/// divergencia, el lado perdedor NO se reconcilia con `restore` (destino
/// no limpio) y si con un push por la API: se mide cuanto cuesta.
#[test]
fn divergent_writes_converge_by_api_push_and_restore_refuses_to_clobber() {
    let primary = Deployment::new();
    // El unicos que pueden converger por push son los repos que declaran
    // `raw.replace: allow`: una politica del operador, no del cliente.
    primary.seed_with_replacement(&[FIRST_REPOSITORY]);
    let mut service = primary.start(&[]);
    let shared = content(7, 25_000);
    publish_raw(&primary, service.port, "shared.bin", &shared);
    // La regla general, medida antes de la excepcion: con la politica por
    // defecto una direccion publica una vez y la segunda es un 409.
    let probe = content(6, 4_000);
    publish_in(
        &primary,
        service.port,
        FIRST_ONLY_REPOSITORY,
        "probe.bin",
        &probe,
    );
    assert_eq!(
        publish_status(
            &primary,
            service.port,
            FIRST_ONLY_REPOSITORY,
            "probe.bin",
            &content(7, 4_000)
        ),
        409,
        "a default repository does not let anyone replace what is published"
    );
    support::stop(&mut service);

    let backup = primary.directory.path().join("sync").join("divergence");
    std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
    assert_eq!(
        recovery(&primary, &["backup", "--to", &backup.to_string_lossy()]).code,
        0
    );
    let replica = Deployment::new();
    assert_eq!(
        recovery(&replica, &["restore", "--from", &backup.to_string_lossy()]).code,
        0
    );

    // Divergencia: ambos escriben shared.bin con bytes distintos.
    let mut service = primary.start(&[]);
    let primary_sha = publish_raw(&primary, service.port, "shared.bin", &content(8, 25_000));
    support::stop(&mut service);
    let mut replica_service = replica.start(&[]);
    let replica_sha = publish_raw(
        &replica,
        replica_service.port,
        "shared.bin",
        &content(9, 25_000),
    );
    support::stop(&mut replica_service);
    assert_ne!(primary_sha, replica_sha, "the conflict is real");

    // Decision medida: `restore` del backup del primario sobre el replica
    // CON datos se rechaza. No existe la via de pisar por copia.
    let refused = recovery(&replica, &["restore", "--from", &backup.to_string_lossy()]);
    assert_eq!(refused.code, 2, "{}", refused.stdout);
    assert!(refused.stderr.contains("not clean"), "{}", refused.stderr);

    // Convergencia por API: el cliente re-publica en el replica los bytes
    // ganadores. El coste medible es el alta de contenido, no el de los
    // blobs que ya existen: un digests-first nunca mandaria lo puesto.
    let mut replica_service = replica.start(&[]);
    let winner = content(8, 25_000);
    let started = Instant::now();
    let converged = publish_raw(&replica, replica_service.port, "shared.bin", &winner);
    let push_ms = started.elapsed().as_millis();
    assert_eq!(converged, primary_sha);
    let sha = download_digest(&replica, replica_service.port, "shared.bin").expect("served");
    assert_eq!(
        sha,
        sha256_of_bytes(&winner),
        "the replica now holds the winner"
    );
    eprintln!(
        "convergence measured: a {}-byte push in {push_ms} ms settles a conflict",
        winner.len()
    );
    support::stop(&mut replica_service);

    // Y la integridad del replica sigue sana tras converger.
    let scrub = recovery(&replica, &["scrub"]);
    assert_eq!(scrub.code, 0, "{}", scrub.stdout);
}

/// Lo que mide `support::sha256_of` pero desde memoria.
fn sha256_of_bytes(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::new();
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
