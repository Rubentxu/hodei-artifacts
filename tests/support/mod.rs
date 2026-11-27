//! The conformance harness: a **real** service, a real `curl` and a real
//! `sha256sum`.
//!
//! What is checked here is not a Rust function, but a process
//! that starts, listens on a port and serves bytes: that is why everything
//! used to talk to it is a system tool and not the
//! code under test.
//!
//! The comparison of the bytes is done by `sha256sum`, which has nothing to
//! do with this service. If the hash that comes back is not the hash that went in,
//! the content is not the same, and that is not decided by the code under test.
//!
//! And the result is **written**: a report with the version, the command of
//! each case, the real result and the limits. A conformance that only
//! exists in a CI log is a conformance that nobody can read
//! six months from now.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fs::{File, FileTimes};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;
use sha2::Digest as _;

/// The registry laboratory: the upstream with a counter, the outcome of
/// a case and the JSON report of a run.
///
/// It lives here and not in the file of one case because seven more tasks are
/// going to use it: an upstream with a counter that is implemented twice is a
/// counter that measures one of the two.
pub mod lab;

/// The podman laboratory of the graph profile (real Dgraph and S3): used by the
/// binary walkthrough (HSP-237) and the OCI image one (HSP-308).
pub mod graph_lab;

/// The graph profile of the harness (HSP-314 F4 S2a): one ACL namespace of a
/// real Dgraph per deployment, and the seeders over the real catalog store.
pub mod graph_profile;

/// The risk facts of a deployment on the graph profile (HSP-314 F4 S2c): the
/// ledger of deltas and the materialized impact view, seeded through the real
/// stores.
pub mod graph_risk;

/// Whether the deployments of this build run on the graph profile: always,
/// the embedded engine does not exist (HSP-314 F4).
pub fn graph_profile() -> bool {
    true
}

/// The service binary, as the CI compiles it.
pub const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");

/// The secret of the test tokens.
///
/// It is a test secret, and it is here on purpose: the token is signed
/// by the harness itself, and a token that cannot be forged would prove
/// nothing.
pub const SECRET: &[u8] = b"raw-conformity-secret-long-enough";

/// The issuer that the tokens declare.
pub const ISSUER: &str = "https://issuer.example";

/// The audience that the tokens declare.
pub const AUDIENCE: &str = "hodei-artifacts";

/// The account of the first person.
pub const FIRST_ACCOUNT: &str = "acme";

/// The secret of the signing key of the test deployments: 64 hex characters,
/// as `openssl rand -hex 32` writes them. A fixture, never a real key.
pub const SIGNING_SECRET: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";

/// A second fixture secret, for the rotation cases.
pub const SIGNING_SECRET_NEXT: &str =
    "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

/// The environment that gives a test deployment a signing ring.
///
/// Each entry of `ring` is a key id and the name of the environment variable
/// that holds its secret; the first signs and all of them verify. The
/// secrets themselves are passed in the variables that the names point at.
pub fn signing_environment(ring: &[(&str, &str, &str)]) -> Vec<(String, String)> {
    let mut environment = Vec::new();
    for (index, (id, variable, secret)) in ring.iter().enumerate() {
        environment.push((format!("HODEI_SIGNING__KEYS__{index}__ID"), id.to_string()));
        environment.push((
            format!("HODEI_SIGNING__KEYS__{index}__SECRET_ENV"),
            variable.to_string(),
        ));
        environment.push((variable.to_string(), secret.to_string()));
    }
    environment
}

/// The ring most cases use: one key, `k1`.
pub fn the_signing_ring() -> Vec<(String, String)> {
    signing_environment(&[("k1", "HODEI_TEST_SIGNING_SECRET_K1", SIGNING_SECRET)])
}

/// The account of the second person.
pub const SECOND_ACCOUNT: &str = "other";

/// The key of the first account's repository.
pub const FIRST_REPOSITORY: &str = "releases";

/// The key of the second account's repository.
pub const SECOND_REPOSITORY: &str = "releases";

/// The key of a repository that only the first account has.
///
/// It serves the foreign-account case: a repository that exists in
/// one account and not in the other. The response of the account that does not have it
/// cannot say whether it exists.
pub const FIRST_ONLY_REPOSITORY: &str = "private";

/// The Cedar schema of the deployment.
///
/// It declares the two actions of the raw content surface, because
/// a bundle that does not declare them cannot grant them, and the service does not
/// start with a bundle that does not declare what it authorizes with.
pub const SCHEMA: &str = r#"{
  "": { "entityTypes": {}, "actions": {} },
  "Iam": { "entityTypes": { "User": { "shape": { "type": "Record", "attributes": {} } } }, "actions": {} },
  "Artifacts": {
    "entityTypes": {
      "Repository": { "memberOfTypes": ["Artifacts::Repository"], "shape": { "type": "Record", "attributes": { "namespace": { "type": "String", "required": false }, "tags": { "type": "Set", "element": { "type": "String" }, "required": false } } } },
      "Artifact": { "shape": { "type": "Record", "attributes": {} } }
    },
    "actions": {
      "CreateRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ManageRepositoryTags": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ManageRepositoryAdmission": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } }, "ManageRepositoryFreeze": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ReadRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "PromoteAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "RevokePromotion": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "VerifyTrustPassport": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ReadCapsule": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Artifact"] } },
      "UploadAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "DeleteAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "IngestExecutionReceipt": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "DownloadAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } }
    }
  }
}"#;

/// The Cedar policy of the deployment.
///
/// Both people can publish and download. What the policy does **not** decide
/// is the isolation between accounts: that is carried by the key of
/// each catalog row, and that is why the foreign-account case expects
/// a `404` and not a `403`.
pub fn the_policy() -> String {
    let mut policy = String::new();
    for (account, user) in [(FIRST_ACCOUNT, "alice"), (SECOND_ACCOUNT, "bob")] {
        for action in ["UploadAsset", "DownloadAsset"] {
            policy.push_str(&format!(
                "permit(principal == Iam::User::\"hrn:hodei:iam::{account}:User/{user}\", action == Artifacts::Action::\"{action}\", resource);\n"
            ));
        }
    }
    // Only alice promotes and revokes: bob can read and write, but the
    // decision of which software is trustworthy is not his.
    for action in ["PromoteAsset", "RevokePromotion"] {
        policy.push_str(&format!(
            "permit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Artifacts::Action::\"{action}\", resource);\n"
        ));
    }
    policy
}

/// What a token says.
#[derive(Serialize)]
struct Claims {
    sub: String,
    account: String,
    exp: usize,
    iss: String,
    aud: String,
}

/// The token of a person of an account.
///
/// The token's account is the one the service verifies: not the one the
/// caller says. A token from another account is a token from another account, and
/// that is why the `404` of the foreign repository does not say whether it exists.
///
/// The token is minted with a name, and that name is the one that goes to the
/// report: the real token is never written to a file that is
/// archived. The name comes from here, from the same function that mints the
/// token, so the two cannot get out of sync.
pub fn token(account: &str, user: &str) -> String {
    let signed = encode(
        &Header::new(Algorithm::HS256),
        &Claims {
            sub: format!("hrn:hodei:iam::{account}:User/{user}"),
            account: account.to_string(),
            exp: 4_000_000_000,
            iss: ISSUER.to_string(),
            aud: AUDIENCE.to_string(),
        },
        &EncodingKey::from_secret(SECRET),
    )
    .expect("the token is signed");
    remember_the_name_of(&signed, user);
    signed
}

/// The names of the credentials that the harness has minted.
///
/// The report says **who** acted, not what they authenticated with: the token does not
/// travel to the report, and a token in an archived file is a
/// credential in an archived file.
fn the_names() -> &'static Mutex<HashMap<String, String>> {
    static NAMES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    NAMES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The name with which a credential is written.
///
/// A credential that the harness did not mint has no known name, and is
/// written as redacted: the name is what can be said in the clear.
pub fn the_name_of(token: &str) -> String {
    if token.is_empty() {
        return "<sin credencial>".to_string();
    }
    the_names()
        .lock()
        .expect("the names of the credentials")
        .get(token)
        .cloned()
        .unwrap_or_else(|| "<redactada>".to_string())
}

/// Records the name of a minted credential.
fn remember_the_name_of(token: &str, user: &str) {
    the_names()
        .lock()
        .expect("the names of the credentials")
        .insert(token.to_string(), format!("<{user}>"));
}

/// A free port of the system.
///
/// The port is opened and closed: the service occupies it afterwards. It is a
/// race that is almost never lost, and the harness expects it and retries if
/// the service does not manage to open the port.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("the address of the port")
        .port()
}

/// The port on which the service **says** it listens.
///
/// The service writes it in its log **after** having occupied the
/// port, so that line is the proof that the process that
/// wrote it owns the port. A `connect` that works does not
/// prove it: the other process may hold the port and ours
/// may have died for not being able to obtain it.
fn the_bound_port(log: &Path) -> Option<u16> {
    let content = std::fs::read_to_string(log).ok()?;
    let line = content
        .lines()
        .find(|line| line.contains("Listening on http://"))?;
    let address = without_the_colors(line.split("Listening on http://").nth(1)?);
    address.trim().rsplit(':').next()?.trim().parse().ok()
}

/// Strips the escape sequences from the log.
///
/// The log format may carry color codes, and a port with a
/// `\u{1b}[0m` glued to it cannot be read as a number.
pub fn without_the_colors(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            plain.push(character);
            continue;
        }
        // It is an escape sequence: it is eaten up to its final letter.
        for escaped in characters.by_ref() {
            if escaped.is_ascii_alphabetic() {
                break;
            }
        }
    }
    plain
}

/// The name the store gives to a write in progress.
///
/// `<pid>-<nanos:x>-<counter:x>.part`: the pid of the writing process, the
/// instant and a counter. The harness copies it because what it wants is a
/// **real** staging, with the name the store recognizes: a
/// file with another name is not a write in progress, and the store does not
/// touch it.
fn a_staging_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos:x}-{counter:x}.part", std::process::id())
}

/// A clean deployment: a directory, a database, a store, the
/// policies and the schema.
///
/// "Clean" means **empty**: the database and the store are created here,
/// and what is in them is what the test has put there.
pub struct Deployment {
    /// The directory that contains everything.
    pub directory: tempfile::TempDir,
    /// The database: the embedded engine's directory or, in the graph profile,
    /// the tenant locator (`graph://<namespace>/<key>`) that the
    /// seeder process reads.
    pub db: String,
    /// The deployment's own Dgraph namespace (graph profile): it is deleted
    /// when the deployment is dropped.
    pub graph: Option<graph_profile::GraphTenant>,
    /// The content store directory.
    pub cas: String,
    /// The content store staging directory.
    pub staging: PathBuf,
    /// The staging files that the harness has **locked**.
    ///
    /// The lock is what says that an upload is in progress, and the
    /// file lives as long as the descriptor lives: that is why they are kept
    /// here and not in a loose variable.
    lives: Mutex<Vec<std::fs::File>>,
}

impl Deployment {
    /// An empty deployment.
    pub fn new() -> Self {
        let directory = tempfile::tempdir().expect("a temp dir");
        let graph = graph_profile().then(graph_profile::GraphTenant::create);
        let db = match &graph {
            Some(tenant) => tenant.locator(),
            None => directory.path().join("db").to_string_lossy().into_owned(),
        };
        let cas = directory.path().join("cas").to_string_lossy().into_owned();
        let staging = PathBuf::from(&cas).join("staging");
        let policy = directory.path().join("policies.cedar");
        let schema = directory.path().join("schema.json");
        std::fs::write(&policy, the_policy()).expect("the policy is written");
        std::fs::write(&schema, SCHEMA).expect("the schema is written");
        Self {
            directory,
            db,
            graph,
            cas,
            staging,
            lives: Mutex::new(Vec::new()),
        }
    }

    /// The path of the policy file.
    fn policy_path(&self) -> PathBuf {
        self.directory.path().join("policies.cedar")
    }

    /// The path of the schema file.
    fn schema_path(&self) -> PathBuf {
        self.directory.path().join("schema.json")
    }

    /// The writes in progress that there are in the store.
    pub fn staging_entries(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.staging) else {
            return Vec::new();
        };
        let mut found: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        found.sort();
        found
    }

    /// The bytes that are written in the staging.
    ///
    /// It is what separates "the service has opened the file" from "the
    /// service is writing the body": an empty staging means
    /// the upload has not started arriving yet, and killing the process
    /// there is not killing an upload halfway.
    pub fn staging_bytes(&self) -> u64 {
        self.staging_entries()
            .iter()
            .filter_map(|path| std::fs::metadata(path).ok())
            .map(|metadata| metadata.len())
            .sum()
    }

    /// Ages the staging files.
    ///
    /// Ageing a file is what time does, and the harness cannot
    /// wait two hours: what is wanted is an abandoned
    /// staging, and the expiry is decided by the **modification date**.
    pub fn age_the_staging(&self, age: Duration) {
        let when = SystemTime::now() - age;
        for path in self.staging_entries() {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("the staging file opens");
            file.set_times(FileTimes::new().set_modified(when))
                .expect("the staging file ages");
        }
    }

    /// Leaves a **live** staging, like that of an upload from another process.
    ///
    /// It is a freshly written file in the staging directory, with the
    /// name the store gives to a write, and **locked**:
    /// the lock is what says the upload is in progress, and what
    /// the startup has to respect. An unlocked file would be that
    /// of a process that was left without writing, and that one does get cleaned.
    ///
    /// The descriptor stays in the deployment, because the lock lives in
    /// the descriptor: if it were dropped, the staging would stop being alive.
    pub fn a_live_staging(&self) -> PathBuf {
        let path = self.staging.join(a_staging_name());
        let file = std::fs::File::create(&path).expect("the live staging is written");
        file.try_lock().expect("the live staging is locked");
        self.lives
            .lock()
            .expect("the lives of the harness")
            .push(file);
        path
    }

    /// Seeds the catalog with one repository per account.
    ///
    /// The seeding goes in a **separate process**, and not for convenience: the
    /// embedded engine holds the directory lock while there is an
    /// open connection, and the service is another process that has to be able to
    /// open the database.
    pub fn seed(&self) {
        self.seed_with(&[]);
    }

    /// Seeds the catalog with one repository per account **and** the ones that are
    /// requested.
    ///
    /// The extra repositories are those needed by a suite that pushes to
    /// a named repository: the registry only publishes to repositories
    /// that exist, and without seeding it the suite would measure a 404 where it wanted to
    /// measure a push.
    pub fn seed_with(&self, extra: &[&str]) {
        self.seed_trusted(extra, &[]);
    }

    /// Seeds like [`Self::seed_with`] and leaves in the **trusted** channel the
    /// repositories of the first account that are named.
    ///
    /// The channel is a repository setting (`config_settings.trust.serve`)
    /// and there is no HTTP route that creates catalog repositories, so
    /// it is set when seeding, as whoever operates the deployment would do.
    pub fn seed_trusted(&self, extra: &[&str], trusted: &[&str]) {
        self.seed_proxies(extra, trusted, &[]);
    }

    /// Seeds like [`Self::seed_trusted`] and also creates **proxy**
    /// repositories of the first account: each one with its key and the
    /// configuration of its upstream (`config_settings.proxy.upstream`).
    pub fn seed_proxies(
        &self,
        extra: &[&str],
        trusted: &[&str],
        proxies: &[(&str, serde_json::Value)],
    ) {
        let proxies: serde_json::Map<String, serde_json::Value> = proxies
            .iter()
            .map(|(key, upstream)| (key.to_string(), upstream.clone()))
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_SEED_EXTRA", extra.join(","))
            .env("HODEI_SEED_TRUSTED", trusted.join(","))
            .env(
                "HODEI_SEED_PROXY",
                serde_json::Value::Object(proxies).to_string(),
            )
            .status()
            .expect("the seeder runs");
        assert!(status.success(), "the catalog could not be seeded");
    }

    /// Seeds the catalog declaring the listed repositories **replaceable**:
    /// their `config_settings.raw.replace` is `allow`, and publishing
    /// to an occupied address is a replacement and not a 409.
    ///
    /// The policy cannot be requested over HTTP: it is decided by whoever operates the
    /// deployment when seeding, like the trust channel.
    pub fn seed_with_replacement(&self, replace: &[&str]) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_SEED_REPLACE", replace.join(","))
            .status()
            .expect("the seeder runs");
        assert!(status.success(), "the catalog could not be seeded");
    }

    /// Reconfigures an already seeded proxy repository: replaces its
    /// `config_settings.proxy` and raises its configuration version.
    ///
    /// It is done by a process that ends right away and with the service stopped: the
    /// database is locked by the process that opens it.
    pub fn reconfigure_proxy(&self, key: &str, proxy: serde_json::Value) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_RECONFIG_KEY", key)
            .env("HODEI_RECONFIG_PROXY", proxy.to_string())
            .status()
            .expect("the reconfigurer runs");
        assert!(status.success(), "the repository could not be reconfigured");
    }

    /// Reconfigures an already seeded repository: sets (or removes, with
    /// `null`) the namespace policy and raises its configuration
    /// version.
    ///
    /// It is done by a process that ends right away and with the service stopped: the
    /// database is locked by the process that opens it. It is the policy revision
    /// that an incompatible previous resolution has to respect.
    pub fn reconfigure_namespaces(&self, key: &str, namespaces: serde_json::Value) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_RECONFIG_KEY", key)
            .env("HODEI_RECONFIG_NAMESPACES", namespaces.to_string())
            .status()
            .expect("the reconfigurer runs");
        assert!(status.success(), "the namespaces could not be reconfigured");
    }

    /// Creates **group** repositories of the first account, in the given
    /// order (a nested group is declared after its own). Each one is
    /// a key and the ordered list of its members.
    ///
    /// It is seeded in a separate process and with the service stopped, like the
    /// rest: the database is locked by the process that opens it.
    pub fn seed_groups(&self, groups: &[(&str, &[&str])]) {
        let groups: Vec<serde_json::Value> = groups
            .iter()
            .map(|(key, members)| serde_json::json!({ "key": key, "members": members }))
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_GROUPS",
                serde_json::Value::Array(groups).to_string(),
            )
            .status()
            .expect("the group seeder runs");
        assert!(status.success(), "the groups could not be seeded");
    }

    /// Creates a **go group** of the first account: the key and the ordered
    /// list of its members.
    pub fn seed_go_group(&self, key: &str, members: &[&str]) {
        let groups = vec![serde_json::json!({
            "key": key,
            "members": members,
            "format": "go",
        })];
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_GROUPS",
                serde_json::Value::Array(groups).to_string(),
            )
            .status()
            .expect("the group seeder runs");
        assert!(status.success(), "the go group could not be seeded");
    }

    /// Creates a **nuget group** of the first account: the key and the ordered
    /// list of its members.
    pub fn seed_nuget_group(&self, key: &str, members: &[&str]) {
        let groups = vec![serde_json::json!({
            "key": key,
            "members": members,
            "format": "nuget",
        })];
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_GROUPS",
                serde_json::Value::Array(groups).to_string(),
            )
            .status()
            .expect("the group seeder runs");
        assert!(status.success(), "the nuget group could not be seeded");
    }

    /// Creates **Maven2 hosted** repositories of the first account. Each one
    /// is a key and its `config_settings.maven` (`null` if it has none).
    ///
    /// It is seeded in a separate process and with the service stopped: the database
    /// is locked by the process that opens it.
    pub fn seed_maven(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, maven)| serde_json::json!({ "key": key, "maven": maven }))
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_MAVEN",
                serde_json::Value::Array(repositories).to_string(),
            )
            .status()
            .expect("the maven seeder runs");
        assert!(
            status.success(),
            "the maven repositories could not be seeded"
        );
    }

    /// Creates **Maven2 proxy** repositories of the first account. Each one is
    /// a key, its `config_settings.maven` (`null` if it has none) and its
    /// `config_settings.proxy` (the upstream and its cache).
    pub fn seed_maven_proxies(
        &self,
        repositories: &[(&str, serde_json::Value, serde_json::Value)],
    ) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, maven, proxy)| {
                serde_json::json!({ "key": key, "maven": maven, "proxy": proxy })
            })
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_MAVEN",
                serde_json::Value::Array(repositories).to_string(),
            )
            .status()
            .expect("the maven proxy seeder runs");
        assert!(status.success(), "the maven proxies could not be seeded");
    }

    /// Creates **Maven2** groups of the first account, in the given order.
    pub fn seed_maven_groups(&self, groups: &[(&str, &[&str])]) {
        let groups: Vec<serde_json::Value> = groups
            .iter()
            .map(|(key, members)| {
                serde_json::json!({ "key": key, "members": members, "format": "maven2" })
            })
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_GROUPS",
                serde_json::Value::Array(groups).to_string(),
            )
            .status()
            .expect("the maven group seeder runs");
        assert!(status.success(), "the maven groups could not be seeded");
    }

    /// Creates **npm hosted** repositories of the first account. Each one is
    /// a key and its `config_settings.npm` (`null` if it has none).
    pub fn seed_npm(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, npm)| serde_json::json!({ "key": key, "npm": npm, "format": "npm" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the npm repositories");
    }

    /// Creates **cargo hosted** repositories of the first account. Each entry
    /// is a key and its config (`null` if it has none).
    pub fn seed_cargo(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, cargo)| serde_json::json!({ "key": key, "cargo": cargo, "format": "cargo" }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the cargo repositories");
    }

    /// Creates **conan hosted** repositories of the first account. Each one
    /// is a key and its config (`null` if it has none).
    pub fn seed_conan(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, conan)| serde_json::json!({ "key": key, "conan": conan, "format": "conan" }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the conan repositories");
    }

    /// Creates **galaxy hosted** repositories of the first account. Each one
    /// is a key and its config (`null` if it has none).
    /// Creates **terraform** repositories of the first account. Each
    /// entry is a key and its config (`null` if it has none); with the
    /// config it is created hosted (module registry) and with `proxy` proxy
    /// (provider mirror).
    pub fn seed_terraform(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, terraform)| {
                serde_json::json!({ "key": key, "terraform": terraform, "format": "terraform" })
            })
            .collect();
        self.run_the_seeder(
            "HODEI_SEED_MAVEN",
            repositories,
            "the terraform repositories",
        );
    }

    /// Creates **terraform proxy** repositories (provider mirror) of the
    /// first account: the key and its `config_settings.proxy`.
    pub fn seed_terraform_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| {
                serde_json::json!({ "key": key, "proxy": proxy, "format": "terraform" })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the terraform mirrors");
    }

    /// Like [`Self::seed_terraform_proxies`], with some of the mirrors in
    /// the **trusted** channel (`config_settings.trust.serve`).
    pub fn seed_terraform_proxies_trusted(
        &self,
        trusted: &[&str],
        repositories: &[(&str, serde_json::Value)],
    ) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| {
                serde_json::json!({
                    "key": key,
                    "proxy": proxy,
                    "format": "terraform",
                    "trusted": trusted.contains(key),
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the terraform mirrors");
    }

    /// Creates **hosted** repositories of an additional format (`conan`,
    /// `galaxy`, `terraform`) in the **trusted** channel: the config is that of the
    /// format (`null` if it has none).
    pub fn seed_format_trusted(&self, format: &str, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, config)| {
                serde_json::json!({
                    "key": key,
                    "format": format,
                    format: config,
                    "trusted": true,
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the trusted repositories");
    }

    pub fn seed_galaxy(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, galaxy)| serde_json::json!({ "key": key, "galaxy": galaxy, "format": "galaxy" }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the galaxy repositories");
    }

    /// Creates **hf hosted** repositories of the first account and seeds their
    /// models. Each asset is `(asset path, source file)`: the
    /// content is stored in the CAS and published in the catalog. The
    /// dialect's write is not declared (the hf surface is
    /// read-only), and that is why the content enters through the hub seeding.
    pub fn seed_hf(&self, repositories: &[(&str, Vec<(&str, std::path::PathBuf)>)]) {
        self.seed_hf_in(&[], repositories);
    }

    /// Like [`Self::seed_hf`], with the repositories in `trusted` in the
    /// **trusted** channel (`config_settings.trust.serve`).
    pub fn seed_hf_trusted(
        &self,
        trusted: &[&str],
        repositories: &[(&str, Vec<(&str, std::path::PathBuf)>)],
    ) {
        self.seed_hf_in(trusted, repositories);
    }

    fn seed_hf_in(
        &self,
        trusted: &[&str],
        repositories: &[(&str, Vec<(&str, std::path::PathBuf)>)],
    ) {
        let entries: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, assets)| {
                serde_json::json!({
                    "key": key,
                    "trusted": trusted.contains(key),
                    "assets": assets
                        .iter()
                        .map(|(asset, file)| {
                            serde_json::json!({
                                "asset": asset,
                                "file": file.to_string_lossy(),
                            })
                        })
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_SEED_CAS", &self.cas)
            .env(
                "HODEI_SEED_HF",
                serde_json::Value::Array(entries).to_string(),
            )
            .status()
            .expect("the seeder runs");
        assert!(status.success(), "the hf repositories could not be seeded");
    }

    /// Creates **cargo proxy** repositories of the first account: the key and
    /// its `config_settings.proxy` (the upstream and its cache).
    pub fn seed_cargo_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, proxy)| serde_json::json!({ "key": key, "proxy": proxy, "format": "cargo" }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the cargo proxies");
    }

    /// Creates **apt hosted** repositories of the first account. Each
    /// entry is a key and its config (`null` if it has none).
    pub fn seed_apt(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, apt)| serde_json::json!({ "key": key, "format": "apt", "apt": apt }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the apt repositories");
    }

    /// Creates **yum hosted** repositories of the first account. Each
    /// entry is a key and its config (`null` if it has none).
    pub fn seed_yum(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, yum)| serde_json::json!({ "key": key, "format": "rpm", "yum": yum }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the yum repositories");
    }

    /// Creates **yum proxy** repositories of the first account. Each one is
    /// a key, its `config_settings.proxy` (the upstream and its cache) and the
    /// trust channel it is seeded in.
    ///
    /// The channel is a repository setting (`config_settings.trust.serve`)
    /// and there is no HTTP route that sets it: it is set when seeding, as
    /// whoever operates the deployment would do. Those that are not in the trust
    /// list are seeded in the open channel.
    pub fn seed_yum_proxies(&self, trusted: &[&str], repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| {
                serde_json::json!({
                    "key": key,
                    "proxy": proxy,
                    "format": "rpm",
                    "trusted": trusted.contains(key),
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the yum proxies");
    }

    /// Seeds a **yum group** repository for the first account.
    pub fn seed_yum_group(&self, key: &str, members: &[&str]) {
        let groups = vec![serde_json::json!({
            "key": key,
            "members": members,
            "format": "rpm",
        })];
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_GROUPS",
                serde_json::Value::Array(groups).to_string(),
            )
            .status()
            .expect("the group seeder runs");
        assert!(status.success(), "the yum group could not be seeded");
    }

    /// Creates **go hosted** repositories of the first account. Each entry
    /// is a key and its config (`null` if it has none).
    pub fn seed_go(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, go)| serde_json::json!({ "key": key, "format": "go", "go": go }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the go repositories");
    }

    /// Creates **go hosted** repositories of the first account with the
    /// **partitioned content scope**.
    ///
    /// It exists for the case that measures the scope policy: this release only
    /// serves the global scope, and a member with a partitioned scope has to be
    /// declared with the scope diagnostic, not with a 404 which would be
    /// a lie.
    pub fn seed_go_partitioned(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, go)| {
                serde_json::json!({
                    "key": key,
                    "format": "go",
                    "go": go,
                    "scope": "partitioned",
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the go repositories");
    }

    /// Creates **go proxy** repositories of the first account: the key and its
    /// `config_settings.proxy` (the upstream and its cache).
    pub fn seed_go_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| serde_json::json!({ "key": key, "proxy": proxy, "format": "go" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the go proxies");
    }

    /// Creates **go proxy** repositories of the first account, some of them
    /// in the **trusted** channel.
    ///
    /// The channel is a repository setting (`config_settings.trust.serve`)
    /// and there is no HTTP route that sets it: it is set when seeding, as
    /// whoever operates the deployment would do. Those that are not in the trust
    /// list are seeded in the open channel.
    pub fn seed_go_proxies_trusted(
        &self,
        trusted: &[&str],
        repositories: &[(&str, serde_json::Value)],
    ) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| {
                serde_json::json!({
                    "key": key,
                    "proxy": proxy,
                    "format": "go",
                    "trusted": trusted.contains(key),
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the go proxies");
    }

    /// Creates **nuget hosted** repositories of the first account. Each
    /// entry is a key and its config (`null` if it has none).
    pub fn seed_nuget(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, nuget)| serde_json::json!({ "key": key, "format": "nuget", "nuget": nuget }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the nuget repositories");
    }

    /// Creates **nuget proxy** repositories of the first account: the key and
    /// its `config_settings.proxy` (the upstream and its cache).
    pub fn seed_nuget_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, proxy)| serde_json::json!({ "key": key, "proxy": proxy, "format": "nuget" }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the nuget proxies");
    }

    /// Creates **helm** repositories of the first account, served through the
    /// OCI channel. Each entry is a key and its config (`null` if it has none).
    pub fn seed_helm(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, helm)| serde_json::json!({ "key": key, "format": "helm", "helm": helm }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the helm repositories");
    }

    /// Creates **npm proxy** repositories of the first account: the key and its
    /// `config_settings.proxy` (the upstream and its cache).
    pub fn seed_npm_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| serde_json::json!({ "key": key, "proxy": proxy, "format": "npm" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the npm proxies");
    }

    /// Creates **PyPI hosted** repositories of the first account. Each one is
    /// a key and its `config_settings.pypi` (`null` if it has none).
    pub fn seed_pypi(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, pypi)| serde_json::json!({ "key": key, "pypi": pypi, "format": "pypi" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the pypi repositories");
    }

    /// Creates **PyPI hosted** repositories of the first account, some of
    /// them in the **trusted** channel.
    ///
    /// The channel is a repository setting (`config_settings.trust.serve`)
    /// and there is no HTTP route that sets it: it is set when seeding, as
    /// whoever operates the deployment would do.
    pub fn seed_pypi_trusted(&self, trusted: &[&str], repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, pypi)| {
                serde_json::json!({
                    "key": key,
                    "pypi": pypi,
                    "format": "pypi",
                    "trusted": trusted.contains(key),
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the pypi repositories");
    }

    /// Creates **PyPI proxy** repositories of the first account: the key and its
    /// `config_settings.proxy` (the upstream and its cache).
    pub fn seed_pypi_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| serde_json::json!({ "key": key, "proxy": proxy, "format": "pypi" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the pypi proxies");
    }

    /// Creates **PyPI** groups of the first account, in the given order.
    pub fn seed_pypi_groups(&self, groups: &[(&str, &[&str])]) {
        let groups: Vec<serde_json::Value> = groups
            .iter()
            .map(|(key, members)| {
                serde_json::json!({ "key": key, "members": members, "format": "pypi" })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_GROUPS", groups, "the pypi groups");
    }

    /// Creates **npm** groups of the first account, in the given order.
    pub fn seed_npm_groups(&self, groups: &[(&str, &[&str])]) {
        let groups: Vec<serde_json::Value> = groups
            .iter()
            .map(|(key, members)| {
                serde_json::json!({ "key": key, "members": members, "format": "npm" })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_GROUPS", groups, "the npm groups");
    }

    fn run_the_seeder(&self, variable: &str, entries: Vec<serde_json::Value>, what: &str) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(variable, serde_json::Value::Array(entries).to_string())
            .status()
            .expect("the seeder runs");
        assert!(status.success(), "{what} could not be seeded");
    }

    /// Seeds repositories of the first account with free-form settings.
    ///
    /// Each entry is the JSON that `seed_the_maven` understands: `key`,
    /// `format` (`raw`, `npm`, `pypi`, `maven2`...), `proxy` (if it is a proxy),
    /// `namespaces` (the reservation policy, if it declares one), `scope` (the
    /// content scope: `partitioned` if it declares it) and the format's own
    /// settings (`maven`, `npm`, `pypi`). It is the door used by
    /// the suites that mix format and reservation without multiplying seeders.
    pub fn seed_repositories(&self, entries: &[serde_json::Value]) {
        self.run_the_seeder(
            "HODEI_SEED_MAVEN",
            entries.to_vec(),
            "the free-form repositories",
        );
    }

    /// Seeds groups of the first account from free-form entries:
    /// `key`, `members` and the `namespaces` they declare.
    pub fn seed_groups_raw(&self, groups: &[serde_json::Value]) {
        self.run_the_seeder("HODEI_SEED_GROUPS", groups.to_vec(), "the free-form groups");
    }

    /// Changes the members of an already seeded group: the version of its
    /// configuration goes up.
    pub fn reconfigure_group(&self, key: &str, members: &[&str]) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_RECONFIG_GROUP", key)
            .env(
                "HODEI_RECONFIG_MEMBERS",
                serde_json::json!(members).to_string(),
            )
            .status()
            .expect("the group reconfigurer runs");
        assert!(status.success(), "the group could not be reconfigured");
    }

    /// Replaces the Cedar policy of the deployment. It has to be done before
    /// starting the service.
    pub fn write_policy(&self, policy: &str) {
        std::fs::write(self.policy_path(), policy).expect("the policy is written");
    }

    /// Starts the service and waits for it to open the port.
    ///
    /// The port is chosen on each attempt, and not beforehand: a free port
    /// can be taken by another process between being chosen and the
    /// service occupying it, and in that case the harness would end up talking to
    /// the service of another deployment -- a green suite that did not measure
    /// what it says it measures --.
    ///
    /// What removes that doubt is the **log**: the service says which
    /// port it listened on, and says it **after** having occupied it. If another
    /// process held the port, this service cannot write that
    /// line, so the harness does not take it as its own and retries with another.
    ///
    /// The service may also die on startup when the disk engine
    /// still holds the directory lock, so an attempt
    /// that dies before opening the port is repeated: what is tested is
    /// the service, not the lock race.
    pub fn start(&self, extra_environment: &[(&str, &str)]) -> Service {
        for _attempt in 0..5 {
            if let Some(service) = self.try_start(free_port(), extra_environment) {
                return service;
            }
        }
        panic!(
            "the service did not open the port; the log says:\n{}",
            std::fs::read_to_string(self.log_path()).unwrap_or_default()
        );
    }

    /// Starts the service on a specific port.
    ///
    /// The restart needs the **same** port: the client already knows where
    /// it is looking, and with another port another service would be measured -- a newly
    /// born one -- and not what is meant to be measured -- the same service, again.
    ///
    /// The attempts are repeated on the same port, like those of
    /// [`Self::start`]: the disk engine may take time to release the
    /// directory lock, and a restart that gives up at the first
    /// attempt would measure a startup failure for the startup.
    pub fn start_on(&self, port: u16, extra_environment: &[(&str, &str)]) -> Service {
        for _attempt in 0..5 {
            if let Some(service) = self.try_start(port, extra_environment) {
                return service;
            }
        }
        panic!(
            "the service did not open the port {port}; the log says:\n{}",
            std::fs::read_to_string(self.log_path()).unwrap_or_default()
        )
    }

    /// An attempt to start the service on a port.
    fn try_start(&self, port: u16, extra_environment: &[(&str, &str)]) -> Option<Service> {
        let log = self.log_path();
        let stderr = File::create(&log).expect("the log of the service");
        let stdout = stderr.try_clone().expect("the log of the service, twice");
        let mut command = Command::new(BIN);
        command
            .env_remove("RUN_MODE")
            .env_remove("HODEI_SECURITY__AUTH_JWKS_PATH")
            .env("HODEI_SERVER__HOST", "127.0.0.1")
            .env("HODEI_SERVER__PORT", port.to_string())
            .env("HODEI_CONTENT__CAS_PATH", &self.cas)
            .env("HODEI_REPOSITORY__DURABLE", "true")
            .env("HODEI_SECURITY__RAW_API_ENABLED", "true")
            .env("HODEI_SECURITY__OCI_API_ENABLED", "true")
            .env(
                "HODEI_SECURITY__AUTH_JWT_SECRET",
                String::from_utf8(SECRET.to_vec()).unwrap(),
            )
            .env("HODEI_SECURITY__AUTH_ISSUER", ISSUER)
            .env("HODEI_SECURITY__AUTH_AUDIENCE", AUDIENCE)
            .env(
                "HODEI_SECURITY__AUTHORIZATION_POLICY_PATH",
                self.policy_path(),
            )
            .env(
                "HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH",
                self.schema_path(),
            )
            .env("RUST_LOG", "info");
        // The graph profile: the production binary on `RUN_MODE=dgraph`
        // against the tenant of this deployment.
        if let Some(tenant) = &self.graph {
            for (name, value) in tenant.service_environment() {
                command.env(name, value);
            }
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        for (name, value) in extra_environment {
            command.env(name, value);
        }
        let mut child = command.spawn().expect("the service starts");

        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            // The port is stated by the service, and it only states it if it has
            // occupied it: that is the proof that the child owns the
            // port.
            if the_bound_port(&log) == Some(port)
                && TcpStream::connect(("127.0.0.1", port)).is_ok()
                && matches!(child.try_wait(), Ok(None))
            {
                return Some(Service { port, child });
            }
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        std::thread::sleep(Duration::from_millis(500));
        None
    }

    /// The path of the service log.
    pub fn log_path(&self) -> PathBuf {
        self.directory.path().join("service.log")
    }

    /// The service log, to read it when something fails.
    pub fn the_log(&self) -> String {
        std::fs::read_to_string(self.log_path()).unwrap_or_default()
    }
}

/// The service that is up.
pub struct Service {
    /// The port it listens on.
    pub port: u16,
    /// The process.
    pub child: Child,
}

impl Drop for Service {
    /// The service does not stay alive if the test falls over.
    ///
    /// Without this, a test that fails before stopping the service leaves
    /// a process listening on its port: the next test may
    /// talk to an old service, and the suite goes green without having
    /// tested what it says it tests.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Stops the service.
pub fn stop(service: &mut Service) {
    let _ = service.child.kill();
    let _ = service.child.wait();
}

/// Stops the service, abruptly.
///
/// A `SIGKILL` is what happens when a machine is switched off: there is no
/// chance to clean anything up, and that is why it is the one that reproduces
/// a real interruption.
pub fn kill(service: &mut Service) {
    let _ = service.child.kill();
    let _ = service.child.wait();
}

/// The metadata of a distribution, as `twine` reads it.
pub fn the_metadata(name: &str, version: &str, requires_python: Option<&str>) -> String {
    let mut metadata = format!(
        "Metadata-Version: 2.1\n\
         Name: {name}\n\
         Version: {version}\n\
         Summary: a distribution for the PyPI suite\n"
    );
    if let Some(requires_python) = requires_python {
        metadata.push_str(&format!("Requires-Python: {requires_python}\n"));
    }
    metadata.push('\n');
    metadata
}

/// The name carried by the file of a distribution.
///
/// PEP 427: in the file name, the project name is written with
/// `_` instead of `-`.
pub fn the_escaped_name(name: &str) -> String {
    name.replace(['-', '.'], "_")
}

/// A wheel `RECORD`: the files, their hash and their size.
fn the_record(entries: &[(&str, &[u8])]) -> String {
    let mut record = String::new();
    for (path, body) in entries {
        let digest = sha2::Sha256::digest(body);
        record.push_str(&format!(
            "{path},sha256={},{}\n",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest),
            body.len()
        ));
    }
    record
}

/// A wheel: a zip with the metadata, the `RECORD` and the package module.
pub fn a_wheel(name: &str, version: &str, requires_python: Option<&str>) -> Vec<u8> {
    the_wheel(name, version, requires_python, 0)
}

/// A wheel with a filler member.
///
/// A real distribution weighs more than two megabytes, and the limit that
/// the web server brings by default is two: one that passes it is needed.
pub fn a_big_wheel(
    name: &str,
    version: &str,
    requires_python: Option<&str>,
    filler: usize,
) -> Vec<u8> {
    the_wheel(name, version, requires_python, filler)
}

/// The usual wheel, with a filler member if requested.
fn the_wheel(name: &str, version: &str, requires_python: Option<&str>, filler: usize) -> Vec<u8> {
    use std::io::Write;
    let escaped = the_escaped_name(name);
    let dist = format!("{escaped}-{version}.dist-info");
    let metadata = the_metadata(name, version, requires_python);
    let wheel = "Wheel-Version: 1.0\n\
                 Generator: hodei-tests\n\
                 Root-Is-Purelib: true\n\
                 Tag: py3-none-any\n";
    let module = format!("\"\"\"{name}.\"\"\"\n\nVALUE = 42\n");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut entries: Vec<(String, Vec<u8>)> = vec![
        (format!("{dist}/METADATA"), metadata.into_bytes()),
        (format!("{dist}/WHEEL"), wheel.as_bytes().to_vec()),
        (format!("{name}/__init__.py"), module.into_bytes()),
    ];
    if filler > 0 {
        entries.push((format!("{name}/data.bin"), vec![b'x'; filler]));
    }
    for (path, body) in &entries {
        writer
            .start_file(path.clone(), options)
            .expect("the file of the wheel starts");
        writer
            .write_all(body)
            .expect("the body of the file is written");
    }
    let record = the_record(
        &entries
            .iter()
            .map(|(path, body)| (path.as_str(), body.as_slice()))
            .collect::<Vec<_>>(),
    );
    writer
        .start_file(format!("{dist}/RECORD"), options)
        .expect("the record of the wheel starts");
    writer
        .write_all(record.as_bytes())
        .expect("the record is written");
    writer.finish().expect("the wheel is closed").into_inner()
}

/// An sdist: a gzipped tar with the `PKG-INFO` and the `pyproject.toml`.
pub fn an_sdist(name: &str, version: &str, requires_python: Option<&str>) -> Vec<u8> {
    use std::io::Write;
    let root = format!("{name}-{version}");
    let metadata = the_metadata(name, version, requires_python);
    let pyproject = format!(
        "[build-system]\n\
         requires = [\"setuptools>=61\"]\n\
         build-backend = \"setuptools.build_meta\"\n\
         \n\
         [project]\n\
         name = \"{name}\"\n\
         version = \"{version}\"\n"
    );
    let module = format!("\"\"\"{name}.\"\"\"\n\nVALUE = 42\n");
    let entries: Vec<(String, &[u8])> = vec![
        (format!("{root}/PKG-INFO"), metadata.as_bytes()),
        (format!("{root}/pyproject.toml"), pyproject.as_bytes()),
        (format!("{root}/{name}/__init__.py"), module.as_bytes()),
    ];
    let mut tar: Vec<u8> = Vec::new();
    for (path, body) in entries {
        tar.extend_from_slice(&the_tar_header(&path, body.len() as u64));
        tar.extend_from_slice(body);
        let padding = (512 - body.len() % 512) % 512;
        tar.extend(std::iter::repeat_n(0u8, padding));
    }
    // Two blocks of zeros: the end of the archive.
    tar.extend(std::iter::repeat_n(0u8, 1024));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).expect("the tar is compressed");
    encoder.finish().expect("the sdist is closed")
}

/// The header of a tar file.
fn the_tar_header(path: &str, size: u64) -> [u8; 512] {
    let mut header = [0u8; 512];
    header[..path.len()].copy_from_slice(path.as_bytes());
    let write = |header: &mut [u8; 512], at: usize, text: &str| {
        header[at..at + text.len()].copy_from_slice(text.as_bytes());
    };
    write(&mut header, 100, "000644 ");
    write(&mut header, 108, "000000 ");
    write(&mut header, 116, "000000 ");
    write(&mut header, 124, &format!("{size:011o} "));
    write(&mut header, 136, &format!("{:011o} ", 0));
    // The checksum is computed with the field blank.
    header[148..156].fill(b' ');
    header[156] = b'0';
    write(&mut header, 257, "ustar");
    header[263] = 0;
    write(&mut header, 265, "00");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    write(&mut header, 148, &format!("{checksum:06o} "));
    header[154] = 0;
    header
}

/// The `sha256sum` command that checks the digest of a file.
///
/// It is the same program and the same file that the harness runs: what
/// is written in the report is what was run, not a reconstruction.
pub fn sha256_command(file: &Path) -> String {
    let root = file
        .parent()
        .map(|root| root.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = if root.is_empty() {
        file.display().to_string()
    } else {
        file.display().to_string().replace(&root, "<despliegue>")
    };
    format!("sha256sum {name}")
}

/// The digest of a file, as stated by `sha256sum`.
///
/// The comparison is not made by the code under test: `sha256sum` is a
/// program that has nothing to do with this service, and it is the one that says
/// whether the bytes are the same.
pub fn sha256_of(file: &Path) -> String {
    let output = Command::new("sha256sum")
        .arg(file)
        .output()
        .expect("sha256sum has to be there");
    assert!(output.status.success(), "sha256sum failed on {file:?}");
    String::from_utf8(output.stdout)
        .expect("the digest is text")
        .split_whitespace()
        .next()
        .expect("a digest")
        .to_string()
}

/// The response of a `curl`.
pub struct Answer {
    /// The status code.
    pub status: u16,
    /// The bytes of the body that `curl` wrote.
    ///
    /// It is stated by `curl` itself, which knows how many bytes it wrote to the
    /// destination: it is the way to measure a bodiless `HEAD` without assuming it.
    pub size: u64,
    /// The headers, as text.
    pub headers: String,
    /// The body that `curl` wrote, if it wrote it to a file.
    ///
    /// It is what the server says, byte by byte: without this, a test can only
    /// assert the code and not **which** bytes it served.
    pub body: Vec<u8>,
}

impl Answer {
    /// A response header.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.trim().eq_ignore_ascii_case(name) {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
    }
}

/// A `curl` with the arguments it is run with.
///
/// The command that is run and the command that is written in the report
/// come from the **same** argument list. A summarized command is a
/// paraphrase, and a paraphrase is an opinion: what was run and what
/// is written are the same thing.
///
/// The credential goes **redacted** in what is written: the report says
/// who was talked to, not what they authenticated with. A report is archived and
/// shared, and a token in a file is a credential in a
/// file.
pub struct Curl {
    /// The arguments, in the order in which they are passed to `curl`.
    args: Vec<String>,
    /// The real credential, which is redacted when written.
    token: String,
    /// The deployment directory, which is abbreviated when written.
    ///
    /// The deployment paths are in a temporary directory that
    /// does not exist six months from now. Writing the whole path is
    /// writing noise; writing `<deployment>/file` says the same and
    /// can be read.
    root: Option<PathBuf>,
    /// Where `curl` writes the body, if it writes it to a file.
    ///
    /// Without this, a response only says the code: it cannot be asserted
    /// **which** bytes the server served, only that it said something.
    body: Option<PathBuf>,
}

impl Curl {
    /// The arguments that all requests share.
    ///
    /// The path goes as is (`--path-as-is`), the credential goes if there
    /// is one, the headers are dumped and `curl` counts the code and the
    /// bytes it wrote: the code and the size are stated by `curl`, not
    /// by the code under test.
    fn base(port: u16, path: &str, token: &str) -> Self {
        let mut args = vec![
            "--silent".to_string(),
            "--show-error".to_string(),
            "--path-as-is".to_string(),
        ];
        if !token.is_empty() {
            args.push("--header".to_string());
            args.push(format!("Authorization: Bearer {token}"));
        }
        args.push("--dump-header".to_string());
        args.push("-".to_string());
        args.push("--write-out".to_string());
        args.push("%{http_code} %{size_download}".to_string());
        args.push(format!("http://127.0.0.1:{port}{path}"));
        Self {
            args,
            token: token.to_string(),
            root: None,
            body: None,
        }
    }

    /// A `curl` that publishes a file.
    ///
    /// With `--limit-rate` the body arrives slowly, which is what is
    /// needed to be able to kill the service **halfway through the upload**.
    pub fn upload(
        port: u16,
        path: &str,
        token: &str,
        file: &Path,
        limit_rate: Option<&str>,
    ) -> Self {
        let mut curl = Self::base(port, path, token);
        curl.args.push("--request".to_string());
        curl.args.push("PUT".to_string());
        curl.args.push("--data-binary".to_string());
        curl.args.push(format!("@{}", file.display()));
        if let Some(rate) = limit_rate {
            curl.args.push("--limit-rate".to_string());
            curl.args.push(rate.to_string());
        }
        curl.args.push("--output".to_string());
        curl.args.push("/dev/null".to_string());
        curl.root = file.parent().map(Path::to_path_buf);
        curl
    }

    /// A `curl` that downloads to a file, or to nothing.
    pub fn download(port: u16, path: &str, token: &str, into: Option<&Path>) -> Self {
        let mut curl = Self::base(port, path, token);
        curl.args.push("--output".to_string());
        curl.args.push(match into {
            Some(file) => file.display().to_string(),
            None => "/dev/null".to_string(),
        });
        curl.root = into.and_then(|file| file.parent()).map(Path::to_path_buf);
        curl.body = into.map(Path::to_path_buf);
        curl
    }

    /// A `curl` that only asks for the description.
    pub fn describe(port: u16, path: &str, token: &str) -> Self {
        let mut curl = Self::base(port, path, token);
        curl.args.push("--head".to_string());
        curl.args.push("--output".to_string());
        curl.args.push("/dev/null".to_string());
        curl
    }

    /// A `curl` with an extra header, which leaves the body in a
    /// file.
    ///
    /// It is what allows checking the **bytes** of a range, and not only its
    /// code and its `Content-Range`.
    pub fn with_header(
        port: u16,
        path: &str,
        token: &str,
        header: (&str, &str),
        into: &Path,
    ) -> Self {
        let mut curl = Self::base(port, path, token);
        curl.args.push("--header".to_string());
        curl.args.push(format!("{}: {}", header.0, header.1));
        curl.args.push("--output".to_string());
        curl.args.push(into.display().to_string());
        curl.root = into.parent().map(Path::to_path_buf);
        curl
    }

    /// A `curl` with the method, the body and the content type that
    /// are needed.
    ///
    /// The push protocol is the one that uses the most methods: `POST` opens the
    /// session, `PATCH` puts in the chunk, `PUT` closes it and `DELETE`
    /// cancels it. Each one with its own body, and the manifest with its
    /// own content type.
    pub fn call(
        port: u16,
        method: &str,
        path: &str,
        token: &str,
        body: Option<&Path>,
        content_type: Option<&str>,
        into: Option<&Path>,
    ) -> Self {
        match content_type {
            Some(content_type) => Self::call_with_headers(
                port,
                method,
                path,
                token,
                body,
                &[("content-type", content_type)],
                into,
            ),
            None => Self::call_with_headers(port, method, path, token, body, &[], into),
        }
    }

    /// A `curl` with the method, the body and the extra headers that
    /// are needed.
    ///
    /// The push protocol's `Content-Range` is one more header, and
    /// without it it cannot be proven that a chunk that does not fit receives the
    /// error the protocol states. The content type is also a
    /// header, and that is why it travels in the same list.
    pub fn call_with_headers(
        port: u16,
        method: &str,
        path: &str,
        token: &str,
        body: Option<&Path>,
        extra: &[(&str, &str)],
        into: Option<&Path>,
    ) -> Self {
        let mut curl = Self::base(port, path, token);
        if method != "GET" {
            // `--request HEAD` makes curl wait for a body that HEAD
            // never brings: the description mode is `--head`.
            if method == "HEAD" {
                curl.args.push("--head".to_string());
            } else {
                curl.args.push("--request".to_string());
                curl.args.push(method.to_string());
            }
        }
        if let Some(file) = body {
            curl.args.push("--data-binary".to_string());
            curl.args.push(format!("@{}", file.display()));
        }
        for (name, value) in extra {
            curl.args.push("--header".to_string());
            curl.args.push(format!("{name}: {value}"));
        }
        curl.args.push("--output".to_string());
        curl.args.push(match into {
            // With `--head` curl writes the headers to the destination:
            // HEAD has no body, and the harness requires it empty.
            Some(_) if method == "HEAD" => "/dev/null".to_string(),
            Some(file) => file.display().to_string(),
            None => "/dev/null".to_string(),
        });
        curl.root = into
            .or(body)
            .and_then(|file| file.parent())
            .map(Path::to_path_buf);
        // HEAD writes no body: if we pointed the destination, the read
        // would find the leftovers of another run's file number.
        curl.body = if method == "HEAD" {
            None
        } else {
            into.map(Path::to_path_buf)
        };
        curl
    }

    /// The process that is run.
    fn command(&self) -> Command {
        let mut command = Command::new("curl");
        command.args(&self.args);
        command
    }

    /// The command as it is written in the report.
    pub fn shown(&self) -> String {
        let root = self
            .root
            .as_ref()
            .map(|root| root.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut text = String::from("curl");
        for argument in &self.args {
            text.push(' ');
            let argument = if root.is_empty() {
                argument.clone()
            } else {
                argument.replace(&root, "<despliegue>")
            };
            if argument.contains(' ') {
                text.push('\'');
                text.push_str(&argument);
                text.push('\'');
            } else {
                text.push_str(&argument);
            }
        }
        if !self.token.is_empty() {
            text = text.replace(&self.token, &the_name_of(&self.token));
        }
        text
    }

    /// Runs `curl` and returns its response.
    pub fn run(&self) -> Answer {
        let mut command = self.command();
        the_answer(&mut command, self.body.as_deref())
    }

    /// Launches `curl` in the background.
    ///
    /// The upload stays in progress while the harness does something else: it is
    /// what allows killing the service **halfway through an upload**, which
    /// is the way to look at a real cut.
    pub fn run_in_background(&self) -> Child {
        let mut command = self.command();
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.spawn().expect("the upload starts")
    }
}

/// A `curl` that publishes a file.
///
/// With `--limit-rate` the body arrives slowly, which is what is needed
/// to be able to kill the service **halfway through the upload**.
pub fn curl_upload(
    port: u16,
    path: &str,
    token: &str,
    file: &Path,
    limit_rate: Option<&str>,
) -> Answer {
    Curl::upload(port, path, token, file, limit_rate).run()
}

/// A `curl` that downloads to a file, or to nothing.
pub fn curl_download(port: u16, path: &str, token: &str, into: Option<&Path>) -> Answer {
    Curl::download(port, path, token, into).run()
}

/// A `curl` that only asks for the description.
pub fn curl_describe(port: u16, path: &str, token: &str) -> Answer {
    Curl::describe(port, path, token).run()
}

/// A `curl` with an extra header, which leaves the body in a file.
pub fn curl_get_with_header(
    port: u16,
    path: &str,
    token: &str,
    header: (&str, &str),
    into: &Path,
) -> Answer {
    Curl::with_header(port, path, token, header, into).run()
}

/// A `curl` that publishes a file, in the background.
pub fn curl_upload_in_background(
    port: u16,
    path: &str,
    token: &str,
    file: &Path,
    limit_rate: Option<&str>,
) -> Child {
    Curl::upload(port, path, token, file, limit_rate).run_in_background()
}

/// The response of a `curl` that was launched in the background.
///
/// A `curl` that could not talk to the service has no code: that is
/// a test failure, and that is why the code is `0` and not an
/// invented number.
pub fn wait_for_the_answer(child: Child) -> Answer {
    let output = child.wait_with_output().expect("the curl finishes");
    assert!(
        output.status.success(),
        "the curl failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let (headers, status, size) = the_answer_of(&stdout);
    Answer {
        status,
        size,
        headers,
        body: Vec::new(),
    }
}

/// The counts that the startup states about the stagings.
///
/// The startup cleans what has expired and respects what is alive, and says so: the
/// number of both. A count that only exists in the code is not
/// evidence of anything; the service log is.
pub fn the_boot_cleanup(log: &str) -> (Option<u64>, Option<u64>) {
    let plain = without_the_colors(log);
    let mut removed = None;
    let mut alive = None;
    for line in plain.lines() {
        let Some((_, fields)) = line.split_once("at the boot") else {
            continue;
        };
        if removed.is_none() {
            removed = a_number_after(fields, "removed");
        }
        if alive.is_none() {
            alive = a_number_after(fields, "alive");
        }
    }
    (removed, alive)
}

/// The number that a field states in a log line.
///
/// The log format belongs to the service, and changes; what does not change is
/// that the number comes after the field name.
fn a_number_after(text: &str, field: &str) -> Option<u64> {
    let mut rest = text;
    while let Some(position) = rest.find(field) {
        let after = &rest[position + field.len()..];
        let digits: String = after
            .trim_start()
            .trim_start_matches(['=', ':'])
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
        rest = &rest[position + field.len()..];
    }
    None
}

/// The output of a `curl`, checking that `curl` finished well.
///
/// A `curl` that fails has no response: its code is an error, not an
/// HTTP status. Looking at the exit status is what separates "the service
/// said 404" from "there was no service".
fn the_output(command: &mut Command) -> String {
    let output = command.output().expect("curl has to be there");
    assert!(
        output.status.success(),
        "the curl failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The response of a `curl` that has run.
fn the_answer(command: &mut Command, body: Option<&Path>) -> Answer {
    let stdout = the_output(command);
    let (headers, status, size) = the_answer_of(&stdout);
    Answer {
        status,
        size,
        headers,
        body: body
            .and_then(|path| std::fs::read(path).ok())
            .unwrap_or_default(),
    }
}

/// The headers, the code and the size of the output of a `curl`.
///
/// `curl` writes the headers and then, at the end, the code and the
/// number of bytes it wrote to the destination.
fn the_answer_of(stdout: &str) -> (String, u16, u64) {
    let mut headers = String::new();
    let mut status = 0;
    let mut size = 0;
    for line in stdout.lines() {
        let trimmed = line.trim();
        let mut parts = trimmed.split_whitespace();
        let (code, bytes) = match (parts.next(), parts.next()) {
            (Some(code), Some(bytes))
                if !code.is_empty()
                    && !bytes.is_empty()
                    && code.chars().all(|character| character.is_ascii_digit())
                    && bytes.chars().all(|character| character.is_ascii_digit()) =>
            {
                (code, bytes)
            }
            _ => {
                headers.push_str(line);
                headers.push('\n');
                continue;
            }
        };
        status = code.parse().unwrap_or(0);
        size = bytes.parse().unwrap_or(0);
    }
    (headers, status, size)
}

/// A case of the report.
#[derive(Clone)]
pub struct Case {
    /// The case identifier.
    pub id: String,
    /// What was given.
    pub given: String,
    /// What was done.
    pub when: String,
    /// The command that did it.
    pub command: String,
    /// The real result.
    pub outcome: String,
    /// Whether the case passed.
    pub passed: bool,
}

/// The conformance report.
///
/// The cases are written to a file, not only to the log: a report that
/// only exists in the output of a CI is lost as soon as the CI is
/// cleaned.
///
/// Each surface has **its own** report, with its title and its notice of what it
/// does not certify: a report titled "RAW conformance" that
/// certifies OCI cases is a report that does not tell the truth, and whoever reads it
/// cannot know what it measures.
pub struct Report {
    /// The title of the report.
    title: &'static str,
    /// The notice of what this report does **not** certify.
    disclaimer: &'static str,
    /// Where the report is written.
    path: PathBuf,
    /// The tools that have talked to the service.
    tools: String,
    cases: Vec<Case>,
}

/// What the raw surface report does **not** certify.
const RAW_DISCLAIMER: &str = "\
- **Ni compatibilidad OCI.** Nada de lo que se ha ejecutado habla el protocolo de OCI Distribution: no hay manifiestos, ni `HEAD` de tags, ni `GET` de blobs por digest con la semántica de OCI. La superficie es `raw`.\n\
- **Ni compatibilidad S3.** No hay `ListBuckets`, ni `PutObject` con firma AWS, ni nada que se parezca a la API de S3.\n\
- **Ni disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea: lo que se ha medido es que los bytes vuelven intactos.\n\
- **Ni la recolección de blobs sin referencia.** Un blob sin referenciar se puede quedar en el almacén; lo que se ha comprobado es que no se publica solo.";

/// What the OCI push report does **not** certify.
const OCI_PUSH_DISCLAIMER: &str = "\
- **Ni compatibilidad OCI completa.** Lo que se ha ejecutado es el corte de **push**: las sesiones de subida y el manifiesto. El indice multi-plataforma, los referrers y las firmas son de otras tareas, y no se pueden certificar con lo que aqui se ha medido.\n\
- **Ni `oras` ni el `docker` de la CLI.** Los clientes reales que han hablado con el registro son `skopeo` y `podman`: son los que estan instalados en este entorno.\n\
- **Ni la promoción del canal de confianza.** Lo que se ha comprobado es que el artefacto admitido queda en **cuarentena**; promoverlo es de otra tarea.\n\
- **Ni disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea: lo que se ha medido es que los bytes vuelven intactos.\n\
- **Ni la recolección de blobs sin referencia.** Un blob sin referenciar se puede quedar en el almacén; lo que se ha comprobado es que no se publica solo.";

/// What the index and referrers report does **not** certify.
const OCI_INDEX_DISCLAIMER: &str = "\
- **Ni la validacion criptografica de las firmas.** Un referrer de firma se registra como lo que es: un manifiesto que dice que cuelga de otro. El registro **no** comprueba firmas, y la presencia de una no concede nada: la decision la toma el motor de confianza.\n\
- **Ni la promocion de nada.** Lo que se ha comprobado es que un referrer **no** cambia el canal de confianza de aquello a lo que cuelga: sigue en cuarentena, y promoverla es de otra tarea.\n\
- **Ni compatibilidad OCI completa.** Lo que se ha ejecutado es el corte de **consulta y adjuntos**: la lista de etiquetas, los referrers, el indice multi-plataforma y el manifiesto de artefacto. Las sesiones de subida son de otra tarea.\n\
- **Ni `docker` de la CLI.** Los clientes reales que han hablado con el registro son `oras`.\n\
- **Ni la suite oficial de conformidad.** Este informe dice lo que se ha ejecutado aqui, no lo que dice la suite del `distribution-spec`.\n\
- **Ni disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea.";

/// What the real-client compatibility report does **not**
/// certify.
///
/// This report is the one that measures **which clients speak the protocol**, and its
/// value lies in their being the real ones. Its limit lies in what real
/// clients do not do, and in what the registry does not do.
const OCI_E2E_DISCLAIMER: &str = "\
- **Ni la conformidad de las categorias que no se han ejecutado.** El informe nombra las categorias de la suite oficial que se han ejecutado, con su resultado. De las demas no se dice nada: una categoria que no se ha ejecutado no es una categoria conforme, y decir lo contrario seria propaganda.\n\
- **Ni validacion criptografica de las firmas.** Un referrer de firma se registra como lo que es: un manifiesto que dice que cuelga de otro. El registro **no** comprueba firmas, y la presencia de una no concede nada: la decision la toma el motor de confianza.\n\
- **Ni la promocion de nada.** Lo que se ha comprobado es que un referrer **no** cambia el canal de confianza de aquello a lo que cuelga: sigue en cuarentena, y promoverla es de otra tarea.\n\
- **Ni que el cliente de Docker y el motor sean la misma cosa.** El `docker` de la CLI es el cliente real y el motor que hay detras es el `podman` exponiendo la API de Docker: la CLI de Docker no habla con un registro, habla con un demonio.\n\
- **Ni el borrado de contenido.** El protocolo lo tiene (borrar un blob, un manifiesto, una etiqueta) y este registro no lo implementa.\n\
- **Ni disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea.";

impl Report {
    /// An empty report, the raw surface one.
    pub fn new() -> Self {
        Self {
            title: "Conformidad RAW — informe",
            disclaimer: RAW_DISCLAIMER,
            path: Self::the_path(),
            tools: the_tools(),
            cases: Vec::new(),
        }
    }

    /// An empty report, the OCI push one.
    ///
    /// It is a separate report, with its title and its notice: the raw
    /// surface report is read by the RAW conformance gates, and an
    /// OCI case in it says nothing of what the gate believes.
    pub fn for_oci_push() -> Self {
        Self {
            title: "Conformidad OCI push — informe",
            disclaimer: OCI_PUSH_DISCLAIMER,
            path: Self::the_oci_push_path(),
            // The client versions go in the report: a client
            // without a version is a client of which it cannot be said that
            // it speaks the protocol.
            tools: format!(
                "{}; `{}`; `{}`",
                the_tools(),
                the_first_line_of(&["skopeo", "--version"]),
                the_first_line_of(&["podman", "--version"])
            ),
            cases: Vec::new(),
        }
    }

    /// An empty report, the index and referrers one.
    ///
    /// It is a separate report, with its title and its notice: one that certifies
    /// the push and the index under the same title would say the registry
    /// complies with the whole protocol, and what has been measured is a part.
    pub fn for_oci_index() -> Self {
        Self {
            title: "Conformidad OCI indice, etiquetas y referrers — informe",
            disclaimer: OCI_INDEX_DISCLAIMER,
            path: Self::the_oci_index_path(),
            // The client versions go in the report: a client
            // without a version is a client of which it cannot be said that
            // it speaks the protocol.
            tools: format!(
                "{}; `oras {}`",
                the_tools(),
                the_version_of(&[&the_oras(), "version"])
            ),
            cases: Vec::new(),
        }
    }

    /// An empty report, the real-client compatibility one.
    ///
    /// It is a **separate** report, with its title and its notice: the index one
    /// certifies the queries and the attachments, and what is measured here is something
    /// else -- which real clients speak the protocol -- and a report that
    /// mixed it would say the registry complies with it entirely.
    pub fn for_oci_e2e() -> Self {
        Self {
            title: "Compatibilidad OCI con clientes reales — informe",
            disclaimer: OCI_E2E_DISCLAIMER,
            path: Self::the_oci_e2e_path(),
            // The client versions go in the report: a
            // compatibility without the client version is not a
            // compatibility, it is an intention.
            tools: format!(
                "{}; `oras {}`; `podman {}`; `docker {}`",
                the_tools(),
                the_version_of(&[&the_oras(), "version"]),
                the_first_line_of(&[&the_podman(), "--version"]),
                the_first_line_of(&[&the_docker(), "--version"]),
            ),
            cases: Vec::new(),
        }
    }

    /// Records a case and writes the report.
    ///
    /// A case with the same identifier **replaces** the previous one: a
    /// case that is recorded before running and recorded again with its
    /// result leaves a `FAILURE` in the report if the test dies
    /// before finishing, and not a case that does not appear. A report that
    /// can only say `PASS` is not a report: it is propaganda.
    pub fn record(&mut self, case: Case) {
        match self
            .cases
            .iter_mut()
            .find(|existing| existing.id == case.id)
        {
            Some(existing) => *existing = case,
            None => self.cases.push(case),
        }
        self.write();
    }

    /// The path of the report file.
    pub fn the_path() -> PathBuf {
        let configured = std::env::var_os("HODEI_UAT_REPORT").map(PathBuf::from);
        configured.unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("uat")
                .join("raw-conformity.md")
        })
    }

    /// The path of the OCI push report file.
    ///
    /// The raw surface report's is decided by the pipeline, and it is not
    /// mixed with that of another surface.
    pub fn the_oci_push_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("uat")
            .join("oci-push-conformity.md")
    }

    /// The path of the index report file.
    pub fn the_oci_index_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("uat")
            .join("oci-index-conformity.md")
    }

    /// The path of the real-client compatibility report
    /// file.
    pub fn the_oci_e2e_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("uat")
            .join("oci-compatibility.md")
    }

    /// Writes the report.
    ///
    /// The write is **atomic**: the report is written to a file
    /// next to it and renamed on top. A half-written report -- by a cut or
    /// by a reader arriving while it is being written -- is a corrupt
    /// report, and a corrupt report that exists passes the pipeline's guard
    /// as if it were good.
    pub fn write(&self) {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).expect("the directory of the report");
        }
        let temporary = self.path.with_extension("md.tmp");
        std::fs::write(&temporary, self.as_markdown()).expect("the report is written");
        std::fs::rename(&temporary, &self.path).expect("the report is put in its place");
    }

    /// The report, in Markdown.
    pub fn as_markdown(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!("# {}\n\n", self.title));
        text.push_str(&format!("**Servicio:** {}\n\n", the_version()));
        text.push_str(&format!("**Cuando:** {}\n\n", the_moment()));
        text.push_str(&format!("**Herramientas:** {}\n\n", self.tools));
        text.push_str(
            "\n**Credenciales:** van redactadas. Donde el `curl` llevaba el token, aqui \
             dice `<alice>` o `<bob>`: el informe dice con quien se hablo, no con que se \
             autentico. Un informe se archiva y se comparte, y un token en un fichero es \
             una credencial en un fichero.\n\n",
        );
        text.push_str("| Caso | Given | When | Comando | Resultado |\n");
        text.push_str("|---|---|---|---|---|\n");
        for case in &self.cases {
            text.push_str(&format!(
                "| {} | {} | {} | `{}` | {} {} |\n",
                the_cell(&case.id),
                the_cell(&case.given),
                the_cell(&case.when),
                the_cell(&case.command),
                if case.passed { "PASS" } else { "FALLO" },
                the_cell(&case.outcome)
            ));
        }
        text.push_str(&format!(
            "\n## Lo que este informe **no** certifica\n\n{}\n",
            self.disclaimer
        ));
        text
    }
}

/// The text of a cell of the report table.
///
/// The table is Markdown, so a vertical bar breaks the row and
/// a line break turns it into two: the text of a case -- which may
/// be the output of a client -- is escaped so that the table remains
/// a table.
///
/// And it is **redacted**: the report says who was talked to, not what they
/// authenticated with. A text that reaches the report without passing through here may
/// carry a token, and a report is archived and shared.
fn the_cell(text: &str) -> String {
    let mut cell = String::new();
    for line in text.lines() {
        if !cell.is_empty() {
            cell.push_str("<br>");
        }
        cell.push_str(&line.replace('|', "\\|"));
    }
    the_names()
        .lock()
        .expect("the names of the credentials")
        .iter()
        .filter(|(token, _)| !token.is_empty())
        .fold(cell, |cell, (token, name)| {
            cell.replace(token.as_str(), name)
        })
}

/// The report shared by the cases of this binary.
static REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

/// The shared report.
pub fn the_report() -> &'static Mutex<Report> {
    REPORT.get_or_init(|| Mutex::new(Report::new()))
}

/// Records a case in the shared report.
pub fn record(case: Case) {
    let mut report = the_report().lock().expect("the report is not poisoned");
    report.record(case);
}

/// The OCI push report, shared by the cases of its binary.
static OCI_PUSH_REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

/// The OCI push report.
pub fn the_oci_push_report() -> &'static Mutex<Report> {
    OCI_PUSH_REPORT.get_or_init(|| Mutex::new(Report::for_oci_push()))
}

/// Records a case in the OCI push report.
pub fn record_oci_push(case: Case) {
    let mut report = the_oci_push_report()
        .lock()
        .expect("the report is not poisoned");
    report.record(case);
}

/// The index and referrers report, shared by the cases of its
/// binary.
static OCI_INDEX_REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

/// The index and referrers report.
pub fn the_oci_index_report() -> &'static Mutex<Report> {
    OCI_INDEX_REPORT.get_or_init(|| Mutex::new(Report::for_oci_index()))
}

/// Records a case in the index and referrers report.
pub fn record_oci_index(case: Case) {
    let mut report = the_oci_index_report()
        .lock()
        .expect("the report is not poisoned");
    report.record(case);
}

/// The real-client compatibility report, shared by
/// the cases of its binary.
static OCI_E2E_REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

/// The real-client compatibility report.
pub fn the_oci_e2e_report() -> &'static Mutex<Report> {
    OCI_E2E_REPORT.get_or_init(|| Mutex::new(Report::for_oci_e2e()))
}

/// Records a case in the real-client compatibility
/// report.
pub fn record_oci_e2e(case: Case) {
    let mut report = the_oci_e2e_report()
        .lock()
        .expect("the report is not poisoned");
    report.record(case);
}

/// Where `oras` is.
///
/// It is not always on the `PATH` of whoever launches the tests, and a
/// tool that cannot be found is a tool that cannot be
/// tested: it is said where it is looked for and the place is left.
pub fn the_oras() -> String {
    if let Some(binary) = std::env::var_os("HODEI_ORAS_BIN") {
        return binary.to_string_lossy().into_owned();
    }
    THE_ORAS
        .get_or_init(|| {
            for candidate in [
                "oras".to_string(),
                format!(
                    "{}/.local/bin/oras",
                    std::env::var("HOME").unwrap_or_default()
                ),
            ] {
                if Command::new(&candidate)
                    .arg("version")
                    .output()
                    .is_ok_and(|output| output.status.success())
                {
                    return candidate;
                }
            }
            "oras".to_string()
        })
        .clone()
}

/// Where `oras` is: it is looked up **once**.
///
/// The lookup launches a process for each command the tool uses,
/// and a test that calls `oras` twenty times launches twenty processes
/// to ask the same thing.
static THE_ORAS: OnceLock<String> = OnceLock::new();

/// Where the CLI `docker` is.
///
/// The Docker CLI does not come with the system: it is installed separately, and the place
/// where it was put is stated, so that the report can name the version
/// it talked to.
pub fn the_docker() -> String {
    if let Some(binary) = std::env::var_os("HODEI_DOCKER_BIN") {
        return binary.to_string_lossy().into_owned();
    }
    THE_DOCKER
        .get_or_init(|| {
            for candidate in [
                "docker".to_string(),
                format!(
                    "{}/.local/bin/docker",
                    std::env::var("HOME").unwrap_or_default()
                ),
            ] {
                if Command::new(&candidate)
                    .arg("--version")
                    .output()
                    .is_ok_and(|output| output.status.success())
                {
                    return candidate;
                }
            }
            "docker".to_string()
        })
        .clone()
}

/// Where the CLI `docker` is: it is looked up **once**.
static THE_DOCKER: OnceLock<String> = OnceLock::new();

/// Where `podman` is.
///
/// `podman` is the image engine, and the one that acts as daemon for the
/// CLI `docker`: the Docker CLI does not talk to a registry, it talks to
/// a daemon.
pub fn the_podman() -> String {
    if let Some(binary) = std::env::var_os("HODEI_PODMAN_BIN") {
        return binary.to_string_lossy().into_owned();
    }
    THE_PODMAN
        .get_or_init(|| {
            for candidate in ["podman".to_string(), "/usr/bin/podman".to_string()] {
                if Command::new(&candidate)
                    .arg("--version")
                    .output()
                    .is_ok_and(|output| output.status.success())
                {
                    return candidate;
                }
            }
            "podman".to_string()
        })
        .clone()
}

/// Where `podman` is: it is looked up **once**.
static THE_PODMAN: OnceLock<String> = OnceLock::new();

/// The version of the service that has been tested.
pub fn the_version() -> String {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|commit| !commit.is_empty())
        .unwrap_or_else(|| "sin commit".to_string());
    format!(
        "hodei-artifacts {} (commit {})",
        env!("CARGO_PKG_VERSION"),
        commit
    )
}

/// The versions of the tools that take part in the conformance.
pub fn the_tools() -> String {
    let curl = the_first_line_of(&["curl", "--version"]);
    let sha = the_first_line_of(&["sha256sum", "--version"]);
    let cargo = the_first_line_of(&["cargo", "--version"]);
    let system = the_first_line_of(&["uname", "-srm"]);
    format!("`{curl}`; `{sha}`; `{cargo}`; sistema `{system}`")
}

/// The moment of the conformance.
///
/// A report without a date is a report that cannot be placed in
/// time, and a report that cannot be placed in time does not say whether
/// it has been repeated or whether it is the first time.
pub fn the_moment() -> String {
    let output = Command::new("date").args(["--iso-8601=seconds"]).output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "sin fecha".to_string(),
    }
}

/// The version of a tool, without the noise that surrounds it.
///
/// `oras` states its version on a line of the form
/// `Version:        1.2.0`, and in the report what matters is the
/// number.
fn the_version_of(command: &[&str]) -> String {
    let line = the_first_line_of(command);
    match line.split_once(':') {
        Some((_, version)) => version.trim().to_string(),
        None => line,
    }
}

/// The first line of the output of a command.
fn the_first_line_of(command: &[&str]) -> String {
    let Ok(output) = Command::new(command[0]).args(&command[1..]).output() else {
        return format!("{}: no disponible", command[0]);
    };
    if !output.status.success() {
        return format!("{}: no disponible", command[0]);
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("desconocida")
        .trim()
        .to_string()
}

/// Seeds the catalog in the current process.
///
/// It only seeds if the process is the one that another has ordered to seed: the
/// process that seeds is another, and the one under test is the service.
pub fn seed_if_asked() {
    let Some(db) = std::env::var_os("HODEI_SEED_DB") else {
        return;
    };
    // The graph profile: the locator names the tenant of the deployment and
    // the catalog is written through the real Dgraph store.
    if let Some(tenant) = graph_profile::GraphTenant::from_locator(&db.to_string_lossy()) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("the runtime")
            .block_on(graph_profile::seed_from_environment(&tenant));
        return;
    }
    panic!("HODEI_SEED_DB is not a graph tenant locator and the embedded engine is not built");
}
