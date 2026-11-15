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

/// Los comandos de un caso, tal y como se ejecutaron.
///
/// La columna del informe lleva el comando **real**: lo que se ejecutó,
/// con sus banderas. Un comando resumido es una paráfrasis, y una
/// paráfrasis es una opinión.
///
/// La credencial va **redactada**: el informe dice `<alice>` donde el
/// `curl` llevaba el token. Un informe se archiva y se comparte, y un
/// token en un fichero es una credencial en un fichero. Lo que se
/// escribe es quién actuó, no con qué se autenticó.
fn the_commands(curls: &[&support::Curl]) -> String {
    curls
        .iter()
        .map(|curl| curl.shown())
        .collect::<Vec<_>>()
        .join(" ; ")
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

    // Los `curl` se construyen primero: el informe y la ejecución salen
    // de la misma construcción, así que el informe no puede mentir.
    let download_path = deployment.directory.path().join("downloaded.bin");
    let range_path = deployment.directory.path().join("ranged.bin");
    let upload = support::Curl::upload(port, &path, &token, &source, None);
    let download = support::Curl::download(port, &path, &token, Some(&download_path));
    let describe = support::Curl::describe(port, &path, &token);
    let range =
        support::Curl::with_header(port, &path, &token, ("range", "bytes=0-9"), &range_path);

    let mut case = the_case(
        "RAW-01, RAW-02",
        "un repositorio hosted limpio y un token válido",
        "se publica, se descarga, se describe y se pide un tramo con curl",
        &format!(
            "{} ; {}",
            the_commands(&[&upload, &download, &describe, &range]),
            support::sha256_command(&download_path)
        ),
    );
    support::record(case.clone());

    // El comando del informe tiene que ser el que se ejecuta, con sus
    // banderas: un `HEAD` que no esta en el comando no se puede repetir.
    assert!(
        case.command.contains("--head"),
        "the command of the report does not have the HEAD: {}",
        case.command
    );
    assert!(
        case.command.contains("range: bytes=0-9"),
        "the command of the report does not have the range: {}",
        case.command
    );
    assert!(
        !case.command.contains(token.as_str()),
        "the real token must never reach the report: {}",
        case.command
    );

    let published = upload.run();
    assert_eq!(
        published.status,
        201,
        "the publication failed:\n{}",
        deployment.the_log()
    );

    let answer = download.run();
    assert_eq!(answer.status, 200);
    let measured = support::sha256_of(&download_path);
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
    let described = describe.run();
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
    let ranged = range.run();
    assert_eq!(ranged.status, 206);
    assert_eq!(
        ranged.header("content-range"),
        Some(format!("bytes 0-9/{}", the_content().len()))
    );
    let ranged_bytes = std::fs::read(&range_path).expect("the range is written");
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

    let cut_path = the_path(FIRST_REPOSITORY, "cut/file.bin");
    let keep_path = the_path(FIRST_REPOSITORY, "keep/file.bin");
    let published_path = deployment.directory.path().join("published.bin");
    let big_path = deployment.directory.path().join("big.bin");
    let after_path = deployment.directory.path().join("after.bin");

    // El corte se ve en el comando: el `curl` limitado que se queda en
    // curso, el `kill` y el arranque de verdad.
    let cut_upload = support::Curl::upload(port, &cut_path, &token, &big_path, Some("256K"));
    let keep_upload = support::Curl::upload(port, &keep_path, &token, &published_path, None);

    let mut case = the_case(
        "RAW-06",
        "un contenido publicado y una subida en curso",
        "el proceso se muere a mitad de la subida y se vuelve a arrancar",
        &format!(
            "{} ; {} ; kill -9 <servicio> ; <servicio> (arranque normal, con la caducidad por defecto)",
            keep_upload.shown(),
            cut_upload.shown()
        ),
    );
    support::record(case.clone());

    // Un contenido que sí se publicó, para poder comprobar que sigue
    // intacto después del corte.
    std::fs::write(&published_path, the_content()).expect("the source is written");
    let expected = support::sha256_of(&published_path);
    let answer = keep_upload.run();
    assert_eq!(answer.status, 201);

    // Y ahora una subida que no llega a terminarse.
    std::fs::write(&big_path, the_content_of(7).repeat(128)).expect("the source is written");
    let mut upload = cut_upload.run_in_background();

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
    //
    // El vivo también lleva dos horas: si lo protegiera solo su fecha,
    // el arranque se lo llevaría. Lo que lo protege es el **bloqueo**,
    // que es lo que dice que alguien lo está escribiendo.
    let live = deployment.a_live_staging();
    deployment.age_the_staging(Duration::from_secs(2 * 3600));

    // Al reiniciar, el servicio limpia lo que encuentra caducado y
    // respeta lo que está vivo. Y se reinicia con la configuración de
    // verdad: la caducidad por defecto, sin tocar nada.
    let mut service = deployment.start(&[]);
    let port = service.port;

    // El reinicio abre otro puerto, así que los `curl` de despues se
    // construyen aqui: el comando del informe es el que se ejecutó, con
    // el puerto que se ejecutó.
    let missing_command = support::Curl::download(port, &cut_path, &token, None);
    let again_command = support::Curl::upload(port, &cut_path, &token, &published_path, None);
    let after_command = support::Curl::download(port, &keep_path, &token, Some(&after_path));

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
    let missing = missing_command.run();
    assert_eq!(
        missing.status, 404,
        "an interrupted upload must not publish a path"
    );

    // Y publicarla otra vez **funciona**: un `404` y un `409` juntos
    // serían un asset colgado, un camino que nadie puede usar. Con el
    // `409` solo, lo que se comprueba es que la dirección está libre.
    let again = again_command.run();
    assert_eq!(
        again.status,
        201,
        "the interrupted path is not free:\n{}",
        deployment.the_log()
    );

    // Y el contenido que sí estaba publicado sigue siendo el mismo.
    let answer = after_command.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&after_path),
        expected,
        "the content that was published did not survive the interruption"
    );

    // El arranque dice cuantos stagings se llevo y cuantos dejo, y el
    // informe lo dice: es el recuento del servicio, no el del arnes.
    let (removed, alive) = support::the_boot_cleanup(&deployment.the_log());
    assert_eq!(
        removed,
        Some(1),
        "the boot did not say that it removed the expired staging:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        alive,
        Some(1),
        "the boot did not say that it left the live staging alone:\n{}",
        deployment.the_log()
    );

    case.command = format!(
        "{} ; {} ; kill -9 <servicio> ; <servicio> (arranque normal, con la caducidad por defecto) ; {} ; {} ; {}",
        keep_upload.shown(),
        cut_upload.shown(),
        missing_command.shown(),
        again_command.shown(),
        after_command.shown()
    );
    case.outcome = format!(
        "la ruta cortada no existe ({}), republicar en ella da {} (no 409), el staging caducado se limpió al arrancar y el vivo sigue —caducado y bloqueado, que es lo que lo salva—, y el contenido publicado sigue con el mismo sha256 ({expected})",
        missing.status, again.status
    );
    // Los recuentos se escriben como los dice el log del servicio, con
    // los nombres de sus campos: asi el informe y el log se pueden
    // comparar sin traducir.
    case.outcome.push_str(&format!(
        " (removed: {removed}, alive: {alive})",
        removed = removed.unwrap_or(0),
        alive = alive.unwrap_or(0)
    ));
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

    let same_path = the_path(FIRST_REPOSITORY, "same/file.bin");
    let same_download_path = directory.join("same-downloaded.bin");

    // Los `curl` de este caso se construyen antes de ejecutarlos, y el
    // informe los lleva tal cual: los cinco que compiten por la misma
    // ruta son el mismo comando con la misma bandera de limite.
    let same_upload = support::Curl::upload(
        port,
        &same_path,
        &token,
        &directory.join("same.bin"),
        Some("1M"),
    );
    let same_download =
        support::Curl::download(port, &same_path, &token, Some(&same_download_path));

    // Los tres grupos de subidas y las tres descargas: cada grupo es un
    // comando, y el numero de veces que se repite lo dice la prosa.
    let distinct_upload = support::Curl::upload(
        port,
        &the_path(FIRST_REPOSITORY, "distinct/0.bin"),
        &token,
        &directory.join("distinct-0.bin"),
        Some("1M"),
    );
    let distinct_download = support::Curl::download(
        port,
        &the_path(FIRST_REPOSITORY, "distinct/0.bin"),
        &token,
        Some(&directory.join("distinct-0-downloaded.bin")),
    );
    let contested_upload = support::Curl::upload(
        port,
        &the_path(FIRST_REPOSITORY, "contested/file.bin"),
        &token,
        &directory.join("contested-0.bin"),
        Some("1M"),
    );
    let contested_download = support::Curl::download(
        port,
        &the_path(FIRST_REPOSITORY, "contested/file.bin"),
        &token,
        Some(&directory.join("contested-downloaded.bin")),
    );

    let mut case = the_case(
        "CAS-02, CAT-02",
        "cinco peticiones con el mismo contenido, cuatro con contenidos distintos y cuatro disputando la misma ruta",
        "se publican todos a la vez y se descarga lo que hay",
        &format!(
            "{} (cinco en paralelo) ; {} (cuatro en paralelo) ; {} (cuatro en paralelo) ; {} ; {} ; {} ; {}",
            same_upload.shown(),
            distinct_upload.shown(),
            contested_upload.shown(),
            same_download.shown(),
            distinct_download.shown(),
            contested_download.shown(),
            support::sha256_command(&same_download_path)
        ),
    );
    support::record(case.clone());

    // --- El mismo contenido, a la misma ruta, por cinco peticiones.
    let same = directory.join("same.bin");
    std::fs::write(&same, a_big_content(1)).expect("the source is written");
    let same_digest = support::sha256_of(&same);
    let mut uploads = Vec::new();
    for _ in 0..5 {
        uploads.push(same_upload.run_in_background());
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

    let answer = same_download.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&same_download_path),
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
        uploads.push(
            support::Curl::upload(
                port,
                &the_path(FIRST_REPOSITORY, &format!("distinct/{writer}.bin")),
                &token,
                &file,
                Some("1M"),
            )
            .run_in_background(),
        );
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
        uploads.push(
            support::Curl::upload(
                port,
                &the_path(FIRST_REPOSITORY, "contested/file.bin"),
                &token,
                &file,
                Some("1M"),
            )
            .run_in_background(),
        );
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

    let shared_first = the_path(FIRST_REPOSITORY, "shared/file.bin");
    let shared_second = the_path(SECOND_REPOSITORY, "shared/file.bin");
    let only_first = the_path(FIRST_ONLY_REPOSITORY, "secret/file.bin");
    let carol = support::token(FIRST_ACCOUNT, "carol");

    // Los `curl` de este caso se construyen antes: el informe lleva las
    // dos cuentas y la peticion sin credencial, cada una con su
    // credencial redactada.
    let first_upload = support::Curl::upload(
        port,
        &shared_first,
        &first,
        &directory.join("first.bin"),
        None,
    );
    let second_upload = support::Curl::upload(
        port,
        &shared_second,
        &second,
        &directory.join("second.bin"),
        None,
    );
    let only_first_upload = support::Curl::upload(
        port,
        &only_first,
        &first,
        &directory.join("only-first.bin"),
        None,
    );
    let first_download = support::Curl::download(
        port,
        &shared_first,
        &first,
        Some(&directory.join("by-first.bin")),
    );
    let second_download = support::Curl::download(
        port,
        &shared_second,
        &second,
        Some(&directory.join("by-second.bin")),
    );
    let other_account = support::Curl::download(port, &only_first, &second, None);
    let uncovered = support::Curl::download(port, &shared_first, &carol, None);
    let anonymous = support::Curl::download(port, &only_first, "", None);

    let mut case = the_case(
        "RAW-05, CAT-03",
        "dos cuentas con tokens reales y repositorios con la misma clave",
        "las dos publican en la misma dirección y cada una descarga, y la otra busca un repositorio que no tiene",
        &the_commands(&[
            &first_upload,
            &second_upload,
            &only_first_upload,
            &first_download,
            &second_download,
            &other_account,
            &uncovered,
            &anonymous,
        ]),
    );
    support::record(case.clone());

    // Las dos cuentas publican en la misma dirección.
    let first_file = directory.join("first.bin");
    std::fs::write(&first_file, the_content_of(1)).expect("the source is written");
    let first_digest = support::sha256_of(&first_file);
    let answer = first_upload.run();
    assert_eq!(answer.status, 201, "{}", deployment.the_log());

    let second_file = directory.join("second.bin");
    std::fs::write(&second_file, the_content_of(2)).expect("the source is written");
    let second_digest = support::sha256_of(&second_file);
    let answer = second_upload.run();
    assert_eq!(answer.status, 201, "{}", deployment.the_log());

    // Y la primera cuenta publica también en un repositorio que la
    // segunda no tiene. Sin esto, el `404` de la segunda no distinguiría
    // "no lo ves" de "no hay nada publicado": haría falta que lo hubiera
    // publicado alguien.
    let only_first_file = directory.join("only-first.bin");
    std::fs::write(&only_first_file, the_content_of(3)).expect("the source is written");
    let answer = only_first_upload.run();
    assert_eq!(answer.status, 201, "{}", deployment.the_log());

    // Y cada una descarga lo suyo de la misma dirección.
    let answer = first_download.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&directory.join("by-first.bin")),
        first_digest,
        "the first account did not get its own content"
    );

    let answer = second_download.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&directory.join("by-second.bin")),
        second_digest,
        "the second account did not get its own content"
    );

    // Y lo que hay en la dirección de la otra cuenta no se puede leer:
    // con el token de la segunda, la dirección de la primera no existe.
    let answer = other_account.run();
    assert_eq!(
        answer.status, 404,
        "a repository of another account must not exist for this one"
    );

    // Y la denegación es **coherente**: la misma cuenta, el mismo
    // repositorio y otra persona: el bundle de Cedar no la cubre, así que
    // la respuesta es un 403 —que sí dice que el repositorio existe— en
    // vez de un 404 que mentiría.
    let answer = uncovered.run();
    assert_eq!(
        answer.status,
        403,
        "a principal that the bundle does not cover has to be refused:\n{}",
        deployment.the_log()
    );

    // Y sin credencial tampoco se dice nada.
    let answer = anonymous.run();
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
    // Y dice que la credencial va redactada, para que nadie lea la
    // columna como si fueran comandos copiables tal cual.
    assert!(
        markdown.contains("redactad"),
        "the report does not say that the credentials are redacted:\n{markdown}"
    );
}
