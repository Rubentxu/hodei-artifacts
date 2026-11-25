//! Fail-closed contract of the unique Dgraph profile (HSP-209).
//!
//! El AC#1 de HSP-209 tiene dos mitades y esta suite fija ambas sin tocar
//! red ni discos:
//!
//! 1. **El perfil se valida consigo mismo.** Un `AppConfig` con `[graph]`
//!    declarado valida la sección del alpha (endpoint con puerto, TLS con
//!    su CA, credenciales completas) y deja de validar —y de preflightar—
//!    el motor embebido: con `[graph]` no hay RocksDB que abrir. Un
//!    despliegue que declara mal el alpha no arranca, igual que uno que
//!    declara mal RocksDB en el perfil embebido.
//! 2. **El fallback a RocksDB está rechazado, no ausente.** El AC prohíbe
//!    explícitamente que un arranque del perfil productivo "intente abrir
//!    RocksDB" en silencio: lo que la suite fija es que ese intento es un
//!    error con nombre. Con `[graph]`, `[database] db_type = "rocksdb"`
//!    —la forma exacta en que los perfiles emitidos reclaman el motor
//!    embebido— produce el rechazo que nombra el motor rechazado, y el
//!    preflight del perfil de grafo no crea, abre ni toca el path que la
//!    sección `[rocksdb]` menciona. Sin `[graph]`, el contrato viejo sigue
//!    intacto: `dgraph` en `db_type` es rechazado por el validador del
//!    motor embebido, no encendido como si fuera un backend.
//!
//! Los tres caminos usan el mismo deserializador estratificado y el mismo
//! `validate()`/`preflight_storage()` que el arranque del binario, y los
//! ficheros que la distribución emite de verdad: una regresión aquí
//! significa que un perfil emitido arranca mal, no que un struct discute
//! consigo mismo.

use std::path::PathBuf;

use hodei_artifacts_api::config::AppConfig;

/// El perfil productivo del grafo tal y como lo emite la distribución.
const SHIPPED_GRAPH_PROFILE: &str = "config/dgraph.toml";

/// El perfil embebido tal y como lo emite la distribución (el que el grafo
/// debe seguir sin tocar).
const SHIPPED_RELEASE_PROFILE: &str = "config/release.toml";

/// The message the Dgraph profile answers with when the embedded engine is
/// claimed. It is the same constant the profile module and the boot share;
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

/// Sustituye una línea `clave = ...` de un perfil por el valor dado, sin
/// tocar el resto: la única variable bajo prueba es el knob declarado.
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
/// authority: `[graph]` presente y validada (endpoint con puerto, TLS con
/// su CA declarada, credenciales completas — la contraseña llega por el
/// entorno del proceso, que es la cadena que la config promete y que aquí
/// se simula con una fuente superpuesta), el motor embebido sin reclamar
/// (`db_type = "dgraph"`), y el preflight del perfil sin crear, abrir ni
/// tocar el path que `[rocksdb]` nombra.
#[test]
fn the_shipped_graph_profile_boots_on_the_single_authority() {
    // HODEI_GRAPH__CREDENTIALS__PASSWORD entra por el entorno del
    // proceso, nunca por el TOML: el overlay simula esa fuente sin tocar
    // el entorno compartido del test. Un TOML que exija la contraseña
    // para validar sería, además, el fallo que esta suite existe para
    // impedir.
    let config = deserialize_like_the_boot(&[
        shipped_profile(SHIPPED_GRAPH_PROFILE),
        "[graph.credentials]\npassword = \"from-the-process-environment\"".to_string(),
    ])
    .expect("config/dgraph.toml must be a bootable profile");
    assert!(
        config.graph.is_some(),
        "the graph profile declares its authority"
    );
    assert_ne!(
        config.database.db_type, "rocksdb",
        "the graph profile must not claim the embedded engine"
    );
    // El preflight del perfil de grafo es una decisión de autoridad, no un
    // check de redirección: el path embebido que el fichero nombra sigue
    // siendo inexistente después de preflightar.
    let nonexistent = {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "hodei-hsp209-rocksdb-should-not-exist-{}",
            std::process::id()
        ));
        path.to_string_lossy().into_owned()
    };
    let mut config = config;
    config.rocksdb.path = nonexistent.clone();
    config
        .preflight_storage()
        .expect("the graph profile preflight never touches the embedded engine");
    assert!(
        !std::path::Path::new(&nonexistent).exists(),
        "a graph deployment must not create the RocksDB path it names"
    );
}

/// G2: the rejection the AC#1 demands, in its exact observable form. With
/// `[graph]` declared, asking for the embedded engine —the `db_type =
/// "rocksdb"` of the shipped embedded profiles— is refused by an error
/// that names the refused engine, before anything opens anything.
#[test]
fn the_graph_profile_refuses_the_rocksdb_fallback_by_name() {
    let graph_profile = shipped_profile(SHIPPED_GRAPH_PROFILE);
    // La forma literal en que un despliegue intenta el fallback: mantener
    // [graph] y reclamar el motor embebido en [database].
    let claims_rocksdb = with_line(&graph_profile, "db_type", "\"rocksdb\"");
    let error = deserialize_like_the_boot(&[claims_rocksdb])
        .expect_err("db_type = \"rocksdb\" with [graph] must be refused, never booted");
    let message = error.to_string();
    assert!(
        message.contains(REFUSAL),
        "the rejection must name the refused engine, got: {message}"
    );
    assert!(
        message.contains("no fallback"),
        "the rejection must say there is no fallback, got: {message}"
    );
}

/// G3: a malformed graph section is a refusal, not a silent localhost or a
/// degraded boot: the endpoint without port fails, and TLS without its CA
/// fails. Both messages are the ones the user will see on the console.
#[test]
fn an_impossible_graph_deployment_does_not_boot() {
    let graph_profile = shipped_profile(SHIPPED_GRAPH_PROFILE);
    // Sin puerto no hay endpoint: ni localhost implícito ni intento.
    let no_port = with_line(&graph_profile, "endpoint", "\"alpha.internal\"");
    let error = deserialize_like_the_boot(&[no_port])
        .expect_err("an endpoint without port must be refused");
    assert!(
        error.to_string().to_lowercase().contains("endpoint"),
        "the error names the endpoint: {error}"
    );
    // TLS habilitado sin CA propia es la decisión que no se toma sola.
    let tls_without_ca = with_line(&graph_profile, "ca_cert_path", "none");
    let error = deserialize_like_the_boot(&[tls_without_ca])
        .expect_err("TLS without its CA must be refused");
    assert!(
        error.to_string().to_lowercase().contains("ca"),
        "the error names the CA: {error}"
    );
}

/// G4: el contrato viejo del motor embebido sigue vivo. Sin `[graph]`,
/// reclamar el backend del grafo en `db_type` era —y debe seguir siendo—
/// un rechazo nombrado; el nuevo perfil no reabrió esa puerta. Y el
/// perfil embebido emitido (`config/release.toml`) valida con el motor
/// embebido tal y como lo declara el fichero.
#[test]
fn the_embedded_contract_survives_the_graph_profile() {
    let release = shipped_profile(SHIPPED_RELEASE_PROFILE);
    assert!(
        !release.contains("[graph]"),
        "the embedded profile must not declare the graph section"
    );
    deserialize_like_the_boot(std::slice::from_ref(&release))
        .expect("config/release.toml must boot on the embedded engine");
    let error = deserialize_like_the_boot(&[with_line(&release, "db_type", "\"dgraph\"")])
        .expect_err("db_type = \"dgraph\" without [graph] must be refused, never booted");
    assert!(
        error.to_string().contains("dgraph"),
        "the refusal names the value the deployment asked for: {error}"
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
