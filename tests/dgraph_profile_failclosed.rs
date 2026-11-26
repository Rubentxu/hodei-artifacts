//! Fail-closed contract of the unique Dgraph profile (HSP-209).
//!
//! HSP-209 AC#1 has two halves and this suite pins both without touching
//! network or disks:
//!
//! 1. **The profile validates against itself.** An `AppConfig` with `[graph]`
//!    declared validates the alpha section (endpoint with port, TLS with
//!    its CA, complete credentials). A deployment that declares the
//!    alpha wrongly does not start.
//! 2. **There is no other profile.** Since HSP-314 F4 the embedded engine does not exist:
//!    with `[graph]`, `[database] db_type = "rocksdb"` produces a rejection that
//!    names the rejected value, and a configuration without `[graph]` (the
//!    old profile) is rejected with the migration message.
//!
//! The paths use the same layered deserializer and the same
//! `validate()`/`preflight_storage()` as the binary's boot, and the
//! files the distribution actually emits: a regression here
//! means an emitted profile boots badly, not that a struct argues
//! with itself.

use std::path::PathBuf;

use hodei_artifacts_api::config::AppConfig;

/// The production graph profile as the distribution emits it.
const SHIPPED_GRAPH_PROFILE: &str = "config/dgraph.toml";

/// The base the distribution emits for all profiles: without `[graph]`
/// it is not a bootable profile.
const SHIPPED_BASE_PROFILE: &str = "config/default.toml";

/// The message the Dgraph profile answers with when a backend that is not the
/// graph is claimed. It is the same constant the profile module and the boot share;
/// a change in one that does not change the other must fail here.
const REFUSAL: &str = hodei_artifacts_api::profile::REFUSAL_MESSAGE;

/// Reads one of the shipped profile files, as `build_config` will.
fn shipped_profile(relative: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative))
        .unwrap_or_else(|error| panic!("the shipped profile {relative} must be readable: {error}"))
}

/// Deserializes and validates a configuration exactly the way the boot
/// does — same layered sources, same struct, same `validate()`. The
/// result is either a wired configuration or the loud error the boot
/// would report.
fn deserialize_like_the_boot(sources: &[String]) -> Result<AppConfig, config::ConfigError> {
    let mut builder = config::Config::builder();
    for source in sources {
        builder = builder.add_source(config::File::from_str(source, config::FileFormat::Toml));
    }
    let app_config: AppConfig = builder.build()?.try_deserialize()?;
    app_config.validate()?;
    Ok(app_config)
}

/// Replaces a `key = ...` line of a profile with the given value, without
/// touching the rest: the only variable under test is the declared knob.
fn with_line(profile: &str, key: &str, value: &str) -> String {
    let replacement = format!("{key} = {value}");
    let replaced = profile
        .lines()
        .map(|line| {
            if line.trim_start().starts_with(&format!("{key} =")) {
                replacement.clone()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        replaced.contains(&replacement),
        "the profile declares {key}; the replacement must have happened"
    );
    replaced
}

/// G1: the shipped graph profile is a valid deployment of the single
/// authority: `[graph]` present and validated (endpoint with port, TLS with
/// its declared CA, complete credentials: the password arrives through the
/// process environment, which is the chain the config promises and which here
/// is simulated with an overlaid source), and `db_type = "dgraph"`.
#[test]
fn the_shipped_graph_profile_boots_on_the_single_authority() {
    // HODEI_GRAPH__CREDENTIALS__PASSWORD comes in through the process
    // environment, never through the TOML: the overlay simulates that source without touching
    // the shared test environment. A TOML that required the password
    // to validate would be, moreover, the failure this suite exists to
    // prevent.
    let config = deserialize_like_the_boot(&[
        shipped_profile(SHIPPED_GRAPH_PROFILE),
        "[graph.credentials]\npassword = \"from-the-process-environment\"".to_string(),
    ])
    .expect("config/dgraph.toml must be a bootable profile");
    assert!(
        config.graph.is_some(),
        "the graph profile declares its authority"
    );
    assert_eq!(
        config.database.db_type, "dgraph",
        "the graph profile declares the graph as its backend"
    );
}

/// G2: the rejection the AC#1 demands, in its exact observable form. With
/// `[graph]` declared, asking for the retired embedded engine —the `db_type =
/// "rocksdb"` of the old profiles— is refused by an error that names the
/// refused value, before anything opens anything.
#[test]
fn the_graph_profile_refuses_the_rocksdb_fallback_by_name() {
    let graph_profile = shipped_profile(SHIPPED_GRAPH_PROFILE);
    // The literal way a deployment attempts the fallback: keep
    // [graph] and claim the embedded engine in [database].
    let claims_rocksdb = with_line(&graph_profile, "db_type", "\"rocksdb\"");
    let error = deserialize_like_the_boot(&[claims_rocksdb])
        .expect_err("db_type = \"rocksdb\" with [graph] must be refused, never booted");
    let message = error.to_string();
    assert!(
        message.contains(REFUSAL),
        "the rejection must name the refused engine, got: {message}"
    );
    assert!(
        message.contains("rocksdb") && message.contains("single authority"),
        "the rejection must name the value and the single authority, got: {message}"
    );
}

/// G3: a malformed graph section is a refusal, not a silent localhost or a
/// degraded boot: the endpoint without port fails, and TLS without its CA
/// fails. Both messages are the ones the user will see on the console.
#[test]
fn an_impossible_graph_deployment_does_not_boot() {
    let graph_profile = shipped_profile(SHIPPED_GRAPH_PROFILE);
    // Without a port there is no endpoint: neither an implicit localhost nor an attempt.
    let no_port = with_line(&graph_profile, "endpoint", "\"alpha.internal\"");
    let error = deserialize_like_the_boot(&[no_port])
        .expect_err("an endpoint without port must be refused");
    assert!(
        error.to_string().to_lowercase().contains("endpoint"),
        "the error names the endpoint: {error}"
    );
    // TLS enabled without its own CA is a decision that does not take itself.
    let tls_without_ca = with_line(&graph_profile, "ca_cert_path", "none");
    let error = deserialize_like_the_boot(&[tls_without_ca])
        .expect_err("TLS without its CA must be refused");
    assert!(
        error.to_string().to_lowercase().contains("ca"),
        "the error names the CA: {error}"
    );
}

/// G4: there is no embedded profile. The base the distribution emits, without
/// `[graph]`, is the old profile and is rejected with the migration
/// message; and claiming the graph backend without declaring `[graph]` is
/// still a rejection, not a boot.
#[test]
fn a_configuration_without_the_graph_is_refused_with_the_migration_message() {
    let base = shipped_profile(SHIPPED_BASE_PROFILE);
    assert!(
        !base.contains("[graph]"),
        "the shared base must not declare the graph section"
    );
    let error = deserialize_like_the_boot(std::slice::from_ref(&base))
        .expect_err("a configuration without [graph] must be refused, never booted");
    assert!(
        error
            .to_string()
            .contains(hodei_artifacts_api::config::GRAPH_PROFILE_REQUIRED),
        "the refusal carries the migration message: {error}"
    );
    let error = deserialize_like_the_boot(&[with_line(&base, "db_type", "\"rocksdb\"")])
        .expect_err("db_type = \"rocksdb\" without [graph] must be refused, never booted");
    assert!(
        error
            .to_string()
            .contains(hodei_artifacts_api::config::GRAPH_PROFILE_REQUIRED),
        "without [graph] the profile itself is what is refused: {error}"
    );
}

/// HSP-307: `namespace` placed after `[graph.credentials]` used to be read
/// as `graph.credentials.namespace` and silently discarded, so a value
/// other than 0 was ignored. Unknown keys under `[graph.*]` are now an
/// error that names the key, wherever they are declared.
#[test]
fn a_misplaced_graph_namespace_is_rejected_by_name() {
    let shipped = shipped_profile(SHIPPED_GRAPH_PROFILE);
    let password = "[graph.credentials]\npassword = \"from-the-process-environment\"".to_string();
    // The pre-fix shape: `namespace` after the `[graph.credentials]` header.
    for table in ["graph.credentials", "graph.tls"] {
        let misplaced = format!("[{table}]\nnamespace = 7");
        let error =
            deserialize_like_the_boot(&[shipped.clone(), password.clone(), misplaced.clone()])
                .expect_err("a namespace under a graph subtable must not be dropped silently")
                .to_string();
        assert!(
            error.contains("namespace"),
            "the error must name the unknown key under [{table}], got: {error}"
        );
    }
    let error = deserialize_like_the_boot(&[
        shipped,
        password,
        "[graph]\nnot_a_graph_key = 1".to_string(),
    ])
    .expect_err("an unknown key directly under [graph] must be rejected")
    .to_string();
    assert!(error.contains("not_a_graph_key"), "got: {error}");
}

/// HSP-307: the well-placed `namespace` reaches `graph.namespace`, and the
/// shipped profile keeps declaring it under `[graph]`.
#[test]
fn a_graph_namespace_under_graph_reaches_the_client_config() {
    let shipped = shipped_profile(SHIPPED_GRAPH_PROFILE);
    let password = "[graph.credentials]\npassword = \"from-the-process-environment\"".to_string();
    let config = deserialize_like_the_boot(&[shipped.clone(), password.clone()])
        .expect("the shipped profile boots");
    assert_eq!(config.graph.as_ref().expect("graph").namespace, 0);
    let config = deserialize_like_the_boot(&[shipped, password, "[graph]\nnamespace = 7".into()])
        .expect("a namespace under [graph] is valid");
    assert_eq!(config.graph.as_ref().expect("graph").namespace, 7);
}
