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
//!   que quedó y respeta lo que está vivo.
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

/// Un contenido grande, para que las subidas se solapen de verdad.
///
/// Un cuerpo de cuatro kilobytes se sube antes de que el siguiente
/// `curl` conecte, y entonces lo que se mide no es la concurrencia sino
/// la idempotencia de republicar.
fn a_big_content(seed: u8) -> Vec<u8> {
    the_content_of(seed).repeat(512)
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

/// El caso del informe, antes de ejecutarlo.
///
/// Se anota dos veces: al principio, con lo que se espera, y al final,
/// con lo que se ha medido. Si la prueba se muere entre medias, el
/// informe se queda con la primera anotación, que dice `FALLO`. Un
/// informe que solo sabe decir `PASS` no es un informe.
fn the_case(id: &str, given: &str, when: &str, command: &str) -> Case {
    Case {
        id: id.to_string(),
        given: given.to_string(),
        when: when.to_string(),
        command: command.to_string(),
        outcome: "en curso".to_string(),
        passed: false,
    }
}

/// El `curl` que hace el arnés, con sus argumentos de verdad.
///
/// La columna del informe tiene el comando **real**: lo que se ejecutó,
/// con sus banderas y su credencial. Un comando resumido es una
/// paráfrasis, y una paráfrasis es una opinión.
fn the_command(port: u16, path: &str, token: &str, extra: &str) -> String {
    format!(
        "curl {extra} --silent --show-error --path-as-is --dump-header - --write-out '%{{http_code}} %{{size_download}}' --header 'Authorization: Bearer {token}' http://127.0.0.1:{port}{path}"
    )
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
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");
    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, the_content()).expect("the source is written");
    let expected = support::sha256_of(&source);
    let path = the_path(FIRST_REPOSITORY, "dir/file.bin");

    let mut case = the_case(
        "RAW-01, RAW-02",
        "un repositorio hosted limpio y un token válido",
        "se publica, se descarga, se describe y se pide un tramo con curl",
        &format!(
            "{} ; {} ; sha256sum descargado",
            the_command(
                port,
                &path,
                "<alice>",
                "--request PUT --data-binary @fuente"
            ),
            the_command(port, &path, "<alice>", "--output descargado")
        ),
    );
    support::record(case.clone());

    let published = support::curl_upload(port, &path, &token, &source, None);
    assert_eq!(
        published.status,
        201,
        "the publication failed:\n{}",
        deployment.the_log()
    );

    let downloaded = deployment.directory.path().join("downloaded.bin");
    let answer = support::curl_download(port, &path, &token, Some(&downloaded));
    assert_eq!(answer.status, 200);
    let measured = support::sha256_of(&downloaded);
    assert_eq!(
        measured, expected,
        "the bytes that came back are not the bytes that went in"
    );

    // Y la cabecera dice lo que el contenido mide, con el digest dentro.
    let etag = answer.header("etag");
    assert_eq!(
        etag,
        Some(format!("\"sha256:{expected}\"")),
        "the ETag has to be the digest of the content"
    );

    // El HEAD describe sin traer: mismo estado, mismo tamaño, y **cero**
    // bytes de cuerpo. El cero lo cuenta el propio `curl`.
    let described = support::curl_describe(port, &path, &token);
    assert_eq!(described.status, 200);
    assert_eq!(
        described.header("content-length"),
        Some((the_content().len()).to_string()),
        "the HEAD has to say the size of the content"
    );
    assert_eq!(
        described.size, 0,
        "the HEAD has to come without a body, and the curl counted {} bytes",
        described.size
    );

    // Y el tramo es un 206 con su Content-Range, y con **sus bytes**: un
    // 206 con el Content-Range correcto y el contenido de otro asset
    // sería un tramo que miente.
    let ranged_file = deployment.directory.path().join("ranged.bin");
    let ranged =
        support::curl_get_with_header(port, &path, &token, ("range", "bytes=0-9"), &ranged_file);
    assert_eq!(ranged.status, 206);
    assert_eq!(
        ranged.header("content-range"),
        Some(format!("bytes 0-9/{}", the_content().len()))
    );
    let ranged_bytes = std::fs::read(&ranged_file).expect("the range is written");
    assert_eq!(
        ranged_bytes,
        &the_content()[0..10],
        "the bytes of the range are not the bytes of the content"
    );
    assert_eq!(ranged.size, 10, "the range is ten bytes long");

    case.outcome = format!(
        "sha256sum idéntico ({measured}), ETag {}, HEAD {} con {} bytes de cuerpo, Range {} con {} y sus 10 bytes",
        etag.clone().unwrap_or_default(),
        described.status,
        described.size,
        ranged.status,
        ranged.header("content-range").unwrap_or_default()
    );
    case.passed = true;
    support::record(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// RAW-06 — el proceso se muere a mitad de una subida.
// ---------------------------------------------------------------------------

/// Un proceso que se muere a mitad de una subida no deja una ruta
/// publicada a medias, y al reiniciar el servicio recoge lo que quedó.
///
/// El `curl` va limitado para que la subida dure lo suficiente como para
/// poder matar al servicio con el cuerpo a medias, y el arnés espera a
/// que el staging **tenga bytes** antes de matar: un staging vacío
/// significa que la subida no había empezado a llegar, y matar ahí no es
/// matar una subida a la mitad.
#[test]
fn an_upload_that_is_interrupted_leaves_nothing_published() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let mut case = the_case(
        "RAW-06",
        "un contenido publicado y una subida en curso",
        "el proceso se mata a mitad de la subida y se vuelve a arrancar",
        &format!(
            "{} ; kill -9 <servicio> ; <servicio> (arranque normal, con la caducidad por defecto)",
            the_command(
                port,
                &the_path(FIRST_REPOSITORY, "cut/file.bin"),
                "<alice>",
                "--limit-rate 256K --request PUT --data-binary @grande"
            )
        ),
    );
    support::record(case.clone());

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
    std::fs::write(&big, the_content_of(7).repeat(128)).expect("the source is written");
    let mut upload = support::curl_upload_in_background(
        port,
        &the_path(FIRST_REPOSITORY, "cut/file.bin"),
        &token,
        &big,
        Some("256K"),
    );

    // Se espera a que la subida esté **en curso**: el staging existe y
    // tiene bytes, o sea que el servicio está escribiendo el cuerpo.
    let deadline = Instant::now() + Duration::from_secs(30);
    while deployment.staging_bytes() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        deployment.staging_bytes() > 0,
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

    // Ese staging es un fichero abandonado: lleva dos horas ahí, que es
    // más que la caducidad por defecto (una hora). Y al lado se deja un
    // staging **vivo**, como el de una subida de otro proceso, que es lo
    // que el arranque no tiene que tocar.
    deployment.age_the_staging(Duration::from_secs(2 * 3600));
    let live = deployment.a_live_staging();

    // Al reiniciar, el servicio limpia lo que encuentra caducado y
    // respeta lo que está vivo. Y se reinicia con la configuración de
    // verdad: la caducidad por defecto, sin tocar nada.
    let mut service = deployment.start(&[]);
    let port = service.port;

    assert!(
        deployment.staging_entries().contains(&live),
        "the staging of a live upload was removed at the boot"
    );
    assert!(
        live.exists(),
        "the staging of a live upload was removed at the boot"
    );
    assert_eq!(
        deployment.staging_entries().len(),
        1,
        "the expired staging was not cleaned up: {:?}",
        deployment.staging_entries()
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

    // Y publicarla otra vez **funciona**: un `404` y un `409` juntos
    // serían un asset colgado, un camino que nadie puede usar. Con el
    // `409` solo, lo que se comprueba es que la dirección está libre.
    let again = support::curl_upload(
        port,
        &the_path(FIRST_REPOSITORY, "cut/file.bin"),
        &token,
        &published,
        None,
    );
    assert_eq!(
        again.status,
        201,
        "the interrupted path is not free:\n{}",
        deployment.the_log()
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

    case.outcome = format!(
        "la ruta cortada no existe ({}), republicar en ella da {} (no 409), el staging caducado se limpió al arrancar y el vivo sigue, y el contenido publicado sigue con el mismo sha256 ({expected})",
        missing.status, again.status
    );
    case.passed = true;
    support::record(case);

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
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");
    let directory = deployment.directory.path().to_path_buf();

    let mut case = the_case(
        "CAS-02, CAT-02",
        "cinco peticiones con el mismo contenido, cuatro con contenidos distintos y cuatro disputando la misma ruta",
        "se publican todos a la vez y se descarga lo que hay",
        &format!(
            "{} & (cinco, cuatro y cuatro en paralelo) ; {} ; sha256sum descargado",
            the_command(
                port,
                &the_path(FIRST_REPOSITORY, "same/file.bin"),
                "<alice>",
                "--limit-rate 1M --request PUT --data-binary @contenido"
            ),
            the_command(
                port,
                &the_path(FIRST_REPOSITORY, "same/file.bin"),
                "<alice>",
                "--output descargado"
            )
        ),
    );
    support::record(case.clone());

    // --- El mismo contenido, a la misma ruta, por cinco peticiones.
    let same = directory.join("same.bin");
    std::fs::write(&same, a_big_content(1)).expect("the source is written");
    let same_digest = support::sha256_of(&same);
    let mut uploads = Vec::new();
    for _ in 0..5 {
        uploads.push(support::curl_upload_in_background(
            port,
            &the_path(FIRST_REPOSITORY, "same/file.bin"),
            &token,
            &same,
            Some("1M"),
        ));
    }

    // Y las subidas **se solapan**: hay más de un staging en curso, que
    // es lo que dice que las peticiones estaban en vuelo a la vez y que
    // cada una tenía su propio staging. Sin esta comprobación, lo que
    // se mide es una republicar en fila.
    let deadline = Instant::now() + Duration::from_secs(30);
    while deployment.staging_entries().len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        deployment.staging_entries().len() >= 2,
        "the uploads did not overlap: {:?}",
        deployment.staging_entries()
    );

    let mut same_statuses = Vec::new();
    for upload in uploads {
        same_statuses.push(support::wait_for_the_answer(upload).status);
    }
    assert!(
        same_statuses
            .iter()
            .all(|status| *status == 200 || *status == 201),
        "the same content published at once has to be idempotent: {same_statuses:?}\n{}",
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
        std::fs::write(&file, a_big_content(writer)).expect("the source is written");
        digests.push((writer, support::sha256_of(&file)));
        uploads.push(support::curl_upload_in_background(
            port,
            &the_path(FIRST_REPOSITORY, &format!("distinct/{writer}.bin")),
            &token,
            &file,
            Some("1M"),
        ));
    }
    let mut distinct_statuses = Vec::new();
    for upload in uploads {
        distinct_statuses.push(support::wait_for_the_answer(upload).status);
    }
    assert!(
        distinct_statuses
            .iter()
            .all(|status| *status == 200 || *status == 201),
        "distinct contents at distinct paths have to be published: {distinct_statuses:?}\n{}",
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
        std::fs::write(&file, a_big_content(100 + writer)).expect("the source is written");
        contenders.push(support::sha256_of(&file));
        uploads.push(support::curl_upload_in_background(
            port,
            &the_path(FIRST_REPOSITORY, "contested/file.bin"),
            &token,
            &file,
            Some("1M"),
        ));
    }
    let mut contested_statuses = Vec::new();
    for upload in uploads {
        contested_statuses.push(support::wait_for_the_answer(upload).status);
    }
    let created = contested_statuses
        .iter()
        .filter(|status| **status == 201)
        .count();
    let refused = contested_statuses
        .iter()
        .filter(|status| **status == 409)
        .count();
    assert_eq!(
        created,
        1,
        "a path that publishes once has to have exactly one winner: {contested_statuses:?}\n{}",
        deployment.the_log()
    );
    assert_eq!(
        refused, 3,
        "the publications that lost have to be refused: {contested_statuses:?}"
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

    case.outcome = format!(
        "el mismo contenido es idempotente ({same_statuses:?}), las rutas distintas guardan lo suyo ({distinct_statuses:?}), la ruta disputada tiene {created} ganador y {refused} 409 ({contested_statuses:?}), y cada hash descargado es el hash de lo subido"
    );
    case.passed = true;
    support::record(case);

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
    let mut service = deployment.start(&[]);
    let port = service.port;
    let first = support::token(FIRST_ACCOUNT, "alice");
    let second = support::token(SECOND_ACCOUNT, "bob");
    let directory = deployment.directory.path().to_path_buf();

    let mut case = the_case(
        "RAW-05, CAT-03",
        "dos cuentas con tokens reales y repositorios con la misma clave",
        "las dos publican en la misma dirección y cada una descarga, y la otra busca un repositorio que no tiene",
        &format!(
            "{} ; {}",
            the_command(
                port,
                &the_path(FIRST_REPOSITORY, "shared/file.bin"),
                "<alice>",
                "--request PUT --data-binary @contenido"
            ),
            the_command(
                port,
                &the_path(SECOND_REPOSITORY, "shared/file.bin"),
                "<bob>",
                "--request PUT --data-binary @otro"
            )
        ),
    );
    support::record(case.clone());

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

    // Y la primera cuenta publica también en un repositorio que la
    // segunda no tiene. Sin esto, el `404` de la segunda no distinguiría
    // "no lo ves" de "no hay nada publicado": haría falta que lo hubiera
    // publicado alguien.
    let only_first = directory.join("only-first.bin");
    std::fs::write(&only_first, the_content_of(3)).expect("the source is written");
    let answer = support::curl_upload(
        port,
        &the_path(FIRST_ONLY_REPOSITORY, "secret/file.bin"),
        &first,
        &only_first,
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

    case.outcome = format!(
        "cada cuenta descarga su propio contenido (hash distinto en la misma dirección), el repositorio de la otra cuenta es {}, el principal que el bundle no cubre es {} y sin credencial es {}",
        404, 403, 401
    );
    case.passed = true;
    support::record(case);

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
