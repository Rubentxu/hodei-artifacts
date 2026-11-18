//! La vigencia de las concesiones del canal trusted (HSP-135), contra el
//! wire.
//!
//! Promover no es conceder para siempre: la concesion nace con un horizonte
//! y al cerrarse el contenido vuelve a la cuarentena sin que nadie lo
//! revoque. Y el horizonte es una propiedad **escrita** junto a la
//! concesion: sobrevive al reinicio del servicio, y una nueva promocion lo
//! reabre con una decision nueva.
//!
//! Las pruebas hablan con un proceso de verdad y un catalogo real. El
//! horizonte se acorta por configuracion (`GRANT_MAX_VALIDITY_SECS=1`) y se
//! espera de verdad: lo que se mide es el wire, no un reloj inyectado en una
//! capa que nadie pregunta.

mod support;

use support::{Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

/// El repositorio que se queda en el canal abierto.
const OPEN_REPOSITORY: &str = "lab";

/// El digest de un contenido, sin el prefijo del algoritmo.
fn hex_of(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// Una peticion cualquiera, con el cuerpo de la respuesta en un fichero.
fn call(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<(&[u8], &str)>,
) -> (u16, String) {
    let directory = deployment.directory.path();
    let into = directory.join("response.bin");
    let _ = std::fs::remove_file(&into);
    let sent = directory.join("request.bin");
    let (file, content_type) = match body {
        Some((bytes, content_type)) => {
            std::fs::write(&sent, bytes).expect("the body is written");
            (Some(sent.as_path()), Some(content_type))
        }
        None => (None, None),
    };
    let answer = if method == "HEAD" {
        support::Curl::describe(port, path, token).run()
    } else {
        support::Curl::call(port, method, path, token, file, content_type, Some(&into)).run()
    };
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

/// Sube un fichero crudo.
fn put_raw(deployment: &Deployment, port: u16, token: &str, path: &str, bytes: &[u8]) -> u16 {
    call(
        deployment,
        port,
        "PUT",
        path,
        token,
        Some((bytes, "application/octet-stream")),
    )
    .0
}

/// Pide a promover y devuelve el codigo y el cuerpo.
fn promote(
    deployment: &Deployment,
    port: u16,
    token: &str,
    path: &str,
    digest_hex: &str,
) -> (u16, String) {
    let body = serde_json::json!({
        "repository": FIRST_REPOSITORY,
        "path": path,
        "digest": format!("sha256:{digest_hex}"),
    })
    .to_string();
    call(
        deployment,
        port,
        "POST",
        "/api/v1/promotions",
        token,
        Some((body.as_bytes(), "application/json")),
    )
}

/// Escribe una politica de confianza que todo software cumple.
fn permissive(deployment: &Deployment) -> String {
    let policy = hodei_trust_policy::TrustPolicy::new("default", "1", Vec::new());
    let path = deployment.directory.path().join("trust-policy.json");
    std::fs::write(&path, serde_json::to_vec(&policy).expect("the policy")).expect("written");
    path.to_string_lossy().into_owned()
}

/// Un despliegue con `releases` en el canal trusted y `lab` abierto.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed_trusted(&[OPEN_REPOSITORY], &[FIRST_REPOSITORY]);
    deployment
}

fn raw(path: &str) -> String {
    format!("/repository/{FIRST_REPOSITORY}/{path}")
}

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// AC2: la validez se agota, y el agotamiento se nota en el wire.
// ---------------------------------------------------------------------------

#[test]
fn a_promotion_serves_until_its_horizon_closes_and_a_new_one_reopens_it() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__GRANT_MAX_VALIDITY_SECS", "1"),
    ]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw("app/aging.bin");
    let bytes = b"release with a horizon".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &bytes),
        201
    );

    let (status, body) = promote(&deployment, service.port, &alice, "app/aging.bin", &digest);
    assert_eq!(status, 200, "{body}");
    let (status, served) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 200, "the fresh promotion does not serve");
    assert_eq!(served.as_bytes(), bytes.as_slice());

    // El horizonte se cierra: sin revocacion de por medio, el contenido
    // vuelve a ser cuarentena. Nadie dijo "no": simplemente nadie lo dice ya.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 404, "an expired grant was served");

    // Una promocion nueva reabre el horizonte con una decision nueva.
    let (status, body) = promote(&deployment, service.port, &alice, "app/aging.bin", &digest);
    assert_eq!(status, 200, "{body}");
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 200, "the re-promotion does not serve");
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC4: la vigencia es estado persistido, no memoria del proceso.
// ---------------------------------------------------------------------------

#[test]
fn a_promotion_survives_a_restart_with_its_validity() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let environment = [("HODEI_TRUST__POLICY_FILE", policy.as_str())];
    let mut service = deployment.start(&environment);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw("app/durable.bin");
    let bytes = b"promised to last".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(put_raw(&deployment, port, &alice, &path, &bytes), 201);
    let (status, body) = promote(&deployment, port, &alice, "app/durable.bin", &digest);
    assert_eq!(status, 200, "{body}");
    support::stop(&mut service);

    // Otro proceso, la misma base: la concesion sigue admitiendo porque su
    // vigencia viaja escrita con ella, no en la memoria de quien la dicto.
    let mut service = deployment.start_on(port, &environment);
    let (status, served) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 200, "the grant did not survive the restart");
    assert_eq!(served.as_bytes(), bytes.as_slice());
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC1: la vigencia no se negocia por el cuerpo de la peticion.
// ---------------------------------------------------------------------------

#[test]
fn the_client_cannot_ask_for_a_longer_leash() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__GRANT_MAX_VALIDITY_SECS", "1"),
    ]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw("app/impudent.bin");
    let bytes = b"who decides how long".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &bytes),
        201
    );

    // Un campo de mas en el cuerpo no abre la promocion ni alarga nada:
    // o se ignora y se promueve con el horizonte del servidor, o se
    // rechaza. Lo que no puede pasar es que el cliente fije la vigencia.
    let body = serde_json::json!({
        "repository": FIRST_REPOSITORY,
        "path": "app/impudent.bin",
        "digest": format!("sha256:{digest}"),
        "valid_until": "2999-01-01T00:00:00Z",
    })
    .to_string();
    let (status, _) = call(
        &deployment,
        service.port,
        "POST",
        "/api/v1/promotions",
        &alice,
        Some((body.as_bytes(), "application/json")),
    );
    assert!(
        status == 200 || status == 400,
        "the client negotiated its horizon: {status}"
    );
    if status == 200 {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
        assert_eq!(status, 404, "the client's horizon outlived the server's");
    }
    support::stop(&mut service);
}
