//! Process-level contract for the `hodei-artifacts-api` binary.
//!
//! HSP-314 F2: this target runs on the default (productive) build, with no
//! `legacy` feature. It holds the guarantee that does not depend on a backend:
//!
//! - P3 (negative) The binary exits with a failure code and an explicit
//!   message when the configuration is invalid. A service that boots with a
//!   configuration nobody asked for is worse than one that refuses to boot.
//!
//! The boot-and-serve guarantees (P1, P2) need a metadata authority. On the
//! graph profile they are exercised against a real Dgraph and S3 by
//! `hsp237_profile_external_e2e`; on the embedded profile they stay in
//! `binary_contract_embedded` (legacy). Each test owns a throwaway directory.

use std::process::Command;

/// The binary under test, as built by Cargo for this integration test.
const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");

/// Runs the binary with the given environment and returns its exit code.
///
/// The binary is expected to fail during configuration; the call is bounded
/// so a binary that boots anyway cannot hang the suite.
fn run_expecting_exit(env: &[(&str, &str)]) -> (Option<i32>, String) {
    let mut command = Command::new(BIN);
    command
        .env_remove("RUN_MODE")
        .stdin(std::process::Stdio::null());

    for (key, value) in env {
        command.env(key, value);
    }

    let output = command.output().expect("the binary must run");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// P3: an invalid configuration makes the binary fail loudly
#[test]
fn p3_the_binary_exits_with_a_failure_code_on_an_invalid_configuration() {
    let (code, stderr) = run_expecting_exit(&[
        // The graph profile, the productive one: the refusal comes from the
        // configuration validation, before any backend is contacted.
        ("RUN_MODE", "dgraph"),
        ("HODEI_SERVER__HOST", "127.0.0.1"),
        // Port 0 is explicitly rejected by the configuration validation.
        ("HODEI_SERVER__PORT", "0"),
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
