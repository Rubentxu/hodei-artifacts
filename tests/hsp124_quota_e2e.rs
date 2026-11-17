//! Las cuotas por cuenta de HSP-124, de **caja negra**, sobre el hub real.
//!
//! Lo que se mira aquí no es el ledger (eso ya lo dice el test puro),
//! sino lo que ve un cliente de verdad:
//!
//! - Que el `413` llega **antes** de que el contenido rechazado exista:
//!   la ruta rechazada devuelve `404`, y el contenido que sí entró se
//!   descarga intacto.
//! - Que dos subidas simultáneas que no caben las dos **nunca caben las
//!   dos**: una `201` y un `413`, y la reserva de la rechazada se
//!   libera — después cabe contenido nuevo.
//! - Que la saturación responde `429` con `Retry-After`, y que cancelar
//!   la sesión que satura **libera** el puesto.
//! - Que `GET /api/v1/account/usage` dice la cuenta propia con números
//!   propios: el de la otra cuenta son ceros, y sin credencial no hay
//!   respuesta.
//! - Que el contenido que la cuenta ya tenía no se cobra dos veces: el
//!   `usage` lo dice con el mismo número antes y después.

mod support;

use std::path::{Path, PathBuf};

use support::{Answer, Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY, SECOND_ACCOUNT};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Una ruta de la superficie raw del repositorio sembrado.
fn the_raw_path(target: &str) -> String {
    format!("/repository/{FIRST_REPOSITORY}/{target}")
}

/// Un despliegue con el catálogo sembrado.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}

/// Un contenido del tamaño pedido, distinto por semilla.
fn the_content(size: usize, seed: u8) -> Vec<u8> {
    (0..size)
        .map(|index| (index as u8).wrapping_add(seed))
        .collect()
}

/// Escribe un fichero de subida y devuelve su ruta.
fn the_file_in(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("the body is written");
    path
}

/// Un `PUT` raw del fichero dado, con el ritmo que se pida.
fn publish(port: u16, target: &str, token: &str, file: &Path, limit_rate: Option<&str>) -> Answer {
    let path = the_raw_path(target);
    support::Curl::upload(port, &path, token, file, limit_rate).run()
}

/// Un `GET` raw.
fn download(port: u16, target: &str, token: &str) -> Answer {
    let path = the_raw_path(target);
    let into = Path::new(env!("CARGO_TARGET_TMPDIR")).join("downloaded.bin");
    support::Curl::download(port, &path, token, Some(&into)).run()
}

/// El `usage` de un token, como JSON.
fn usage(port: u16, token: &str) -> serde_json::Value {
    let into = Path::new(env!("CARGO_TARGET_TMPDIR")).join("usage.json");
    let answer = support::Curl::call(
        port,
        "GET",
        "/api/v1/account/usage",
        token,
        None,
        None,
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 200, "the usage answer:\n{}", answer.headers);
    serde_json::from_slice(&answer.body).expect("the usage is json")
}

// ---------------------------------------------------------------------------
// El cupo de almacenamiento: antes de guardar, y sin engañar al borrar.
// ---------------------------------------------------------------------------

/// El `413` dice no **antes** de que lo rechazado exista.
///
/// Es la diferencia entre una cuota y un despilfarro con disculpa: si
/// el contenido rechazado quedara publicado, el `no` sería cosmético.
#[test]
fn the_stored_quota_refuses_before_anything_lands() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[("HODEI_QUOTA__MAX_STORED_BYTES_PER_ACCOUNT", "1000")]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");
    let dir = deployment.directory.path();

    let first = the_file_in(dir, "first.bin", &the_content(600, 1));
    assert_eq!(
        publish(port, "one/first.bin", &token, &first, None).status,
        201,
        "the first upload must fit:\n{}",
        deployment.the_log()
    );

    let second = the_file_in(dir, "second.bin", &the_content(600, 2));
    let refused = publish(port, "two/second.bin", &token, &second, None);
    assert_eq!(
        refused.status,
        413,
        "six hundred more must not fit in a thousand:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        refused.header("retry-after"),
        None,
        "a stored quota does not come back: sending later changes nothing"
    );

    // Lo rechazado no existe. Si existiera, la cuota sería un aviso.
    assert_eq!(
        download(port, "two/second.bin", &token).status,
        404,
        "the refused content must not have landed"
    );
    assert_eq!(
        download(port, "one/first.bin", &token).status,
        200,
        "the content that fit must still be served"
    );

    // Y queda margen para lo que sí cabe: el rechazo no castiga a nada.
    let small = the_file_in(dir, "small.bin", &the_content(50, 3));
    assert_eq!(
        publish(port, "three/small.bin", &token, &small, None).status,
        201,
        "fifty bytes still fit under a thousand"
    );
    let over = the_file_in(dir, "over.bin", &the_content(500, 4));
    assert_eq!(
        publish(port, "four/over.bin", &token, &over, None).status,
        413,
        "six hundred and fifty would pass the thousand"
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// La carrera: dos subidas que no caben las dos.
// ---------------------------------------------------------------------------

/// Dos subidas simultáneas que no caben las dos **nunca** caben las dos.
///
/// El cuerpo va lento a propósito (`--limit-rate`) para que las dos
/// corran de verdad solapadas: lo que se prueba es el check-and-reserve
/// bajo una sola llave, y con cuerpos de cuatro kilobytes no habría
/// solapamiento que probar.
#[test]
fn two_concurrent_uploads_that_do_not_both_fit_never_both_land() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[("HODEI_QUOTA__MAX_STORED_BYTES_PER_ACCOUNT", "1000")]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");
    let dir = deployment.directory.path();

    let one = the_file_in(dir, "one.bin", &the_content(800, 10));
    let two = the_file_in(dir, "two.bin", &the_content(800, 20));

    let first_token = token.clone();
    let one_path = one.clone();
    let first = std::thread::spawn(move || {
        publish(port, "race/one.bin", &first_token, &one_path, Some("100"))
    });
    std::thread::sleep(std::time::Duration::from_secs(2));
    let second_token = token.clone();
    let two_path = two.clone();
    let second = std::thread::spawn(move || {
        publish(port, "race/two.bin", &second_token, &two_path, Some("100"))
    });

    let statuses = [first.join().unwrap().status, second.join().unwrap().status];
    let sorted = {
        let mut v = statuses.to_vec();
        v.sort_unstable();
        v
    };
    assert_eq!(
        sorted,
        [201, 413],
        "two eight-hundreds never fit in a thousand: exactly one may land (got {statuses:?})\n{}",
        deployment.the_log()
    );

    // La reserva de la rechazada se liberó al abortar: el margen que
    // queda es el del contenido que aterrizó, no el de las dos.
    let room = the_file_in(dir, "room.bin", &the_content(100, 30));
    assert_eq!(
        publish(port, "after/room.bin", &token, &room, None).status,
        201,
        "the refused upload must have released its reservation"
    );
    let more = the_file_in(dir, "more.bin", &the_content(200, 40));
    assert_eq!(
        publish(port, "after/more.bin", &token, &more, None).status,
        413,
        "eight hundred landed: two hundred more must not fit"
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// La saturación:Advice honesto y liberación al cancelar.
// ---------------------------------------------------------------------------

/// La tercera subida concurrente recibe `429` con `Retry-After`, y el
/// puesto se libera al **cancelar** la sesión que satura.
///
/// La sesión OCI vive entre peticiones — es el caso real de saturación
/// sostenida — y su ticket viaja dentro de la sesión: cancelarla es
/// soltar la reserva.
#[test]
fn the_saturation_advises_when_to_come_back_and_cancelling_releases() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[
        ("HODEI_QUOTA__MAX_CONCURRENT_UPLOADS_PER_ACCOUNT", "1"),
        ("HODEI_QUOTA__RETRY_AFTER_SECS", "42"),
    ]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let opened = support::Curl::call(
        port,
        "POST",
        &format!("/v2/{FIRST_REPOSITORY}/blobs/uploads/"),
        &token,
        None,
        None,
        None,
    )
    .run();
    assert_eq!(
        opened.status,
        202,
        "the session must open (it holds the only slot):\n{}",
        deployment.the_log()
    );
    let location = opened
        .header("location")
        .expect("the opened session has a location");

    let small = the_file_in(
        deployment.directory.path(),
        "small.bin",
        &the_content(10, 5),
    );
    let refused = publish(port, "busy/small.bin", &token, &small, None);
    assert_eq!(
        refused.status,
        429,
        "the slot is taken by the open session:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        refused.header("retry-after").as_deref(),
        Some("42"),
        "the refusal must say when to come back"
    );

    // Cancelar es soltar: la reserva viaja con la sesión.
    let cancelled = support::Curl::call(port, "DELETE", &location, &token, None, None, None).run();
    assert!(
        (200..300).contains(&cancelled.status),
        "cancelling the session must succeed (got {}):\n{}",
        cancelled.status,
        deployment.the_log()
    );

    let answer = publish(port, "free/small.bin", &token, &small, None);
    assert_eq!(
        answer.status,
        201,
        "the freed slot must admit the upload that was refused:\n{}",
        deployment.the_log()
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// La observabilidad: la cuenta propia, y solo la cuenta propia.
// ---------------------------------------------------------------------------

/// El `usage` dice lo que dice la cuota, para el dueño del ledger.
///
/// Los números de la cuenta ajena no se ven por ningún agujero: sin
/// parámetro de cuenta y con ceros donde la otra tiene bytes. Y el
/// contenido que ya estaba no se cobra dos veces — eso es lo que el
/// `usage` puede demostrar y un `201` no.
#[test]
fn the_usage_says_the_own_account_and_nobody_elses() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[
        ("HODEI_QUOTA__MAX_STORED_BYTES_PER_ACCOUNT", "2000"),
        ("HODEI_QUOTA__MAX_CONCURRENT_UPLOADS_PER_ACCOUNT", "3"),
    ]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let bob = support::token(SECOND_ACCOUNT, "bob");

    let unsigned =
        support::Curl::call(port, "GET", "/api/v1/account/usage", "", None, None, None).run();
    assert_eq!(unsigned.status, 401, "the usage asks for the credential");
    assert_eq!(
        unsigned.header("www-authenticate").as_deref(),
        Some("Bearer"),
        "the `401` has to name the scheme"
    );

    let content = the_content(600, 7);
    let file = the_file_in(deployment.directory.path(), "seven.bin", &content);
    assert_eq!(
        publish(port, "usage/seven.bin", &alice, &file, None).status,
        201,
        "the upload that fills the ledger must land:\n{}",
        deployment.the_log()
    );

    let mine = usage(port, &alice);
    assert_eq!(mine["account"], FIRST_ACCOUNT);
    assert_eq!(mine["stored_bytes"], 600);
    assert_eq!(mine["in_flight_bytes"], 0);
    assert_eq!(mine["active_uploads"], 0);
    assert_eq!(mine["limits"]["max_stored_bytes_per_account"], 2000);
    assert_eq!(mine["limits"]["max_concurrent_uploads_per_account"], 3);

    // La misma suma, en otra ruta: el `CAS` ya la tenía y el ledger lo
    // sabe decir. Un `usage` que subiera sería una mentira contable.
    let again = the_file_in(deployment.directory.path(), "seven-again.bin", &content);
    assert_eq!(
        publish(port, "usage/seven-again.bin", &alice, &again, None).status,
        201,
        "re-publishing owned content must not be refused"
    );
    assert_eq!(usage(port, &alice)["stored_bytes"], 600);

    // Bob no ve los números de nadie: los suyos son ceros.
    let theirs = usage(port, &bob);
    assert_eq!(theirs["account"], SECOND_ACCOUNT);
    assert_eq!(theirs["stored_bytes"], 0);
    assert_eq!(theirs["in_flight_bytes"], 0);
    assert_eq!(theirs["active_uploads"], 0);
    support::stop(&mut service);
}
