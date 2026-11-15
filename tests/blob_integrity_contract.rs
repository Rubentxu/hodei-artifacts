//! La sonda de integridad del contenido, conectada al arranque.
//!
//! El puerto `BlobIntegrityProbe` existe y el catálogo lo consume, pero en
//! producción **nadie lo conecta**: `with_probe` no lo llama nadie y el
//! almacén de contenido se monta sin sonda. La consecuencia es que un blob
//! que no está donde el catálogo dice **se sirve como si estuviera sano**,
//! y la corrupción se confunde con la ausencia.
//!
//! Estas pruebas van contra el proceso **de verdad**: un binario real, un
//! almacén de bytes real en disco y un catálogo real. El defecto no se ve
//! en un doble de prueba: se ve cuando el byte guardado no es el que el
//! catálogo dice que hay.

mod support;

use std::path::PathBuf;

use hodei_catalog::{BlobIntegrityProbe, CatalogReader, CatalogWriter};
use hodei_hub::blob::BlobStore;
use support::{Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

/// El semillero del catálogo.
///
/// El despliegue siembra el catálogo en un proceso aparte, y este test es
/// el que lo hace cuando el proceso le pide: el arnés pasa la ruta de la
/// base por la variable `HODEI_SEED_DB`, y este es el que la escribe.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Donde vive un blob en el almacén de bytes.
///
/// El almacén reparte los blobs en dos niveles por el principio del
/// digest, así que la ruta sale del propio digest y no de donde se
/// guardó.
fn blob_path(root: &std::path::Path, digest: &str) -> PathBuf {
    root.join(&digest[..2]).join(&digest[2..4]).join(digest)
}

/// La ruta del contenido dentro del repositorio.
fn the_path(path: &str) -> String {
    format!("/repository/{FIRST_REPOSITORY}/{path}")
}

/// El contenido que se publica.
fn the_content() -> Vec<u8> {
    (0u16..4096).map(|value| (value % 251) as u8).collect()
}

/// Un blob que no esta donde el catálogo dice.
///
/// El catálogo afirma que el blob tiene esos bytes; el almacen no los
/// tiene. Con la sonda conectada, esa contradiccion se responde como
/// **corrupto**, que es "hay algo ahi y no se puede leer bien". Sin ella,
/// el catálogo responde que el asset esta, y el fallo aparece despues,
/// como un 404: el cliente ve que no existe, y borra su copia.
///
/// Un `Corrupt` donde habia un `Missing` hace que el cliente borre algo
/// que no era suyo. Un `Missing` donde habia un `Corrupt` hace que el
/// cliente repita. Confundirlos es peor que no responder.
#[test]
fn a_blob_that_is_not_where_the_catalog_says_is_corrupt_and_not_missing() {
    let deployment = Deployment::new();
    deployment.seed();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, the_content()).expect("the source is written");
    let digest = hodei_evidence::ContentDigest::of_bytes(&the_content()).value;

    let published = support::curl_upload(service.port, &the_path("app.bin"), &token, &source, None);
    assert_eq!(
        published.status,
        201,
        "the content is published: status={} body={:?}\n{}",
        published.status,
        published.body,
        deployment.the_log()
    );

    // Y ahora el blob desaparece del almacen de bytes: el catalogo sigue
    // diciendo que el asset esta, con sus bytes y su longitud.
    let blob = blob_path(&PathBuf::from(&deployment.cas), &digest);
    std::fs::remove_file(&blob).expect("the blob is removed from the store");

    let answer = support::curl_download(service.port, &the_path("app.bin"), &token, None);

    // Lo que se quiere no es un 404: un 404 le dice al cliente que su
    // copia no existe, y el cliente la borra. Lo que se quiere es un
    // 503: el servidor tiene el asset y no puede servirlo, y el cliente
    // reintenta.
    assert_eq!(
        answer.status,
        503,
        "a blob that is not in the store must not be said to be missing:\n{}",
        deployment.the_log()
    );

    support::stop(&mut service);
}

/// Un blob cuyos bytes no son los publicados se responde como corrupto.
///
/// El byte corrupto tiene **la misma longitud** que el bueno: la longitud
/// no lo delata, y la sonda no recalcula el digest —eso es lo barato, y
/// lo profundo es tarea del scrub—. Lo que lo delata es que los bytes ya
/// no son los que su digest dice, y por eso la lectura lo dice.
#[test]
fn a_blob_whose_bytes_are_not_the_published_ones_is_not_served_as_healthy() {
    let deployment = Deployment::new();
    deployment.seed();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    let content = the_content();
    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, &content).expect("the source is written");
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;

    let published = support::curl_upload(service.port, &the_path("app.bin"), &token, &source, None);
    assert_eq!(
        published.status,
        201,
        "the content is published:\n{}",
        deployment.the_log()
    );

    // El mismo digest, la misma longitud, otros bytes.
    let mut other = content.clone();
    other[0] = other[0].wrapping_add(1);
    let blob = blob_path(&PathBuf::from(&deployment.cas), &digest);
    std::fs::write(&blob, &other).expect("the blob is altered on disk");

    let answer = support::curl_download(service.port, &the_path("app.bin"), &token, None);

    // El contenido no puede servirse como si fuera el publicado.
    assert_ne!(
        answer.status, 200,
        "the corrupted content was served as the published one"
    );
    assert!(
        answer.body != other,
        "the bytes served are the corrupted ones"
    );

    support::stop(&mut service);
}

/// La sonda comprueba la longitud, y no recalcula el digest.
///
/// Es lo barato: leer los **metadatos** de un fichero no cuesta lo que
/// leer su contenido, y recalcular el digest en cada lectura seria una
/// lectura entera por peticion. La verificacion profunda —que los bytes
/// sean los que su digest dice— es tarea del scrub, que la hace cuando
/// puede y no en cada peticion.
///
/// El blob de esta prueba tiene la longitud que el catalogo declara y
/// bytes que no son los que su digest dice: la sonda lo acepta, porque lo
/// que mira es la longitud. Eso no es un fallo de la sonda: es su
/// contrato, y un contrato que se cumple es mejor que una garantia que
/// nadie nota.
#[tokio::test]
async fn the_probe_checks_the_length_and_not_the_digest() {
    let deployment = Deployment::new();
    let cas = std::path::PathBuf::from(&deployment.cas);
    let content = the_content();
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;

    let store = hodei_hub::blob::FileBlobStore::new(&cas).expect("the store");
    store.put(&content).await.expect("the content is stored");

    // Misma longitud, otros bytes.
    let mut other = content.clone();
    other[0] = other[0].wrapping_add(1);
    std::fs::write(blob_path(&cas, &digest), &other).expect("the blob is altered");

    let probe = hodei_artifacts_api::blob_probe::BlobStoreProbe::new(std::sync::Arc::new(
        hodei_hub::blob::FileBlobStore::new(&cas).expect("the store"),
    ));
    let location = hodei_catalog::BlobLocation::new(
        FIRST_ACCOUNT,
        hodei_catalog::DigestScope::Global,
        hodei_evidence::ContentDigest::sha256(digest.clone()),
    );
    let length = probe.length_of(&location).await.expect("the store answers");
    assert_eq!(
        length,
        content.len() as u64,
        "the probe answers with the length the file has, not with the digest it should have"
    );
}

/// El catalogo sin sonda sigue funcionando.
///
/// La sonda **mejora** la respuesta, no es un requisito para arrancar: un
/// catalogo sin sonda sigue leyendo y publicando, y lo que no puede es
/// afirmar que los bytes esten donde dice.
#[tokio::test]
async fn a_catalog_without_a_probe_still_publishes_and_reads() {
    let deployment = Deployment::new();
    let cas = std::path::PathBuf::from(&deployment.cas);
    let content = the_content();
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;

    let store = hodei_hub::blob::FileBlobStore::new(&cas).expect("the store");
    store.put(&content).await.expect("the content is stored");

    let db = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(
        deployment.directory.path().join("db"),
    )
    .await
    .expect("rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");

    // Sin sonda: el store se construye igual, y el contenido se publica.
    let catalog = hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
        db,
        std::sync::Arc::new(hodei_catalog::AllowAllAuthorizer),
    );
    catalog.initialize().await.expect("initialize");
    catalog
        .create_repository(
            FIRST_ACCOUNT,
            hodei_catalog::RepositoryDescriptor {
                hrn: hodei_catalog::repository_hrn(
                    "hodei",
                    "artifacts",
                    "eu",
                    FIRST_ACCOUNT,
                    FIRST_REPOSITORY,
                ),
                account: FIRST_ACCOUNT.to_string(),
                repo_key: FIRST_REPOSITORY.to_string(),
                format: hodei_catalog::RepositoryFormat::Raw,
                mode: hodei_catalog::RepositoryMode::Hosted,
                config: hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .expect("the repository is created");
    catalog
        .publish(
            FIRST_ACCOUNT,
            hodei_catalog::Publication {
                repository: FIRST_REPOSITORY.to_string(),
                asset_path: "app.bin".to_string(),
                coordinates: hodei_catalog::PackageCoordinates {
                    package_name: "app.bin".to_string(),
                    package_version: digest.clone(),
                },
                digest: hodei_evidence::ContentDigest::sha256(digest.clone()),
                size_bytes: content.len() as u64,
                tags: Vec::new(),
                actor: "alice".to_string(),
                correlation_id: "no-probe".to_string(),
                overwrite: false,
                precondition: None,
            },
        )
        .await
        .expect("the content is published");

    // Y el asset se lee sin sonda, sin que nadie pregunte por los bytes.
    let asset = catalog
        .asset(FIRST_ACCOUNT, FIRST_REPOSITORY, "app.bin")
        .await
        .expect("the asset is read");
    assert_eq!(asset.size_bytes, content.len() as u64);
}

/// La sonda no cambia el aislamiento entre cuentas.
///
/// La sonda pregunta por la longitud de un blob, y la pregunta lleva la
/// cuenta del que pregunta: el mismo digest puede estar en la ranura
/// global o en la particionada de una cuenta, y preguntar por el digest a
/// secas solo encontraria la primera.
#[test]
fn the_probe_does_not_change_the_isolation_between_accounts() {
    let deployment = Deployment::new();
    deployment.seed();
    let mut service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let other = support::token("other", "bob");

    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, the_content()).expect("the source is written");

    let published = support::curl_upload(service.port, &the_path("app.bin"), &token, &source, None);
    assert_eq!(
        published.status,
        201,
        "the content is published:\n{}",
        deployment.the_log()
    );

    // La otra cuenta no ve el repositorio de la primera.
    let answer = support::curl_download(service.port, &the_path("app.bin"), &other, None);
    assert_ne!(
        answer.status, 200,
        "the other account saw the content of the first"
    );

    support::stop(&mut service);
}
