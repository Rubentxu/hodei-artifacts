//! La conformidad de la superficie de contenido crudo, de **caja negra**.
//!
//! No hay mocks aquí: hay un proceso de verdad, que es el binario del
//! servicio, arrancado en un puerto de verdad, y hay herramientas del
//! sistema hablando con él: `curl` y `sha256sum`.
//!
//! Lo que se mira no es una función de Rust, sino lo que un cliente
//! ve:
//!
//! - Que lo que se publica vuelve **intacto**: el hash del fichero que
//!   sale del `curl` es el hash del fichero que entró, y lo dice un
//!   programa que no tiene nada que ver con este servicio.
//! - Que un proceso que **se muere a mitad de una subida** no deja una
//!   ruta publicada a medias, y que al reiniciar el servicio recoge lo
//!   que quedó.
//! - Que dos subidas simultáneas **no se mezclan**: lo que se descarga
//!   tiene el hash de lo que se subió, siempre.
//! - Que dos cuentas con tokens reales **no se ven** entre sí, y que la
//!   cuenta que no tiene el repositorio no puede saber si existe.
//!
//! Y el resultado se escribe en un informe, que es lo que queda dentro de
//! seis meses.

mod support;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use support::{
    Case, Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT,
    SECOND_REPOSITORY,
};

/// El contenido de la primera persona: bytes que no son texto, para que
/// la comparación sea de bytes y no de líneas.
fn the_content() -> Vec<u8> {
    (0u16..4096).map(|value| (value % 251) as u8).collect()
}

/// Un contenido distinto para cada escritor, para que una mezcla se
/// note: cada uno tiene su propio hash.
fn the_content_of(seed: u8) -> Vec<u8> {
    (0u16..8192)
        .map(|value| ((value as u8).wrapping_add(seed)) % 251)
        .collect()
}

/// El seeder del catálogo.
///
/// El motor embebido mantiene el bloqueo del directorio de la base
/// mientras hay una conexión abierta, y el servicio es otro proceso: la
/// siembra tiene que estar en un proceso que se acaba. Este test es el
/// que se ejecuta en ese proceso, y en el resto de las veces no hace
/// nada.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Un despliegue limpio con el catálogo sembrado.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}

/// La ruta del contenido dentro del repositorio.
fn the_path(repository: &str, path: &str) -> String {
    format!("/repository/{repository}/{path}")
}

// ---------------------------------------------------------------------------
// RAW-01 y RAW-02 — publicar, descargar, describir y pedir un tramo.
// ---------------------------------------------------------------------------

/// Publicar y descargar dan el mismo contenido, y el HEAD lo describe sin
/// traerlo.
///
/// El hash lo dice `sha256sum`, no el código que se está probando.
#[test]
fn the_content_that_is_published_comes_back_intact() {
    let deployment = a_deployment();
    let port = support::free_port();
    let mut service = deployment.start(port, &[]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, the_content()).expect("the source is written");
    let expected = support::sha256_of(&source);

    let published = support::curl_upload(
        port,
        &the_path(FIRST_REPOSITORY, "dir/file.bin"),
        &token,
        &source,
        None,
    );
    assert_eq!(
        published.status,
        201,
        "the publication failed:\n{}",
        deployment.the_log()
    );

    let downloaded = deployment.directory.path().join("downloaded.bin");
    let answer = support::curl_download(
        port,
        &the_path(FIRST_REPOSITORY, "dir/file.bin"),
        &token,
        Some(&downloaded),
    );
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&downloaded),
        expected,
        "the bytes that came back are not the bytes that went in"
    );

    // Y la cabecera dice lo que el contenido mide, con el digest dentro.
    let digest = support::sha256_of(&source);
    assert_eq!(
        answer.header("etag"),
        Some(format!("\"sha256:{digest}\"")),
        "the ETag has to be the digest of the content"
    );

    // El HEAD describe sin traer: mismo estado, mismo tamaño, sin cuerpo.
    let described =
        support::curl_describe(port, &the_path(FIRST_REPOSITORY, "dir/file.bin"), &token);
    assert_eq!(described.status, 200);
    assert_eq!(
        described.header("content-length"),
        Some((the_content().len()).to_string()),
        "the HEAD has to say the size of the content"
    );

    // Y el tramo es un 206 con su Content-Range.
    let ranged = support::curl_with_header(
        port,
        "GET",
        &the_path(FIRST_REPOSITORY, "dir/file.bin"),
        &token,
        ("range", "bytes=0-9"),
    );
    assert_eq!(ranged.status, 206);
    assert_eq!(
        ranged.header("content-range"),
        Some(format!("bytes 0-9/{}", the_content().len()))
    );

    support::record(Case {
        id: "RAW-01, RAW-02".to_string(),
        given: "un repositorio hosted limpio y un token válido".to_string(),
        when: "se publica, se descarga, se describe y se pide un tramo con curl".to_string(),
        command: format!(
            "curl -X PUT http://127.0.0.1:{port}{} ; curl -o fichero http://127.0.0.1:{port}{} ; sha256sum fichero",
            the_path(FIRST_REPOSITORY, "dir/file.bin"),
            the_path(FIRST_REPOSITORY, "dir/file.bin")
        ),
        outcome: format!(
            "sha256sum idéntico ({expected}), ETag con el digest, HEAD 200 sin cuerpo, Range 206 con Content-Range"
        ),
        passed: true,
    });

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// RAW-06 — el proceso se muere a mitad de una subida.
// ---------------------------------------------------------------------------

/// Un proceso que se muere a mitad de una subida no deja una ruta
/// publicada a medias, y al reiniciar el servicio recoge lo que quedó.
///
/// El `curl` va limitado para que la subida dure lo suficiente como para
/// poder matar al servicio con el cuerpo a medias.
#[test]
fn an_upload_that_is_interrupted_leaves_nothing_published() {
    let deployment = a_deployment();
    let port = support::free_port();
    let mut service = deployment.start(port, &[]);
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Un contenido que sí se publicó, para poder comprobar que sigue
    // intacto después del corte.
    let published = deployment.directory.path().join("published.bin");
    std::fs::write(&published, the_content()).expect("the source is written");
    let expected = support::sha256_of(&published);
    let answer = support::curl_upload(
        port,
        &the_path(FIRST_REPOSITORY, "keep/file.bin"),
        &token,
        &published,
        None,
    );
    assert_eq!(answer.status, 201);

    // Y ahora una subida que no llega a terminarse.
    let big = deployment.directory.path().join("big.bin");
    std::fs::write(&big, the_content_of(7).repeat(512)).expect("the source is written");
    let mut upload = support::curl_upload_in_background(
        port,
        &the_path(FIRST_REPOSITORY, "cut/file.bin"),
        &token,
        &big,
        Some("256K"),
    );

    // Se espera a que la subida esté **en curso**: el staging existe
    // cuando el servicio está escribiendo el cuerpo.
    let deadline = Instant::now() + Duration::from_secs(30);
    while deployment.staging_entries().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !deployment.staging_entries().is_empty(),
        "the upload never reached the store:\n{}",
        deployment.the_log()
    );

    // Y ahora se mata el servicio, sin oportunidad de limpiar nada.
    support::kill(&mut service);
    let _ = upload.wait();

    // El staging del proceso muerto sigue en el disco: nadie lo limpió.
    assert!(
        !deployment.staging_entries().is_empty(),
        "the staging of the interrupted upload was expected to be there"
    );

    // Al reiniciar, el servicio limpia lo que encuentra caducado.
    let port = support::free_port();
    let mut service = deployment.start(port, &[("HODEI_CONTENT__STAGING_MAX_AGE_SECS", "0")]);

    assert!(
        deployment.staging_entries().is_empty(),
        "the staging of the interrupted upload was not cleaned up"
    );

    // La ruta de la subida cortada no existe: no se publicó a medias.
    let missing = support::curl_download(
        port,
        &the_path(FIRST_REPOSITORY, "cut/file.bin"),
        &token,
        None,
    );
    assert_eq!(
        missing.status, 404,
        "an interrupted upload must not publish a path"
    );

    // Y el contenido que sí estaba publicado sigue siendo el mismo.
    let after = deployment.directory.path().join("after.bin");
    let answer = support::curl_download(
        port,
        &the_path(FIRST_REPOSITORY, "keep/file.bin"),
        &token,
        Some(&after),
    );
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&after),
        expected,
        "the content that was published did not survive the interruption"
    );

    support::record(Case {
        id: "RAW-06".to_string(),
        given: "un contenido publicado y una subida en curso".to_string(),
        when: "el proceso se mata a mitad de la subida y se vuelve a arrancar".to_string(),
        command: "curl --limit-rate 256K -X PUT ... ; kill -9 <servicio> ; <servicio> --arranque".to_string(),
        outcome: "la ruta cortada no existe (404), el staging caducado se limpió al arrancar, y el contenido publicado sigue con el mismo sha256".to_string(),
        passed: true,
    });

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// CAS-02 y CAT-02 — subidas simultáneas.
// ---------------------------------------------------------------------------

/// Varias subidas a la vez no se mezclan.
///
/// Tres situaciones, porque cada una falla de una manera distinta:
///
/// - El mismo contenido a la misma ruta: el efecto es **uno**, y lo que
///   se descarga es ese contenido.
/// - Contenidos distintos a rutas distintas: cada ruta tiene lo suyo.
/// - Contenidos distintos a la misma ruta: la dirección publica una vez,
///   así que uno gana y los demás reciben un `409`.
///
/// Lo que no puede pasar es que lo que se descargue sea una mezcla: el
/// hash de lo que se baja tiene que ser el hash de lo que se subió.
#[test]
fn simultaneous_uploads_do_not_mix_the_content() {
    let deployment = a_deployment();
    let port = support::free_port();
    let mut service = deployment.start(port, &[]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let directory = deployment.directory.path().to_path_buf();

    // --- El mismo contenido, a la misma ruta, por cinco autores.
    let same = directory.join("same.bin");
    std::fs::write(&same, the_content()).expect("the source is written");
    let same_digest = support::sha256_of(&same);
    let mut uploads = Vec::new();
    for _ in 0..5 {
        uploads.push(support::curl_upload_in_background(
            port,
            &the_path(FIRST_REPOSITORY, "same/file.bin"),
            &token,
            &same,
            None,
        ));
    }
    let mut statuses = Vec::new();
    for upload in uploads {
        statuses.push(support::wait_for_the_answer(upload).status);
    }
    assert!(
        statuses
            .iter()
            .all(|status| *status == 200 || *status == 201),
        "the same content published at once has to be idempotent: {statuses:?}\n{}",
        deployment.the_log()
    );

    let downloaded = directory.join("same-downloaded.bin");
    let answer = support::curl_download(
        port,
        &the_path(FIRST_REPOSITORY, "same/file.bin"),
        &token,
        Some(&downloaded),
    );
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&downloaded),
        same_digest,
        "the content of a simultaneous publication is not the content that was published"
    );

    // --- Contenidos distintos, a rutas distintas.
    let mut uploads = Vec::new();
    let mut digests = Vec::new();
    for writer in 0..4u8 {
        let file = directory.join(format!("distinct-{writer}.bin"));
        std::fs::write(&file, the_content_of(writer)).expect("the source is written");
        digests.push((writer, support::sha256_of(&file)));
        uploads.push(support::curl_upload_in_background(
            port,
            &the_path(FIRST_REPOSITORY, &format!("distinct/{writer}.bin")),
            &token,
            &file,
            None,
        ));
    }
    let mut statuses = Vec::new();
    for upload in uploads {
        statuses.push(support::wait_for_the_answer(upload).status);
    }
    assert!(
        statuses
            .iter()
            .all(|status| *status == 200 || *status == 201),
        "distinct contents at distinct paths have to be published: {statuses:?}\n{}",
        deployment.the_log()
    );

    for (writer, digest) in &digests {
        let file = directory.join(format!("distinct-{writer}-downloaded.bin"));
        let answer = support::curl_download(
            port,
            &the_path(FIRST_REPOSITORY, &format!("distinct/{writer}.bin")),
            &token,
            Some(&file),
        );
        assert_eq!(answer.status, 200);
        assert_eq!(
            support::sha256_of(&file),
            *digest,
            "the content of the path {writer} is not the content that was published there"
        );
    }

    // --- Contenidos distintos, a la misma ruta.
    let mut uploads = Vec::new();
    let mut contenders = Vec::new();
    for writer in 0..4u8 {
        let file = directory.join(format!("contested-{writer}.bin"));
        std::fs::write(&file, the_content_of(100 + writer)).expect("the source is written");
        contenders.push(support::sha256_of(&file));
        uploads.push(support::curl_upload_in_background(
            port,
            &the_path(FIRST_REPOSITORY, "contested/file.bin"),
            &token,
            &file,
            None,
        ));
    }
    let mut statuses = Vec::new();
    for upload in uploads {
        statuses.push(support::wait_for_the_answer(upload).status);
    }
    let created = statuses.iter().filter(|status| **status == 201).count();
    let refused = statuses.iter().filter(|status| **status == 409).count();
    assert_eq!(
        created,
        1,
        "a path that publishes once has to have exactly one winner: {statuses:?}\n{}",
        deployment.the_log()
    );
    assert_eq!(
        refused, 3,
        "the publications that lost have to be refused: {statuses:?}"
    );

    // Y lo que queda es **uno** de los que se publicaron, entero.
    let file = directory.join("contested-downloaded.bin");
    let answer = support::curl_download(
        port,
        &the_path(FIRST_REPOSITORY, "contested/file.bin"),
        &token,
        Some(&file),
    );
    assert_eq!(answer.status, 200);
    let final_digest = support::sha256_of(&file);
    assert!(
        contenders.contains(&final_digest),
        "the content that survived is not any of the contents that were published"
    );

    // Y no queda ninguna escritura a medias.
    assert!(
        deployment.staging_entries().is_empty(),
        "a finished publication must not leave a staging behind: {:?}",
        deployment.staging_entries()
    );

    support::record(Case {
        id: "CAS-02, CAT-02".to_string(),
        given: "cinco autores con el mismo contenido, cuatro con contenidos distintos y cuatro disputando la misma ruta".to_string(),
        when: "se publican todos a la vez y se descarga lo que hay".to_string(),
        command: "curl -X PUT ... & (cinco, cuatro y cuatro en paralelo) ; curl -o fichero ... ; sha256sum fichero".to_string(),
        outcome: "el mismo contenido es idempotente (5 respuestas 200/201), las rutas distintas guardan lo suyo, la ruta disputada tiene un solo ganador (201) y tres 409, y cada hash descargado es el hash de lo subido".to_string(),
        passed: true,
    });

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// RAW-05 y CAT-03 — dos cuentas con tokens reales.
// ---------------------------------------------------------------------------

/// Dos cuentas no se ven, y la que no tiene el repositorio no puede
/// saber si existe.
///
/// Las dos cuentas tienen un repositorio con la **misma clave**, así que
/// la misma dirección con tokens distintos son dos contenidos distintos:
/// es el aislamiento por cuenta, medido con las dos URLs iguales.
#[test]
fn two_accounts_do_not_see_each_other() {
    let deployment = a_deployment();
    let port = support::free_port();
    let mut service = deployment.start(port, &[]);
    let first = support::token(FIRST_ACCOUNT, "alice");
    let second = support::token(SECOND_ACCOUNT, "bob");
    let directory = deployment.directory.path().to_path_buf();

    // Las dos cuentas publican en la misma dirección.
    let first_file = directory.join("first.bin");
    std::fs::write(&first_file, the_content_of(1)).expect("the source is written");
    let first_digest = support::sha256_of(&first_file);
    let answer = support::curl_upload(
        port,
        &the_path(FIRST_REPOSITORY, "shared/file.bin"),
        &first,
        &first_file,
        None,
    );
    assert_eq!(answer.status, 201, "{}", deployment.the_log());

    let second_file = directory.join("second.bin");
    std::fs::write(&second_file, the_content_of(2)).expect("the source is written");
    let second_digest = support::sha256_of(&second_file);
    let answer = support::curl_upload(
        port,
        &the_path(SECOND_REPOSITORY, "shared/file.bin"),
        &second,
        &second_file,
        None,
    );
    assert_eq!(answer.status, 201, "{}", deployment.the_log());

    // Y cada una descarga lo suyo de la misma dirección.
    let downloaded = directory.join("by-first.bin");
    let answer = support::curl_download(
        port,
        &the_path(FIRST_REPOSITORY, "shared/file.bin"),
        &first,
        Some(&downloaded),
    );
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&downloaded),
        first_digest,
        "the first account did not get its own content"
    );

    let downloaded = directory.join("by-second.bin");
    let answer = support::curl_download(
        port,
        &the_path(SECOND_REPOSITORY, "shared/file.bin"),
        &second,
        Some(&downloaded),
    );
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&downloaded),
        second_digest,
        "the second account did not get its own content"
    );

    // Y lo que hay en la dirección de la otra cuenta no se puede leer:
    // con el token de la segunda, la dirección de la primera no existe.
    let answer = support::curl_download(
        port,
        &the_path(FIRST_ONLY_REPOSITORY, "secret/file.bin"),
        &second,
        None,
    );
    assert_eq!(
        answer.status, 404,
        "a repository of another account must not exist for this one"
    );

    // Y la denegación es **coherente**: la misma cuenta, el mismo
    // repositorio y otra persona: el bundle de Cedar no la cubre, así que
    // la respuesta es un 403 —que sí dice que el repositorio existe— en
    // vez de un 404 que mentiría.
    let carol = support::token(FIRST_ACCOUNT, "carol");
    let answer = support::curl_download(
        port,
        &the_path(FIRST_REPOSITORY, "shared/file.bin"),
        &carol,
        None,
    );
    assert_eq!(
        answer.status,
        403,
        "a principal that the bundle does not cover has to be refused:\n{}",
        deployment.the_log()
    );

    // Y sin credencial tampoco se dice nada.
    let answer = support::curl_download(
        port,
        &the_path(FIRST_ONLY_REPOSITORY, "secret/file.bin"),
        "",
        None,
    );
    assert_eq!(answer.status, 401);

    support::record(Case {
        id: "RAW-05, CAT-03".to_string(),
        given: "dos cuentas con tokens reales y repositorios con la misma clave".to_string(),
        when: "las dos publican en la misma dirección y cada una descarga, y la otra busca un repositorio que no tiene".to_string(),
        command: format!(
            "curl -X PUT -H 'Authorization: Bearer <alice>' http://127.0.0.1:{port}{} ; curl -X PUT -H 'Authorization: Bearer <bob>' http://127.0.0.1:{port}{}",
            the_path(FIRST_REPOSITORY, "shared/file.bin"),
            the_path(SECOND_REPOSITORY, "shared/file.bin")
        ),
        outcome: "cada cuenta descarga su propio contenido (hash distinto en la misma dirección), el repositorio de la otra cuenta es 404, el principal que el bundle no cubre es 403 y sin credencial es 401".to_string(),
        passed: true,
    });

    support::stop(&mut service);
}

/// El informe se escribe donde la CI lo puede leer.
#[test]
fn the_report_is_written_where_the_ci_can_read_it() {
    let path: PathBuf = support::Report::the_path();
    assert!(
        path.ends_with("raw-conformity.md"),
        "the report has a known name: {path:?}"
    );
    let parent = path.parent().expect("the report has a directory");
    std::fs::create_dir_all(parent).expect("the directory of the report");
    // Y el informe dice lo que no certifica, porque un informe que solo
    // dice lo que sí es una propaganda.
    let report = support::Report::new();
    let markdown = report.as_markdown();
    assert!(markdown.contains("no** certifica"));
    assert!(markdown.contains("OCI"));
    assert!(markdown.contains("S3"));
}
