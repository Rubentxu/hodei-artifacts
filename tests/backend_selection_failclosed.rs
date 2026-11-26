//! Fail-closed contract for every backend-selection knob the service reads.
//!
//! # Why this file exists
//!
//! The product runs on a single remote Dgraph profile (HSP-209) and the
//! embedded SurrealDB-over-RocksDB engine was retired (HSP-314 F4). The one
//! unacceptable outcome is a quiet one: a deployment (or a default) asks for
//! a metadata backend the binary does not implement and the boot degrades
//! into something else. The bootstrap never falls back --
//! `DatabaseConfig::validate` refuses every `db_type` that is not the graph,
//! the content backend is a closed enum, and `backend = "s3"` without its
//! table is an error that says so -- and this suite pins those refusals.
//!
//! The knob is exercised through the same deserializer and the same
//! `validate()` the boot runs (`build_config` in `src/config.rs`), fed with
//! the very files the distribution ships, so a regression here means the
//! shipped profiles boot wrong, not merely that a struct disagrees with
//! itself. The shared base files declare no `[graph]` (the authority is the
//! deployment's decision), so the suite layers a minimal graph section over
//! them, as the environment of a real deployment does.
//!
//! # Guarantees
//!
//! - G1 Every shipped profile declares its backend knobs with values the
//!   binary actually wires: metadata on the graph (the only backend),
//!   content on the filesystem default.
//! - G2 (negative) A metadata backend that is not the graph -- the retired
//!   embedded engine included -- is refused at startup by an error that
//!   names the offending value. No route silently lands on another
//!   backend, and a missing `db_type` is not silently defaulted either.
//! - G3 (negative) The content backend is a closed choice: unknown values
//!   fail deserialization, `s3` without its table fails validation with the
//!   "no fallback" message, and an enabled `s3` backend is never demoted.
//!
//! Each test parses in-memory text and validates pure configuration, so no
//! state, port, database or network is touched and the suite is fully
//! deterministic.

use std::path::PathBuf;

use config::{Config, File, FileFormat};

use hodei_artifacts_api::config::{AppConfig, ContentBackend};

/// The configuration file the distribution ships, relative to the repository
/// root. It is the file the service reads when nothing else overrides it.
const SHIPPED_DEFAULT: &str = "config/default.toml";

/// The shipped profiles, in the same precedence order `build_config`
/// applies: `default.toml` first, then the per-mode file on top of it.
const SHIPPED_PROFILES: &[&str] = &["config/default.toml", "config/release.toml"];

/// The minimal graph section a deployment adds to the shared base files:
/// the authority is the deployment's decision, never the file's.
const GRAPH_OVERLAY: &str =
    "[graph]\nendpoint = \"127.0.0.1:9080\"\n\n[graph.tls]\nenabled = false\n";

/// The metadata backends the binary does not implement. The first is the
/// retired embedded engine; the rest are the spellings a typo or an
/// optimistic profile would use. Every one of them must be a refusal, never a
/// fallback to the implemented backend.
const UNIMPLEMENTED_METADATA_BACKENDS: &[&str] = &[
    "rocksdb",
    "remote",
    "postgres",
    "sqlite",
    "surrealkv",
    "memory",
    "Dgraph",
    " dgraph",
];

/// Reads one of the shipped profile files.
fn shipped_profile(relative: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative))
        .unwrap_or_else(|error| panic!("the shipped profile {relative} must be readable: {error}"))
}

/// Deserializes a configuration exactly the way the boot does: the layered
/// `config` sources, the same struct, the same `validate()`. The result is
/// either a wired configuration or the loud error the boot would report.
fn deserialize_like_the_boot(sources: &[String]) -> Result<AppConfig, config::ConfigError> {
    let mut builder = Config::builder();
    for source in sources {
        builder = builder.add_source(File::from_str(source, FileFormat::Toml));
    }
    let app_config: AppConfig = builder.build()?.try_deserialize()?;
    app_config.validate()?;
    Ok(app_config)
}

/// Deserializes one configuration text without running the validators, for
/// the cases where the refusal must come from deserialization itself.
fn deserialize_without_validation(source: &str) -> Result<AppConfig, config::ConfigError> {
    Config::builder()
        .add_source(File::from_str(source, FileFormat::Toml))
        .build()?
        .try_deserialize()
}

/// Replaces the `db_type` declaration of a shipped profile, keeping the rest
/// of the file untouched so the only variable under test is the backend name.
fn with_metadata_backend(profile: &str, db_type: &str) -> String {
    let replacement = format!("db_type = \"{db_type}\"");
    let replaced = profile
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("db_type") {
                replacement.clone()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        replaced.contains(&replacement),
        "the shipped profile declares db_type; the replacement must have happened"
    );
    replaced
}

/// G1: every shipped profile declares backend knobs the binary actually
/// wires. The metadata backend of every shipped profile is `dgraph` -- the
/// only implemented one -- the schema storage agrees with it, and
/// the content backend resolves to the filesystem default the files
/// document. A profile whose declaration drifted from the wiring must not
/// pass unnoticed.
#[test]
fn shipped_profiles_declare_only_implemented_backends() {
    for profile in SHIPPED_PROFILES {
        let text = shipped_profile(profile);
        let config = deserialize_like_the_boot(&[text, GRAPH_OVERLAY.to_string()])
            .unwrap_or_else(|error| panic!("{profile} must boot: {error}"));

        assert_eq!(
            config.database.db_type, "dgraph",
            "{profile} must declare the metadata backend the binary implements"
        );
        assert_eq!(
            config.schema.storage_type, "dgraph",
            "{profile} must declare a schema storage consistent with the metadata backend"
        );
        assert_eq!(
            config.content.backend,
            ContentBackend::Filesystem,
            "{profile} must leave the content backend at the documented filesystem default"
        );
    }
}

/// G1 (companion): the shipped profiles make exactly the backend selections
/// this suite knows about -- `database.db_type` and `schema.storage_type` in
/// each file, and no uncommented `backend` in the `[content]` table. A new
/// backend-selection knob added to a shipped profile must either be added to
/// this enumeration with its own fail-closed contract, or not ship. This is
/// the enumeration half of "enumerate the connections of the target
/// profile": the set of knobs is closed and every member is checked above.
#[test]
fn the_shipped_backend_selections_are_exactly_the_enumerated_knobs() {
    for profile in SHIPPED_PROFILES {
        let text = shipped_profile(profile);
        let mut declared: Vec<&str> = Vec::new();
        for line in text.lines() {
            let line = line.trim_start();
            if line.starts_with('#') {
                continue;
            }
            let Some((key, _)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if key == "db_type" || key == "storage_type" || key == "backend" {
                declared.push(key);
            }
        }
        declared.sort_unstable();
        assert_eq!(
            declared,
            ["db_type", "storage_type"],
            "{profile} must select backends only through the enumerated knobs \
             (database.db_type and schema.storage_type); a new knob needs its \
             own fail-closed contract here before it ships"
        );
    }
}

/// G2: a metadata backend that is not the graph is refused at startup, and
/// the refusal names the value the deployment asked for. The retired
/// embedded engine cannot boot back in through a stale `db_type`.
#[test]
fn unimplemented_metadata_backends_are_refused_not_fallback() {
    let default = shipped_profile(SHIPPED_DEFAULT);
    for db_type in UNIMPLEMENTED_METADATA_BACKENDS {
        let sources = [
            with_metadata_backend(&default, db_type),
            GRAPH_OVERLAY.to_string(),
        ];
        let error = deserialize_like_the_boot(&sources)
            .err()
            .unwrap_or_else(|| panic!("db_type = \"{db_type}\" must be refused, not booted"));
        let message = error.to_string();
        assert!(
            message.contains(db_type.trim()),
            "the refusal of db_type = \"{db_type}\" must name the offending value, got: {message}"
        );
        assert!(
            message.contains("dgraph"),
            "the refusal of db_type = \"{db_type}\" must state which backend exists, got: {message}"
        );
    }
}

/// G2 (companion): a profile without a `db_type` at all does not boot either.
/// The hardcoded default exists for structs, not for deployments: the
/// distributed files declare the backend explicitly, and a file that forgot
/// must fail deserialization rather than inherit one silently.
#[test]
fn a_profile_without_metadata_backend_is_a_deserialize_error() {
    let default = shipped_profile(SHIPPED_DEFAULT);
    let without_knob = default
        .lines()
        .filter(|line| !line.trim_start().starts_with("db_type"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = match deserialize_without_validation(&without_knob) {
        Ok(_) => panic!("a profile without db_type must not deserialize"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("db_type"),
        "the missing backend must be reported as such, got: {error}"
    );
}

/// G3: the content backend is a closed choice. An unknown backend name fails
/// deserialization, `s3` without its table fails validation with the
/// explicit "no fallback" message, and an `s3` backend that IS configured
/// stays configured -- an enabled remote route is never demoted to the
/// local directory.
#[test]
fn the_content_backend_is_a_closed_choice() {
    let default = shipped_profile(SHIPPED_DEFAULT);

    // An unimplemented content backend is not a name the enum accepts.
    let unknown = default.replace("[content]", "[content]\nbackend = \"dgraph\"");
    let outcome = deserialize_without_validation(&unknown);
    assert!(
        outcome.is_err(),
        "content.backend = \"dgraph\" must fail deserialization, not fall back"
    );

    // `s3` without its table is the configuration-level refusal the bootstrap
    // restates: there is no fallback to the filesystem.
    let s3_without_table = default.replace("[content]", "[content]\nbackend = \"s3\"");
    let error = deserialize_like_the_boot(&[s3_without_table, GRAPH_OVERLAY.to_string()])
        .err()
        .unwrap_or_else(|| panic!("backend = \"s3\" without [content.s3] must be refused"));
    let message = error.to_string();
    assert!(
        message.contains("no fallback"),
        "the refusal must say there is no fallback, got: {message}"
    );

    // An empty bucket is the same refusal in other words.
    let s3_empty_bucket = default.replace(
        "[content]",
        "[content]\nbackend = \"s3\"\n\n[content.s3]\nbucket = \"  \"",
    );
    let error = deserialize_like_the_boot(&[s3_empty_bucket, GRAPH_OVERLAY.to_string()])
        .err()
        .unwrap_or_else(|| panic!("backend = \"s3\" with an empty bucket must be refused"));
    assert!(
        error.to_string().contains("bucket"),
        "the refusal must name the bucket, got: {error}"
    );

    // The enabled remote route survives validation untouched: choosing a
    // backend is keeping it, never being talked out of it.
    let s3_configured = default.replace(
        "[content]",
        "[content]\nbackend = \"s3\"\n\n[content.s3]\nbucket = \"hodei-content\"\nregion = \"eu-west-1\"",
    );
    let config = deserialize_like_the_boot(&[s3_configured, GRAPH_OVERLAY.to_string()])
        .unwrap_or_else(|error| panic!("a configured s3 backend must boot: {error}"));
    assert_eq!(config.content.backend, ContentBackend::S3);
}
