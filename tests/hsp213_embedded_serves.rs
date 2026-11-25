//! HSP-213: el arbol con `legacy` sigue sirviendo el perfil embebido.
//!
//! La otra mitad del retiro, con el **proceso real**: construido con la
//! feature — el default del arbol, el de la herramienta aislada
//! (`hodei-recovery` y compania, empaquetadas por
//! `scripts/package-legacy-tools.sh`) y el de las suites historicas — el
//! mismo perfil que el binario productivo rechaza sigue arrancando y
//! sirviendo `/health`. Cero regresion del camino embebido mientras dure
//! la ventana (abierta 2026-11-22): la herramienta puede leer y migrar
//! el almacen local de un despliegue que aun no ha dado el paso.
//!
//! La mitad del rechazo vive en `hsp213_profile_rejection.rs`. Este
//! target solo declara el test con `legacy`: en el arbol productivo el
//! perfil se rechaza en el arranque y no hay `/health` que sondear, asi
//! que aqui no habria nada que afirmar.

#[cfg(feature = "legacy")]
mod hsp213_profile_env;

#[cfg(feature = "legacy")]
use hsp213_profile_env::{BIN, embedded_profile_env, free_port};
#[cfg(feature = "legacy")]
use std::process::{Command, Stdio};
// El poll del `/health`: reloj y lectura de la respuesta, que solo
// necesitan esta mitad.
#[cfg(feature = "legacy")]
use std::io::Read;
#[cfg(feature = "legacy")]
use std::time::{Duration, Instant};

/// AC1 (la mitad observable del arranque externo) y AC2 en positivo: el
/// mismo perfil embebido, en el arbol con `legacy`, sigue arrancando y
/// sirviendo.
#[cfg(feature = "legacy")]
#[test]
fn the_legacy_binary_still_serves_the_embedded_profile() {
    let tmp = tempfile::tempdir().expect("a temp dir");

    let port = free_port();
    let mut command = Command::new(BIN);
    command
        .env_remove("RUN_MODE")
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    for (key, value) in embedded_profile_env(tmp.path(), port) {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("the binary runs");

    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().expect("the wait works") {
            panic!(
                "the legacy binary died booting the embedded profile ({status}); \
                 the embedded path must keep working while the window is open"
            );
        }
        if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            let request = format!(
                "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            );
            if std::io::Write::write_all(&mut stream, request.as_bytes()).is_ok() {
                let mut answer = String::new();
                let _ = stream.read_to_string(&mut answer);
                if answer.contains("200") {
                    break;
                }
            }
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the legacy binary never served /health within the deadline");
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // SIGTERM, como en el resto de contratos de proceso.
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe {
        kill(child.id() as i32, 15);
    }
    let _ = child.wait();
}
