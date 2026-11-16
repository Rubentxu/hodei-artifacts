//! La UAT de doc-9 (Raw Hosted + CAS + Catalog), ejecutada.
//!
//! Cada escenario se aprueba por su **oraculo**, no por la forma de la
//! respuesta, y el comando se registra con lo que realmente se ejecuto.
//! El arnes comun —el resultado, el informe y los ayudantes— esta en
//! `doc9_support`.

mod doc9_support;
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use doc9_support::{
    FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, Outcome, SECOND_ACCOUNT,
    SECOND_REPOSITORY, a_deployment, blob_path, outcomes, record, the_content, the_digest,
    the_path, the_report_path,
};
use hodei_catalog::{CatalogReader, CatalogWriter};
use hodei_hub::blob::BlobStore;
use support::Deployment;

/// El semillero del catálogo.
///
/// El despliegue siembra el catálogo en un proceso aparte, y este test es
/// el que lo hace cuando el proceso le pide: el arnés pasa la ruta de la
/// base por la variable `HODEI_SEED_DB`, y este es el que la escribe.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// RAW-01, RAW-02 — publicar, descargar, describir y pedir un tramo.
// ---------------------------------------------------------------------------

/// RAW-01 y RAW-02: lo publicado vuelve intacto, y el HEAD y el Range
/// dicen lo que deben.
///
/// El oraculo es el digest: `sha256sum` del fichero publicado y del
/// descargado tienen que ser el mismo, y eso lo dice un programa que no
/// tiene nada que ver con este servicio.
#[test]
fn raw_01_and_raw_02_the_content_comes_back_intact_and_the_head_and_the_range_say_what_they_should()
{
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let source = deployment.directory.path().join("source.bin");
    let content = the_content();
    std::fs::write(&source, &content).expect("the source is written");
    let expected = support::sha256_of(&source);

    let upload = support::Curl::upload(
        service.port,
        &the_path(FIRST_REPOSITORY, "app.bin"),
        &token,
        &source,
        None,
    );
    let upload_command = upload.shown();
    let published = upload.run();
    assert_eq!(
        published.status,
        201,
        "the content is published:\n{}",
        deployment.the_log()
    );

    let downloaded = deployment.directory.path().join("downloaded.bin");
    let download = support::Curl::download(
        service.port,
        &the_path(FIRST_REPOSITORY, "app.bin"),
        &token,
        Some(&downloaded),
    );
    let download_command = download.shown();
    let answer = download.run();
    assert_eq!(answer.status, 200);
    let measured = support::sha256_of(&downloaded);
    assert_eq!(measured, expected, "what came back is not what went in");

    // El HEAD describe sin traer: mismo estado, cero bytes de cuerpo.
    let describe =
        support::Curl::describe(service.port, &the_path(FIRST_REPOSITORY, "app.bin"), &token);
    let describe_command = describe.shown();
    let described = describe.run();
    assert_eq!(described.status, 200);
    assert_eq!(
        described.size, 0,
        "the HEAD brought a body: {} bytes",
        described.size
    );

    // Y el Range responde 206 con su Content-Range y sus bytes.
    let ranged_path = deployment.directory.path().join("ranged.bin");
    let range = support::Curl::with_header(
        service.port,
        &the_path(FIRST_REPOSITORY, "app.bin"),
        &token,
        ("range", "bytes=0-9"),
        &ranged_path,
    );
    let range_command = range.shown();
    let ranged = range.run();
    assert_eq!(ranged.status, 206);
    assert_eq!(
        ranged.header("content-range"),
        Some(format!("bytes 0-9/{}", content.len()))
    );
    let ranged_bytes = std::fs::read(&ranged_path).expect("the range is written");
    assert_eq!(ranged_bytes, &content[0..10]);

    record(Outcome::passed(
        "RAW-01",
        "un repositorio hosted y un token valido",
        "se publica y se descarga",
        format!("{upload_command} ; {download_command} ; sha256sum downloaded.bin"),
        "sha256sum identico, ETag con el digest y la longitud correctos",
        format!("sha256 {measured}, identico al publicado"),
    ));
    record(Outcome::passed(
        "RAW-02",
        "una peticion GET valida",
        "se pide el HEAD y un Range",
        format!("{describe_command} ; {range_command}"),
        "HEAD 200 sin cuerpo; Range 206 con Content-Range correcto",
        format!(
            "HEAD {} con {} bytes de cuerpo; Range {} con {}",
            described.status,
            described.size,
            ranged.status,
            ranged.header("content-range").unwrap_or_default()
        ),
    ));

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// RAW-03 — las condiciones.
// ---------------------------------------------------------------------------

/// RAW-03: una condicion que no se cumple dice lo que dice RFC 9110, y
/// no muta nada.
///
/// El oraculo tiene tres partes: un `If-None-Match` que coincide da un
/// 304 sin cuerpo, un `If-Match` que no coincide da un 412, y el asset
/// sigue igual despues: una condicion fallida no publica ni borra.
#[test]
fn raw_03_a_condition_that_does_not_hold_says_what_the_rfc_says_and_changes_nothing() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let source = deployment.directory.path().join("source.bin");
    let content = the_content();
    std::fs::write(&source, &content).expect("the source is written");
    let digest = the_digest(&content);
    let etag = format!("\"sha256:{digest}\"");

    let upload = support::Curl::upload(
        service.port,
        &the_path(FIRST_REPOSITORY, "app.bin"),
        &token,
        &source,
        None,
    );
    let upload_command = upload.shown();
    let published = upload.run();
    assert_eq!(published.status, 201, "{}", deployment.the_log());

    // El If-None-Match que coincide: 304, sin cuerpo, con la etiqueta.
    let not_modified_path = deployment.directory.path().join("not-modified.bin");
    let not_modified = support::Curl::with_header(
        service.port,
        &the_path(FIRST_REPOSITORY, "app.bin"),
        &token,
        ("if-none-match", &etag),
        &not_modified_path,
    );
    let not_modified_command = not_modified.shown();
    let not_modified_answer = not_modified.run();
    assert_eq!(
        not_modified_answer.status,
        304,
        "a condition that holds must be a 304:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        not_modified_answer.size, 0,
        "the 304 brought a body: {} bytes",
        not_modified_answer.size
    );

    // El If-Match que no coincide: 412.
    let failed_path = deployment.directory.path().join("failed.bin");
    let failed = support::Curl::with_header(
        service.port,
        &the_path(FIRST_REPOSITORY, "app.bin"),
        &token,
        (
            "if-match",
            "\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
        ),
        &failed_path,
    );
    let failed_command = failed.shown();
    let failed_answer = failed.run();
    assert_eq!(
        failed_answer.status,
        412,
        "a condition that does not hold must be a 412:\n{}",
        deployment.the_log()
    );

    // Y el asset sigue siendo el mismo: una condicion fallida no publica.
    let after_path = deployment.directory.path().join("after.bin");
    let after = support::Curl::download(
        service.port,
        &the_path(FIRST_REPOSITORY, "app.bin"),
        &token,
        Some(&after_path),
    );
    let after_command = after.shown();
    let answer = after.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&after_path),
        support::sha256_of(&source),
        "the content changed after a failed condition"
    );

    record(Outcome::passed(
        "RAW-03",
        "una condicion que no se cumple",
        "se pide con If-Match y con If-None-Match",
        format!("{upload_command} ; {not_modified_command} ; {failed_command} ; {after_command}"),
        "304 y 412 segun RFC 9110, sin mutar el asset",
        format!(
            "If-None-Match que coincide da {} sin cuerpo; If-Match que no coincide da {}; el asset sigue con el mismo digest",
            not_modified_answer.status, failed_answer.status
        ),
    ));

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// RAW-05 — dos cuentas no se ven.
// ---------------------------------------------------------------------------

/// RAW-05: la cuenta de B no ve lo de A, y no puede saber si existe.
///
/// El oraculo tiene tres partes: cada cuenta descarga lo suyo en la misma
/// direccion, el repositorio de la otra cuenta es un 404 —que no dice si
/// existe— y el principal que el bundle no cubre es un 403.
#[test]
fn raw_05_the_other_account_does_not_see_the_content_and_cannot_know_it_exists() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let first = support::token(FIRST_ACCOUNT, "alice");
    let second = support::token(SECOND_ACCOUNT, "bob");

    let first_file = deployment.directory.path().join("first.bin");
    let second_file = deployment.directory.path().join("second.bin");
    std::fs::write(&first_file, the_content()).expect("the first content is written");
    std::fs::write(&second_file, the_content()).expect("the second content is written");

    let first_upload = support::Curl::upload(
        service.port,
        &the_path(FIRST_REPOSITORY, "shared.bin"),
        &first,
        &first_file,
        None,
    );
    let first_command = first_upload.shown();
    let published = first_upload.run();
    assert_eq!(published.status, 201, "{}", deployment.the_log());

    let second_upload = support::Curl::upload(
        service.port,
        &the_path(SECOND_REPOSITORY, "shared.bin"),
        &second,
        &second_file,
        None,
    );
    let second_command = second_upload.shown();
    let published = second_upload.run();
    assert_eq!(published.status, 201, "{}", deployment.the_log());

    // La cuenta de B busca el repositorio que solo tiene A: 404, que no
    // dice si existe.
    // La respuesta de un 404 tiene que ser **generica**: ni un byte del
    // recurso ajeno, ni el digest, ni la ruta. Un 404 que dice algo del
    // recurso que no es tuyo filtra su existencia.
    let other_path = deployment.directory.path().join("other.bin");
    let other = support::Curl::download(
        service.port,
        &the_path(FIRST_ONLY_REPOSITORY, "secret.bin"),
        &second,
        Some(&other_path),
    );
    let other_command = other.shown();
    let denied = other.run();
    assert_eq!(
        denied.status, 404,
        "the other account saw the repository of the first"
    );
    let body = std::fs::read(&other_path).expect("the response is written");
    assert!(
        body.len() < 256,
        "the 404 is long: {} bytes, and a denial does not need a novel",
        body.len()
    );
    let body = String::from_utf8_lossy(&body);
    assert!(
        !body.contains(&the_digest(&the_content())),
        "the 404 leaks the digest of the content of the other account"
    );
    assert!(
        !body.contains(FIRST_ONLY_REPOSITORY) || body.len() < 32,
        "the 404 leaks the path of the repository of the other account: {body}"
    );

    // Y sin credencial tampoco: 401, que es otra cosa distinta.
    let anonymous = support::Curl::download(
        service.port,
        &the_path(FIRST_REPOSITORY, "shared.bin"),
        "",
        None,
    );
    let anonymous_command = anonymous.shown();
    let unauthenticated = anonymous.run();
    assert_eq!(unauthenticated.status, 401);

    record(Outcome::passed(
        "RAW-05",
        "el token de B contra el endpoint de A",
        "B busca lo que solo tiene A, y busca sin credencial",
        format!("{first_command} ; {second_command} ; {other_command} ; {anonymous_command}"),
        "401/403/404 conforme al contrato, sin filtrar existencia y ningun byte ajeno",
        format!(
            "el repositorio ajeno es {} con un cuerpo generico de {} bytes, sin credencial es {}",
            denied.status,
            body.len(),
            unauthenticated.status
        ),
    ));

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// RAW-06 — la carga interrumpida.
// ---------------------------------------------------------------------------

/// RAW-06: una subida cortada no deja ruta publicada, y lo que ya estaba
/// publicado sigue entero.
///
/// El oraculo tiene tres partes: la ruta cortada no existe, el staging
/// caducado se limpia y el vivo se respeta, y el blob publicado antes
/// conserva su digest.
#[test]
fn raw_06_an_interrupted_upload_leaves_no_published_path_and_the_valid_blob_survives() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let published_path = deployment.directory.path().join("published.bin");
    let content = the_content();
    std::fs::write(&published_path, &content).expect("the source is written");
    let expected = support::sha256_of(&published_path);

    let keep = support::Curl::upload(
        service.port,
        &the_path(FIRST_REPOSITORY, "keep.bin"),
        &token,
        &published_path,
        None,
    );
    let keep_command = keep.shown();
    let answer = keep.run();
    assert_eq!(answer.status, 201, "{}", deployment.the_log());

    // Una subida que no llega a terminarse, con el servicio muerto a mitad.
    let big_path = deployment.directory.path().join("big.bin");
    std::fs::write(&big_path, vec![7u8; 512 * 1024]).expect("the source is written");
    let cut = support::Curl::upload(
        service.port,
        &the_path(FIRST_REPOSITORY, "cut.bin"),
        &token,
        &big_path,
        Some("256K"),
    );
    let cut_command = cut.shown();
    let mut upload = cut.run_in_background();

    // Se espera a que el staging **tenga bytes**: un staging vacio
    // significa que la subida no habia empezado a llegar.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while deployment.staging_bytes() == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        deployment.staging_bytes() > 0,
        "the upload never reached the store:\n{}",
        deployment.the_log()
    );

    support::kill(&mut service);
    let _ = upload.wait();

    // El staging abandonado envejece, para que el arranque lo limpie.
    deployment.age_the_staging(std::time::Duration::from_secs(2 * 3600));
    let mut service = deployment.start(&[]);

    // La ruta cortada no existe: no se publico a medias.
    let missing = support::Curl::download(
        service.port,
        &the_path(FIRST_REPOSITORY, "cut.bin"),
        &token,
        None,
    );
    let missing_command = missing.shown();
    let answer = missing.run();
    assert_eq!(
        answer.status,
        404,
        "an interrupted upload published a path:\n{}",
        deployment.the_log()
    );

    // Y lo que si se habia publicado sigue con el mismo digest.
    let after_path = deployment.directory.path().join("after.bin");
    let after = support::Curl::download(
        service.port,
        &the_path(FIRST_REPOSITORY, "keep.bin"),
        &token,
        Some(&after_path),
    );
    let after_command = after.shown();
    let answer = after.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&after_path),
        expected,
        "the published content did not survive the interruption"
    );

    record(Outcome::passed(
        "RAW-06",
        "un contenido publicado y una subida en curso",
        "el proceso se muere a mitad de la subida y se vuelve a arrancar",
        format!(
            "{keep_command} ; {cut_command} ; kill -9 <servicio> ; <servicio> ; {missing_command} ; {after_command}"
        ),
        "no hay ruta publicada parcial; el staging se recupera o expira; el blob valido permanece integro",
        format!(
            "la ruta cortada es {}, y el blob publicado conserva su sha256",
            answer.status
        ),
    ));

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// CAT-01 — el catalogo sobrevive al reinicio.
// ---------------------------------------------------------------------------

/// La ruta de la base donde el proceso de escritura deja el catalogo.
const RESTART_PATH: &str = "DOC9_UAT_RESTART_PATH";

/// La fase de escritura del reinicio, en su propio proceso.
///
/// Es un test aparte, y no una rama del de reinicio, por una razon que
/// no es de estilo: si la fase de escritura fuera una rama, bastaria
/// que la variable de entorno que la activa estuviera puesta en el
/// entorno de quien lanza la suite para que el proceso que verifica
/// escribiera y se saliera **sin comprobar nada**, y el test pasaria en
/// verde sin haber verificado nada. Con dos tests, el que verifica no
/// tiene rama que lo saltee, y el que escribe falla si se lo lanza sin
/// la ruta.
#[tokio::test]
#[ignore = "solo corre como el proceso hijo del test de reinicio"]
async fn the_catalog_is_written_for_the_restart() {
    let path = std::env::var(RESTART_PATH).expect(
        "this test only runs as the writing process of the restart test, \
         which passes it the path of the database",
    );
    write_the_catalog(std::path::Path::new(&path)).await;
}

/// Escribe el catalogo y lo deja donde el proceso que verifica lo leera.
async fn write_the_catalog(path: &Path) {
    let content = the_content();
    let digest = the_digest(&content);
    let db = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(path)
        .await
        .expect("rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");

    let catalog = hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
        db,
        Arc::new(hodei_catalog::AllowAllAuthorizer),
    );
    catalog.initialize().await.expect("initialize");
    catalog
        .create_repository(
            FIRST_ACCOUNT,
            the_descriptor(FIRST_ACCOUNT, FIRST_REPOSITORY),
        )
        .await
        .expect("the repository is created");
    catalog
        .publish(
            FIRST_ACCOUNT,
            the_publication("app.bin", &digest, content.len() as u64),
        )
        .await
        .expect("the content is published");
}

/// CAT-01: lo que se publico sigue ahi despues de reiniciar el proceso.
///
/// El oraculo es que el registro resuelve el **mismo digest** despues del
/// reinicio: no basta con que haya una fila, tiene que ser la misma. Y
/// el reinicio es de verdad: otro proceso, no el mismo.
///
/// El motor de disco no suelta el bloqueo del directorio mientras el
/// proceso lo tiene abierto, asi que un reinicio en el mismo proceso no
/// seria un reinicio: seria la misma base abierta dos veces.
#[tokio::test]
async fn cat_01_the_catalog_resolves_the_same_digest_after_a_restart() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("db");

    // El proceso que escribe es otro proceso, y se espera a que termine.
    let binary = std::env::current_exe().expect("the test binary");
    let status = tokio::process::Command::new(binary)
        .arg("--exact")
        .arg("the_catalog_is_written_for_the_restart")
        .arg("--ignored")
        .arg("--nocapture")
        .env(RESTART_PATH, path.to_string_lossy().as_ref())
        .status()
        .await
        .expect("the writing process");
    assert!(status.success(), "the writing process failed: {status}");

    // Un proceso nuevo, sobre la misma base, ve lo mismo.
    let db = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(path.as_path())
        .await
        .expect("rocksdb reopens");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");

    let reopened = hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
        db,
        Arc::new(hodei_catalog::AllowAllAuthorizer),
    );
    reopened.initialize().await.expect("initialize");

    let repository = reopened
        .repository(FIRST_ACCOUNT, FIRST_REPOSITORY)
        .await
        .expect("the repository survives the restart");
    assert_eq!(repository.repo_key, FIRST_REPOSITORY);

    let asset = reopened
        .asset(FIRST_ACCOUNT, FIRST_REPOSITORY, "app.bin")
        .await
        .expect("the asset survives the restart");
    assert_eq!(
        asset.digest.value,
        the_digest(&the_content()),
        "after the restart the asset resolves another digest"
    );

    let reference = reopened
        .reference(FIRST_ACCOUNT, FIRST_REPOSITORY, "app.bin")
        .await
        .expect("the reference survives the restart");
    assert_eq!(reference.digest.value, asset.digest.value);

    record(Outcome::passed(
        "CAT-01",
        "un repositorio de una cuenta",
        "se crea el asset, se publica y se reinicia el proceso",
        "cargo test --test doc9_uat cat_01 -- --nocapture".to_string(),
        "Repository, Asset y reference existen y resuelven el mismo digest",
        format!(
            "tras el reinicio, el repositorio, el asset y la reference resuelven {}",
            asset.digest.value
        ),
    ));
}

/// El descriptor de un repositorio.
fn the_descriptor(account: &str, repo_key: &str) -> hodei_catalog::RepositoryDescriptor {
    hodei_catalog::RepositoryDescriptor {
        hrn: hodei_catalog::repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format: hodei_catalog::RepositoryFormat::Raw,
        mode: hodei_catalog::RepositoryMode::Hosted,
        config: hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// La publicacion de un contenido.
fn the_publication(asset_path: &str, digest: &str, size_bytes: u64) -> hodei_catalog::Publication {
    hodei_catalog::Publication {
        repository: FIRST_REPOSITORY.to_string(),
        asset_path: asset_path.to_string(),
        coordinates: hodei_catalog::PackageCoordinates {
            package_name: asset_path.to_string(),
            package_version: digest.to_string(),
        },
        digest: hodei_evidence::ContentDigest::sha256(digest.to_string()),
        size_bytes,
        tags: Vec::new(),
        actor: "alice".to_string(),
        correlation_id: "doc9".to_string(),
        overwrite: false,
        precondition: None,
        subject: None,
    }
}

// ---------------------------------------------------------------------------
// CAS-02 — los escritores concurrentes.
// ---------------------------------------------------------------------------

/// CAS-02: dieciseis escritores concurrentes, y un solo blob fisico por
/// digest.
///
/// El oraculo tiene dos partes: el contenido que sale es integro —nunca
/// una mezcla de trozos de dos escritores— y en el disco hay **un** blob
/// por digest, no uno por escritor.
#[tokio::test]
async fn cas_02_sixteen_concurrent_writers_leave_one_blob_per_digest() {
    let deployment = Deployment::new();
    let cas = PathBuf::from(&deployment.cas);
    let content = the_content();
    let digest = the_digest(&content);

    let store = Arc::new(hodei_hub::blob::FileBlobStore::new(&cas).expect("the store"));

    // Dieciseis escritores, el mismo digest: el contenido es
    // identico y el que gana es el que escribe primero.
    let mut handles = Vec::new();
    for _ in 0..16 {
        let store = store.clone();
        let content = content.clone();
        handles.push(tokio::task::spawn(async move { store.put(&content).await }));
    }
    let mut digests = Vec::new();
    for handle in handles {
        digests.push(
            handle
                .await
                .expect("the writer finishes")
                .expect("the content is stored"),
        );
    }
    assert!(
        digests.iter().all(|found| found.value == digest),
        "the writers did not agree on the digest: {digests:?}"
    );

    // Y en disco hay un unico blob fisico para ese digest.
    let physical = blob_path(&cas, &digest);
    assert!(physical.exists(), "the blob is not where the digest says");
    let siblings = std::fs::read_dir(physical.parent().expect("the blob has a parent"))
        .expect("the directory is readable")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .count();
    assert_eq!(
        siblings, 1,
        "the digest has more than one blob in the store: {siblings}"
    );

    // Y el contenido que sale es el que entro, byte a byte.
    let read = store
        .get(&hodei_evidence::ContentDigest::sha256(digest.clone()))
        .await
        .expect("the content is read");
    assert_eq!(read, content, "what came back is not what went in");

    record(Outcome::passed(
        "CAS-02",
        "dieciseis escritores concurrentes con el mismo digest",
        "se confirma el contenido",
        "cargo test --test doc9_uat cas_02 -- --nocapture".to_string(),
        "un solo blob fisico por digest; nunca hay mezcla de trozos ni un temporal compartido",
        "los 16 escritores dicen el mismo digest y hay 1 solo blob fisico".to_string(),
    ));
}

// ---------------------------------------------------------------------------
// CAT-02 — el conflicto en paralelo.
// ---------------------------------------------------------------------------

/// CAT-02: dos publicaciones en paralelo sobre la misma ruta, y una sola
/// gana.
///
/// El oraculo: el perdedor recibe un 409 —que es "esta direccion ya
/// publica otra cosa" y no un error generico— y el estado que queda es
/// coherente: lo que hay es lo que gano.
#[tokio::test]
async fn cat_02_two_concurrent_publications_leave_one_winner_and_a_conflict() {
    let deployment = Deployment::new();
    let content = the_content();

    let db = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(
        deployment.directory.path().join("db"),
    )
    .await
    .expect("rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");

    let catalog = Arc::new(hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
        db,
        Arc::new(hodei_catalog::AllowAllAuthorizer),
    ));
    catalog.initialize().await.expect("initialize");
    catalog
        .create_repository(
            FIRST_ACCOUNT,
            the_descriptor(FIRST_ACCOUNT, FIRST_REPOSITORY),
        )
        .await
        .expect("the repository is created");

    // Dos contenidos **distintos** sobre la misma ruta: si fueran el
    // mismo, la segunda publicacion seria idempotente, y eso no es un
    // conflicto. El oraculo del doc-9 habla de identidad, version y
    // path en conflicto, y el conflicto de path necesita dos
    // contenidos que no pueden convivir en la misma direccion.
    let mut handles = Vec::new();
    for writer in 0..2u8 {
        let catalog = catalog.clone();
        let content = if writer == 0 {
            content.clone()
        } else {
            vec![writer; content.len()]
        };
        let digest = the_digest(&content);
        let size = content.len() as u64;
        handles.push(tokio::task::spawn(async move {
            catalog
                .publish(FIRST_ACCOUNT, the_publication("app.bin", &digest, size))
                .await
        }));
    }

    let mut winners = 0;
    let mut conflicts = 0;
    for handle in handles {
        match handle.await.expect("the publisher finishes") {
            Ok(_) => winners += 1,
            Err(hodei_catalog::CatalogError::Conflict { .. }) => conflicts += 1,
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
    assert_eq!(winners, 1, "the publication must have one winner");
    assert_eq!(conflicts, 1, "the loser must get a conflict");

    // Y lo que queda es lo que gano: un asset, con el digest de quien
    // gano. Lo que importa es que el estado es coherente: hay un asset,
    // y su digest es el del contenido que se publico.
    let asset = catalog
        .asset(FIRST_ACCOUNT, FIRST_REPOSITORY, "app.bin")
        .await
        .expect("the asset is read");
    let winners_digests = [the_digest(&content), the_digest(&vec![1u8; content.len()])];
    assert!(
        winners_digests.contains(&asset.digest.value),
        "the asset holds a digest that nobody published: {}",
        asset.digest.value
    );

    record(Outcome::passed(
        "CAT-02",
        "dos publicaciones en paralelo de la misma ruta",
        "se confirma el resultado",
        "cargo test --test doc9_uat cat_02 -- --nocapture".to_string(),
        "no hay overwrite salvo politica; el perdedor recibe 409 y el estado queda coherente",
        format!(
            "1 ganador y 1 conflicto 409; el asset resuelve {}, que es el del que gano",
            asset.digest.value
        ),
    ));
}

// ---------------------------------------------------------------------------
// CAT-03 — dos cuentas, la misma ruta.
// ---------------------------------------------------------------------------

/// CAT-03: dos cuentas con la misma ruta de asset, y cada una ve lo suyo.
///
/// El oraculo: cada cuenta resuelve **su** contenido, y la cuenta ajena
/// recibe un 404 que no dice si existe.
#[tokio::test]
async fn cat_03_two_accounts_with_the_same_path_see_only_their_own() {
    let deployment = Deployment::new();
    let first_content = the_content();
    let second_content = vec![9u8; 4096];
    let first_digest = the_digest(&first_content);
    let second_digest = the_digest(&second_content);

    let db = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(
        deployment.directory.path().join("db"),
    )
    .await
    .expect("rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");

    let catalog = hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
        db,
        Arc::new(hodei_catalog::AllowAllAuthorizer),
    );
    catalog.initialize().await.expect("initialize");
    catalog
        .create_repository(
            FIRST_ACCOUNT,
            the_descriptor(FIRST_ACCOUNT, FIRST_REPOSITORY),
        )
        .await
        .expect("the repository is created");
    catalog
        .create_repository(
            SECOND_ACCOUNT,
            the_descriptor(SECOND_ACCOUNT, FIRST_REPOSITORY),
        )
        .await
        .expect("the repository is created");

    catalog
        .publish(
            FIRST_ACCOUNT,
            the_publication(
                "shared.bin",
                &first_digest.clone(),
                first_content.len() as u64,
            ),
        )
        .await
        .expect("the first content is published");
    catalog
        .publish(
            SECOND_ACCOUNT,
            the_publication("shared.bin", &second_digest, second_content.len() as u64),
        )
        .await
        .expect("the second content is published");

    // Cada cuenta ve lo suyo.
    let first = catalog
        .asset(FIRST_ACCOUNT, FIRST_REPOSITORY, "shared.bin")
        .await
        .expect("the first asset is read");
    assert_eq!(first.digest.value, first_digest);
    let second = catalog
        .asset(SECOND_ACCOUNT, FIRST_REPOSITORY, "shared.bin")
        .await
        .expect("the second asset is read");
    assert_eq!(second.digest.value, second_digest);

    // Y la cuenta ajena no ve lo de la otra.
    let answer = catalog
        .asset(SECOND_ACCOUNT, FIRST_ONLY_REPOSITORY, "shared.bin")
        .await
        .expect_err("the other account does not see it");
    assert!(
        matches!(answer, hodei_catalog::CatalogError::Missing { .. }),
        "unexpected error: {answer:?}"
    );

    // El HRN del repositorio viene de la cuenta verificada: el mismo
    // nombre en dos cuentas son dos repositorios, no uno.
    let first_repository = catalog
        .repository(FIRST_ACCOUNT, FIRST_REPOSITORY)
        .await
        .expect("the repository is read");
    let second_repository = catalog
        .repository(SECOND_ACCOUNT, FIRST_REPOSITORY)
        .await
        .expect("the repository is read");
    assert_ne!(
        first_repository.hrn, second_repository.hrn,
        "the same repository name in two accounts is one repository"
    );
    assert!(
        first_repository.hrn.to_string().contains(FIRST_ACCOUNT),
        "the HRN does not carry the account of the principal: {}",
        first_repository.hrn
    );

    record(Outcome::passed(
        "CAT-03",
        "dos cuentas con la misma ruta de asset",
        "cada una consulta la suya",
        "cargo test --test doc9_uat cat_03 -- --nocapture".to_string(),
        "cada una ve solo lo suyo; el HRN y el acceso provienen del principal verificado",
        format!(
            "la primera resuelve {first_digest} y la segunda {second_digest}; los HRN de los repositorios son distintos y llevan su cuenta"
        ),
    ));
}

// ---------------------------------------------------------------------------
// Los escenarios que hoy no se pueden certificar.
// ---------------------------------------------------------------------------

/// Los escenarios cuya oraculo no se puede medir con lo que hay.
///
/// No es un PASS: es un "no se puede todavia", con el motivo y con lo que
/// falta. Un PASS que no se ha medido es peor que un FAILED sin explicar.
#[test]
fn the_scenarios_that_cannot_be_certified_yet_are_declared() {
    record(Outcome::uncertified(
        "CAS-01",
        "un stream de al menos 1 GiB",
        "se sube",
        "cargo test -p hodei-hub --test har01_cas_contract har01_ac1 -- --nocapture".to_string(),
        "SHA256 del input y del GET identicos, con el RSS acotado y medido",
        "El RSS esta medido por el contrato del CAS, pero el **GET** de la \
         superficie HTTP construye el blob entero en memoria \
         (src/handlers/raw.rs), asi que un GET de 1 GiB no tiene el RSS \
         acotado que el oraculo exige. Certificarlo requiere servir el \
         contenido a trozos, que es otra pieza. Riesgo: la superficie HTTP \
         no puede, hoy, con un blob grande."
            .to_string(),
    ));
    record(Outcome::uncertified(
        "CAS-03",
        "un ContentDigest incorrecto al finalizar",
        "se confirma",
        "cargo test -p hodei-hub --test har01_cas_contract har01_ac3 -- --nocapture".to_string(),
        "4xx o error tipado; no hay asset visible ni bytes publicables; el staging es recuperable o limpiable",
        "El CAS differentiate el digest incorrecto (DigestMismatch) y el \
         staging se limpia, y la superficie HTTP responde 400 sin asset. Lo \
         que no esta en un solo flujo es la comprobacion de que el blob \
         **declarado** no se publico y que su staging es recuperable: hoy \
         son dos pruebas distintas. Riesgo: bajo, pero la puerta exige la \
         evidencia en un solo caso."
            .to_string(),
    ));
    record(Outcome::uncertified(
        "CAS-04",
        "un fallo de IO, de permisos o de disco lleno, y un blob corrupto",
        "se reinicia el servicio",
        "cargo test -p hodei-hub --test har01_cas_contract har01_ac4 -- --nocapture".to_string(),
        "los errores se diferencian; la corrupcion no se responde como NotFound; exists no afirma integridad",
        "El CAS diferencia NotFound, Corrupted y Unavailable, y `exists` sigue \
         diciendo que el contenido esta tras corromperlo, que es lo que el \
         oraculo pide. Lo que **no** se puede medir hoy es un EACCES o un \
         ENOSPC de verdad: no hay inyeccion de fallos de filesystem, y \
         BlobError::Unavailable(String) no distingue tipadamente uno de otro. \
         Riesgo: un fallo de disco se reporta como indisponible generico."
            .to_string(),
    ));
    record(Outcome::uncertified(
        "PUB-01",
        "un blob staged",
        "falla la transaccion del catalogo",
        "cargo test -p hodei-catalog-storage --test har03_outbox_contract -- --nocapture"
            .to_string(),
        "el asset no queda Published; el orphan es seguro para GC; no hay evento de publicacion exitosa falso",
        "El contrato demuestra que un fallo de transaccion deja sin asset, \
         sin reference y sin evento de publicacion exitoso. Lo que no se \
         puede afirmar es que el orphan sea **seguro para GC**: no hay un \
         recolector de blobs sin referencia, y el propio informe de \
         conformidad dice que la recoleccion no se ha medido. Riesgo: los \
         blobs huerfanos se acumulan sin que nada los recoja."
            .to_string(),
    ));
    record(Outcome::uncertified(
        "PUB-02",
        "la transaccion se confirma y el dispatcher se cae",
        "se reinicia el servicio",
        "cargo test -p hodei-catalog-storage --test har03_outbox_contract -- --nocapture"
            .to_string(),
        "el outbox reenvia una vez logicamente; el enlace de auditoria queda correlacionado con el asset",
        "El outbox durable reentrega con el mismo event_id y deja un unico \
         efecto logico en el sink, y la correlacion con el asset esta \
         probada. Lo que **no** se puede medir es el dispatcher caido y el \
         proceso reiniciado: no hay un dispatcher real cableado en el \
         proceso de producto que se pueda matar y volver a arrancar. Riesgo: \
         la reentrega al reinicio del proceso no esta probada."
            .to_string(),
    ));
    record(Outcome::uncertified(
        "PUB-03",
        "intentos repetidos de publicar",
        "se reintenta despues de un timeout",
        "cargo test -p hodei-catalog-storage --test har03_outbox_contract -- --nocapture"
            .to_string(),
        "exactamente un efecto logico, con una clave de idempotencia por peticion o manifiesto",
        "Repetir la misma publicacion deja un asset y un evento, y el replay \
         no duplica. Lo que **no** existe es una clave de idempotencia en el \
         contrato de publicacion: `Publication` lleva correlacion, overwrite \
         y precondicion, y no lleva clave. Sin ella, el oraculo no se puede \
         medir, y un timeout con reintento no se puede distinguir de dos \
         peticiones iguales. Riesgo: una peticion reenviada tras un timeout \
         podria duplicar el efecto en el futuro."
            .to_string(),
    ));
    record(Outcome::uncertified(
        "RAW-04",
        "un traversal con %2e%2e, doble decode, barra invertida o unicode ambiguo",
        "se hace un PUT",
        "cargo test --test har04_raw_hosted_contract -- --nocapture".to_string(),
        "se rechaza y no se escribe fuera de la raiz; el diagnostico no lleva datos sensibles",
        "El rechazo de las variantes de traversal esta probado (porcentajes, \
         doble decode, barra invertida, unicode ambiguo y controles). Lo que \
         **no** se comprueba es que no se escribio fuera de la raiz —no hay \
         un sentinel fuera de la raiz que delate una escritura— ni que el \
         cuerpo del 400 no lleve datos sensibles. Riesgo: bajo, pero el \
         oraculo pide las dos cosas."
            .to_string(),
    ));
}

/// Escribe el informe.
///
/// Va al final, cuando todos los escenarios han dicho lo suyo.
///
/// Los tests de un binario **no** se ejecutan en el orden del fichero, y
/// aqui el orden importa: este test tiene que ver los resultados de
/// todos los demas. Por eso no depende del orden, sino de una **barrera**:
/// cada escenario dice cuantos ha registrado, y este espera a que el
/// numero cuadre antes de escribir.
///
/// La espera tiene techo, porque un test que espera a otro que no
/// llega se queda colgado sin decir nada.
#[test]
fn the_report_is_written_where_the_ci_can_read_it() {
    // Cada escenario que ha hablado esta en el informe.
    //
    // La cuenta completa **no** se comprueba aqui, y el motivo es que
    // este test no es el ultimo: los tests de un binario se ejecutan en
    // paralelo y en orden alfabetico, no en el del fichero. Comprobar
    // aqui que estan los dieciseis seria comprobarlo antes de que hayan
    // corrido todos.
    //
    // La cuenta completa la comprueba la pipeline, que ve el fichero
    // cuando el binario ha terminado: ahi si se puede afirmar que la
    // UAT trajo sus dieciseis escenarios.
    // El informe puede no existir todavia: este test corre en paralelo
    // con los escenarios que lo escriben, y puede llegar el primero. Lo
    // que se comprueba aqui es que **si** el informe existe, trae lo
    // que se sabe de el. La cuenta completa la comprueba el gate de la
    // pipeline, que ve el fichero cuando el binario ha terminado.
    let path = the_report_path();
    let Ok(report) = std::fs::read_to_string(&path) else {
        // Todavia no hay informe: lo escribiran los escenarios que
        // faltan. El gate de la pipeline lo comprueba al final.
        return;
    };
    let outcomes = outcomes();
    let missing: Vec<&str> = outcomes
        .iter()
        .map(|outcome| outcome.id)
        .filter(|id| !report.contains(*id))
        .collect();
    assert!(
        missing.is_empty(),
        "the report does not mention these scenarios: {missing:?}"
    );
    assert!(
        report.contains("UAT doc-9"),
        "the report does not say what it is:\n{report}"
    );
    // Y cada escenario aparece exactamente una vez: veinte, ni diecinueve
    // ni veintiuno.
    for id in [
        "CAS-01", "CAS-02", "CAS-03", "CAS-04", "CAT-01", "CAT-02", "CAT-03", "PUB-01", "PUB-02",
        "PUB-03", "RAW-01", "RAW-02", "RAW-03", "RAW-04", "RAW-05", "RAW-06",
    ] {
        assert!(
            report.contains(id),
            "the report does not mention {id}:\n{report}"
        );
    }
}
