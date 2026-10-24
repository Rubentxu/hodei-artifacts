//! Process-level contract for the *distributed* layout.
//!
//! # Why this file exists
//!
//! `tests/binary_contract.rs` runs the binary from the repository root, where
//! a readable configuration file happens to be. That is not how a consumer
//! uses the product: the consumer unpacks the distribution and runs the binary
//! from the unpacked directory.
//!
//! The difference was not academic. The first packaged distribution shipped
//! `config/example.toml`, a file the binary never reads — it reads
//! `config/default.toml` — so the extracted artifact refused to start with
//! "missing field `db_type`". The earlier acceptance had been executed from
//! the repository root, so it never noticed.
//!
//! # Guarantees
//!
//! - P4 The binary boots from a directory that contains only the binary and
//!   the configuration the distribution ships, and serves `/health`.
//! - P5 (negative) The same binary in a directory without that configuration
//!   refuses to start with an explicit error. A service that boots with a
//!   configuration nobody asked for is worse than one that refuses to boot.
//!
//! Each test owns a throwaway directory and binds an ephemeral port, so no
//! state and no port is shared.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The binary under test, as built by Cargo for this integration test.
const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");

/// The configuration file the distribution ships, relative to the repository
/// root. It is the file the service reads when nothing else overrides it.
const SHIPPED_CONFIG: &str = "config/default.toml";

/// How long the binary is given to serve `/health` before the test gives up.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the binary is given to refuse to start before the test gives up.
const REFUSAL_TIMEOUT: Duration = Duration::from_secs(30);

/// The root of the repository, as Cargo sees it.
const REPO_ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Reserves an ephemeral port and releases it again.
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

/// Copies the binary and, optionally, the shipped configuration into a
/// throwaway directory and returns that directory.
fn unpack_distribution(dir: &Path, with_configuration: bool) -> PathBuf {
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("the bin directory must be created");

    let binary = bin_dir.join("hodei-artifacts-api");
    std::fs::copy(BIN, &binary).expect("the binary must be copied");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
            .expect("the binary must be executable");
    }

    if with_configuration {
        let config_dir = dir.join("config");
        std::fs::create_dir_all(&config_dir).expect("the config directory must be created");
        std::fs::copy(
            Path::new(REPO_ROOT).join(SHIPPED_CONFIG),
            config_dir.join("default.toml"),
        )
        .expect("the shipped configuration must be copied");
    }

    dir.to_path_buf()
}

/// Starts the binary in the given directory with the given environment.
fn start_binary(dir: &Path, env: &[(&str, &str)]) -> Child {
    let mut command = Command::new(dir.join("bin").join("hodei-artifacts-api"));
    command
        .current_dir(dir)
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

/// A child process that is killed when the test ends, panic included.
///
/// Both tests start a real server. If an assertion fails —or the binary boots
/// when it is expected to refuse— the child must still die, or the session
/// leaves an orphan holding a port and a database lock.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// P4: the unpacked distribution boots on its own
#[test]
fn p4_the_unpacked_distribution_boots_on_its_own() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = unpack_distribution(tmp.path(), true);

    let db_path = dir.join("data").join("hodei.rocksdb");
    let mut last_error = String::new();

    for _ in 0..3 {
        let port = free_port();
        let mut child = KillOnDrop(start_binary(
            &dir,
            &[
                // Only the port and the database location are overridden: the
                // rest of the configuration comes from the shipped file.
                ("HODEI_SERVER__PORT", &port.to_string()),
                ("HODEI_ROCKSDB__PATH", &db_path.to_string_lossy()),
                ("RUST_LOG", "warn"),
            ],
        ));

        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let mut served = false;
        last_error.clear();

        while Instant::now() < deadline {
            if let Ok((status_line, body)) = http_get(port, "/health") {
                if status_line.contains(" 200 ") {
                    assert!(
                        body.contains("\"status\":\"healthy\""),
                        "the health response must report a healthy status, got: {body}"
                    );
                    served = true;
                    break;
                }
                last_error = format!("unexpected status line: {status_line}");
            }

            if let Ok(Some(status)) = child.0.try_wait() {
                assert!(
                    served,
                    "the unpacked distribution must serve /health, but the binary exited with {status}: {last_error}"
                );
            }

            std::thread::sleep(Duration::from_millis(100));
        }

        stop_binary(&mut child.0);

        if served {
            return;
        }

        last_error = format!("the binary never served /health on port {port}");
    }

    panic!("the unpacked distribution never served /health: {last_error}");
}

/// P5: without that configuration the binary refuses to start
#[test]
fn p5_the_binary_refuses_to_start_without_a_readable_configuration() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = unpack_distribution(tmp.path(), false);

    let mut command = Command::new(dir.join("bin").join("hodei-artifacts-api"));
    command
        .current_dir(&dir)
        .env_remove("RUN_MODE")
        // The port is valid on purpose: the only thing wrong with this
        // instance is the missing configuration, so the refusal can only come
        // from that.
        .env("HODEI_SERVER__PORT", free_port().to_string())
        .env(
            "HODEI_ROCKSDB__PATH",
            dir.join("data").join("hodei.rocksdb"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = KillOnDrop(command.spawn().expect("the binary must start"));

    // The binary is expected to refuse to start. The wait is bounded so a
    // binary that boots anyway cannot hang the suite.
    let deadline = Instant::now() + REFUSAL_TIMEOUT;
    let status = loop {
        match child.0.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };

    let status = status.expect("the binary must refuse to start on its own");
    assert!(
        !status.success(),
        "a binary without a readable configuration must not report a successful start"
    );

    let mut stderr = String::new();
    if let Some(mut pipe) = child.0.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    assert!(
        stderr.contains("Configuration error"),
        "the binary must explain why it refused to start, got: {stderr}"
    );
    assert!(
        stderr.contains("missing field"),
        "the refusal must name the missing configuration, got: {stderr}"
    );
}
