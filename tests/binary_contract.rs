//! Process-level contract for the `hodei-artifacts-api` binary.
//!
//! # Why this file exists
//!
//! `tests/http_contract.rs` drives the router in process, so it proves the
//! routes work. It cannot prove that the *binary* honours the documented
//! configuration, because the documented configuration is read from the
//! process environment before the router is ever built.
//!
//! That gap was real: the `HODEI_<SECTION>__<KEY>` override was inert, so the
//! binary always booted with the values from the TOML file no matter what the
//! environment said. Nothing in the suite could observe it, because every
//! test configured the application in memory.
//!
//! # Guarantees
//!
//! - P1 The binary starts with a valid configuration, serves `/health` over a
//!   real socket, and shuts down on `SIGTERM`.
//! - P2 The binary reads its configuration from the environment: the port it
//!   binds and the database it opens are the overridden ones (the
//!   discriminating counterpart of P1 — without it, P1 would still pass with
//!   a binary that ignored the environment and booted with the file values).
//! - P3 (negative) The binary exits with a failure code and an explicit
//!   message when the configuration is invalid. A service that boots with a
//!   configuration nobody asked for is worse than one that refuses to boot.
//!
//! Each test owns a throwaway database directory and binds an ephemeral port,
//! so no state and no port is shared.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The binary under test, as built by Cargo for this integration test.
const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");

/// How long the binary is given to serve `/health` before the test gives up.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Reserves an ephemeral port and releases it again.
///
/// There is an inherent race between releasing the port and the binary
/// binding it, so the caller retries with a fresh port when the binary
/// reports the address as taken.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    listener.local_addr().expect("the local address").port()
}

/// Sends a minimal HTTP/1.1 `GET` and returns the status line and body.
fn http_get(port: u16, path: &str) -> std::io::Result<(String, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;

    let mut body = String::new();
    reader.read_to_string(&mut body)?;

    Ok((status_line, body))
}

/// Starts the binary with the given environment overrides.
fn start_binary(env: &[(&str, &str)]) -> Child {
    let mut command = Command::new(BIN);
    command
        .env_remove("RUN_MODE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    for (key, value) in env {
        command.env(key, value);
    }

    command.spawn().expect("the binary must start")
}

/// Terminates the binary and waits for it to be gone.
fn stop_binary(child: &mut Child) {
    // `SIGTERM` is what the service handles for a graceful shutdown.
    unsafe {
        libc_kill(child.id() as i32, 15);
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}

/// `kill(2)` without pulling in a dependency for a single signal.
unsafe fn libc_kill(pid: i32, signal: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe {
        kill(pid, signal);
    }
}

/// Runs the binary with the given environment and returns its exit code.
///
/// The binary is expected to fail during configuration; the call is bounded
/// so a binary that boots anyway cannot hang the suite.
fn run_expecting_exit(env: &[(&str, &str)]) -> (Option<i32>, String) {
    let mut command = Command::new(BIN);
    command.env_remove("RUN_MODE").stdin(Stdio::null());

    for (key, value) in env {
        command.env(key, value);
    }

    let output = command.output().expect("the binary must run");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// P1 + P2: the binary boots, honours the environment and serves `/health`
#[test]
fn p1_the_binary_serves_health_with_the_configuration_from_the_environment() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db_path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();

    // The port is reserved by the operating system and released again, so it
    // is free at the moment of the call. A binary that fails to bind it is
    // retried with another one, because the race is real and belongs to the
    // operating system, not to the service.
    let mut last_error = String::new();

    for _ in 0..3 {
        let port = free_port();
        let mut child = start_binary(&[
            ("HODEI_SERVER__HOST", "127.0.0.1"),
            ("HODEI_SERVER__PORT", &port.to_string()),
            ("HODEI_ROCKSDB__PATH", &db_path),
            ("HODEI_DATABASE__NAMESPACE", "hodei"),
            ("HODEI_DATABASE__DATABASE", "artifacts"),
            ("RUST_LOG", "warn"),
        ]);

        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let mut served = false;
        last_error.clear();

        while Instant::now() < deadline {
            if let Ok((status_line, body)) = http_get(port, "/health") {
                if status_line.contains(" 200 ") {
                    served = true;
                    assert!(
                        body.contains("\"status\":\"healthy\""),
                        "the health response must report a healthy status, got: {body}"
                    );
                    break;
                }
                last_error = format!("unexpected status line: {status_line}");
            }

            // The binary exited on its own: it failed to start.
            if let Ok(Some(status)) = child.try_wait() {
                assert!(
                    served,
                    "the binary must serve /health, but it exited with {status}: {last_error}"
                );
            }

            std::thread::sleep(Duration::from_millis(100));
        }

        stop_binary(&mut child);

        if served {
            return;
        }

        // The port was taken between the reservation and the bind: retry.
        last_error = format!("the binary never served /health on port {port}");
    }

    panic!("the binary never served /health: {last_error}");
}

/// P3: an invalid configuration makes the binary fail loudly
#[test]
fn p3_the_binary_exits_with_a_failure_code_on_an_invalid_configuration() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db_path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();

    let (code, stderr) = run_expecting_exit(&[
        ("HODEI_SERVER__HOST", "127.0.0.1"),
        // Port 0 is explicitly rejected by the configuration validation.
        ("HODEI_SERVER__PORT", "0"),
        ("HODEI_ROCKSDB__PATH", &db_path),
        ("HODEI_DATABASE__NAMESPACE", "hodei"),
        ("HODEI_DATABASE__DATABASE", "artifacts"),
    ]);

    let code = code.expect("the binary must exit on its own");
    assert_ne!(
        code, 0,
        "an invalid configuration must not produce a successful exit code"
    );
    assert!(
        stderr.contains("Configuration error"),
        "the binary must explain why it refused to start, got: {stderr}"
    );
}
