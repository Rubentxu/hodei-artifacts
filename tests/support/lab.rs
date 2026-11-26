//! The laboratory of the registry: an upstream with a counter, the outcome
//! of a case and the JSON report of a run.
//!
//! What this module adds to the harness of `support` is not another service
//! and not another client: it is the **accountability** of a run. A harness
//! says whether a test passed; a laboratory says what was executed, what
//! was **not** executed, and why, and writes it where somebody can read it
//! six months later.
//!
//! Three rules govern everything here:
//!
//! - **A skipped case is never a green case.** `Outcome::NotRun` has a
//!   count of its own, and the summary keeps it apart from the passes.
//! - **The laboratory never touches the HOME of whoever runs it.** The
//!   service starts with an isolated HOME, and the tree of the real one is
//!   compared before and after.
//! - **The laboratory never reaches the network unless somebody asks for
//!   it.** The real mode is opt-in, and it has a budget of bytes and one
//!   of time.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;

// ---------------------------------------------------------------------------
// The upstream with a counter.
// ---------------------------------------------------------------------------

/// A request that reached the upstream.
#[derive(Debug, Clone)]
pub struct Seen {
    /// The path that was asked for, without the leading slash.
    pub path: String,
    /// The method of the request.
    pub method: String,
    /// The `Authorization` header, if the request carried one.
    pub authorization: Option<String>,
    /// The `If-None-Match` header, if the request carried one.
    pub if_none_match: Option<String>,
}

/// A document that the upstream serves.
#[derive(Debug, Clone)]
pub struct Doc {
    /// The bytes of the document.
    pub body: Vec<u8>,
    /// The validator of the document, if it has one.
    pub etag: Option<String>,
    /// The sha256 that the upstream **declares**, if it declares one.
    ///
    /// A declaration that does not match the bytes is what the digest
    /// scenario of the laboratory needs: the service has to refuse it.
    pub declared_sha256: Option<String>,
    /// The status the upstream answers with.
    pub status: u16,
    /// The place the upstream sends the client to, if it sends it anywhere.
    pub location: Option<String>,
}

impl Doc {
    /// A document with the given bytes.
    pub fn new(body: &[u8]) -> Self {
        Self {
            body: body.to_vec(),
            etag: None,
            declared_sha256: None,
            status: 200,
            location: None,
        }
    }

    /// The document, with a validator.
    pub fn with_etag(mut self, etag: &str) -> Self {
        self.etag = Some(etag.to_string());
        self
    }

    /// The document, with a declared sha256.
    pub fn with_declared_sha256(mut self, hex: &str) -> Self {
        self.declared_sha256 = Some(hex.to_string());
        self
    }

    /// The document, with the status the upstream answers with.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// The document, with the place it sends the client to.
    pub fn redirect(mut self, location: &str) -> Self {
        self.status = 302;
        self.location = Some(location.to_string());
        self
    }
}

/// The state of the upstream, which the laboratory changes while it runs.
struct World {
    docs: Mutex<HashMap<String, Doc>>,
    failing: AtomicBool,
    seen: Mutex<Vec<Seen>>,
    required: Mutex<Option<String>>,
}

/// A local HTTP upstream that counts what it is asked.
///
/// It is the **only** upstream of the laboratory: a counter that lives in
/// two places is a counter that measures one of them.
pub struct Upstream {
    port: u16,
    world: Arc<World>,
    live: Arc<AtomicBool>,
}

impl Upstream {
    /// An upstream with nothing in it.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the upstream binds");
        let port = listener
            .local_addr()
            .expect("the address of the upstream")
            .port();
        listener
            .set_nonblocking(true)
            .expect("the upstream accepts without blocking");
        let world = Arc::new(World {
            docs: Mutex::new(HashMap::new()),
            failing: AtomicBool::new(false),
            seen: Mutex::new(Vec::new()),
            required: Mutex::new(None),
        });
        let live = Arc::new(AtomicBool::new(true));
        let shared = Arc::clone(&world);
        let thread_live = Arc::clone(&live);
        std::thread::spawn(move || {
            while thread_live.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let world = Arc::clone(&shared);
                        std::thread::spawn(move || serve(stream, &world));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self { port, world, live }
    }

    /// The port in which the upstream listens.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The origin of the upstream, as the service sees it.
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The base URL of the upstream.
    pub fn base(&self) -> String {
        format!("{}/", self.origin())
    }

    /// Puts a document in the upstream.
    pub fn put(&self, path: &str, body: &[u8], etag: Option<&str>) {
        let mut doc = Doc::new(body);
        doc.etag = etag.map(str::to_string);
        self.put_doc(path, doc);
    }

    /// Puts a document in the upstream, with everything it declares.
    pub fn put_doc(&self, path: &str, doc: Doc) {
        self.world
            .docs
            .lock()
            .expect("the documents of the upstream")
            .insert(path.to_string(), doc);
    }

    /// Takes a document out of the upstream.
    pub fn remove(&self, path: &str) {
        self.world
            .docs
            .lock()
            .expect("the documents of the upstream")
            .remove(path);
    }

    /// Makes the upstream answer with an error to everything.
    pub fn fail(&self, failing: bool) {
        self.world.failing.store(failing, Ordering::SeqCst);
    }

    /// The credential that the upstream **demands**.
    ///
    /// An upstream that demands a credential answers with a `401` to
    /// anybody that does not bring exactly that one: a case that wants to
    /// measure that a credential travels has to be able to tell "the
    /// credential arrived" from "the credential did not".
    pub fn require_authorization(&self, value: &str) {
        self.world
            .required
            .lock()
            .expect("the credential of the upstream")
            .replace(value.to_string());
    }

    /// Whether the upstream is accepting connections.
    ///
    /// It is the proof that an upstream is **gone**: a counter that does
    /// not grow when the port is closed proves nothing, because it could
    /// not grow anyway.
    pub fn accepts_connections(&self) -> bool {
        std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok()
    }

    /// **Disconnects** the upstream: it stops accepting connections.
    ///
    /// This is not the same as failing: a failing upstream still receives
    /// the request and answers with an error, and the counter grows. A
    /// disconnected upstream is gone, and the counter stays where it was.
    /// That is what lets the laboratory tell "the cache answered" from
    /// "the upstream answered".
    pub fn disconnect(&self) {
        self.live.store(false, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if std::net::TcpStream::connect(("127.0.0.1", self.port)).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the upstream did not stop accepting connections");
    }

    /// Every request that reached the upstream.
    pub fn requests(&self) -> Vec<Seen> {
        self.world
            .seen
            .lock()
            .expect("the log of the upstream")
            .clone()
    }

    /// How many requests reached the upstream for a path.
    pub fn hits(&self, path: &str) -> usize {
        self.requests()
            .iter()
            .filter(|one| one.path == path)
            .count()
    }

    /// How many requests reached the upstream, of any path.
    pub fn total(&self) -> usize {
        self.world
            .seen
            .lock()
            .expect("the log of the upstream")
            .len()
    }
}

/// The value of a header of a request, if the request carries it.
fn header_of(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

/// The reason that goes with a status.
fn the_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// Serves one request of the upstream.
fn serve(mut stream: TcpStream, world: &World) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let text = String::from_utf8_lossy(&request).into_owned();
    let mut parts = text.lines().next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts
        .next()
        .unwrap_or("/")
        .trim_start_matches('/')
        .to_string();
    let if_none_match = header_of(&text, "if-none-match");
    let authorization = header_of(&text, "authorization");
    world
        .seen
        .lock()
        .expect("the log of the upstream")
        .push(Seen {
            path: path.clone(),
            method,
            authorization: authorization.clone(),
            if_none_match: if_none_match.clone(),
        });

    let doc = world
        .docs
        .lock()
        .expect("the documents of the upstream")
        .get(&path)
        .cloned();
    let required = world
        .required
        .lock()
        .expect("the credential of the upstream")
        .clone();
    let response: Vec<u8> = if world.failing.load(Ordering::SeqCst) {
        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec()
    } else if let Some(required) = required
        && authorization.as_deref() != Some(required.as_str())
    {
        // The credential is not the one that the upstream demands.
        let body = b"the credential is not valid";
        let mut out = format!(
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    } else if let Some(doc) = doc {
        the_response(&doc, if_none_match.as_deref())
    } else {
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

/// The response of the upstream for a document.
fn the_response(doc: &Doc, if_none_match: Option<&str>) -> Vec<u8> {
    let validators = doc
        .etag
        .as_ref()
        .map(|etag| format!("ETag: {etag}\r\n"))
        .unwrap_or_default();
    let checksum = doc
        .declared_sha256
        .as_ref()
        .map(|hex| format!("X-Checksum-Sha256: {hex}\r\n"))
        .unwrap_or_default();
    if let Some(location) = &doc.location {
        return format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
    }
    let unchanged = matches!(
        (&doc.etag, if_none_match),
        (Some(etag), Some(sent)) if etag == sent
    );
    if unchanged {
        return format!(
            "HTTP/1.1 304 Not Modified\r\n{validators}{checksum}Content-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
    }
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\n{validators}{checksum}Connection: close\r\n\r\n",
        doc.status,
        the_reason(doc.status),
        doc.body.len()
    )
    .into_bytes();
    out.extend_from_slice(&doc.body);
    out
}

// ---------------------------------------------------------------------------
// The outcome of a case.
// ---------------------------------------------------------------------------

/// What a case was measuring.
///
/// It is what tells a **refusal that was wanted** from a **failure that
/// was not**: both end in `Outcome::Fail`, and only the expectation says
/// which one happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Expectation {
    /// The operation had to succeed.
    Succeeds,
    /// The operation had to be refused: the refusal is the measured result.
    Refused,
}

/// The result of a case.
///
/// `NotRun` is not a soft `Pass`: a case that did not run has not proved
/// anything, and the summary keeps it apart from the passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Outcome {
    /// The case ran and proved what it was measuring.
    #[serde(rename = "PASS")]
    Pass,
    /// The case ran and did not prove it. The cause says why.
    #[serde(rename = "FAIL")]
    Fail,
    /// The case did not run. The cause says why.
    #[serde(rename = "NOT_RUN")]
    NotRun,
}

impl Outcome {
    /// The label of the outcome, as it is written in the report.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::NotRun => "NOT_RUN",
        }
    }

    /// Whether this outcome may be counted as green.
    ///
    /// Only `Pass` may. A `NotRun` that counted as green would be a lie
    /// about what was measured.
    pub fn is_green(self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// What a case measures.
///
/// A case that measures the **harness** says nothing about the product:
/// it says that the laboratory counts what it says it counts. Mixing the
/// two in one summary would let a green harness dress a product that was
/// never measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// The case measures the product: the registry, as it is.
    Product,
    /// The case measures the harness: the laboratory itself.
    Harness,
}

/// A case of the laboratory.
#[derive(Clone, Debug, Serialize)]
pub struct LabCase {
    /// The identifier of the case.
    pub id: String,
    /// What the case is about.
    pub title: String,
    /// What the case measures: the product or the harness.
    pub scope: Scope,
    /// What the case was measuring.
    pub expectation: Expectation,
    /// The command that carried the case out.
    pub command: String,
    /// The result of the case.
    pub result: Outcome,
    /// Whether the result was the one the case was measuring.
    ///
    /// A refusal that the case was asking for is a result: it is a
    /// failure of the operation and the point of the case at the same
    /// time. A failure nobody asked for is not.
    pub expected: bool,
    /// The cause of a failure, or the reason a case did not run.
    pub cause: Option<String>,
    /// The origin the case talked to, when it talked to one.
    pub origin: Option<String>,
    /// The digest of what came back, when something came back.
    pub sha256: Option<String>,
}

impl LabCase {
    /// A case with an identifier and a title.
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            scope: Scope::Product,
            expectation: Expectation::Succeeds,
            command: String::new(),
            result: Outcome::NotRun,
            expected: false,
            cause: None,
            origin: None,
            sha256: None,
        }
    }

    /// The case, with what it measures.
    pub fn scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// The case, with what it was measuring.
    pub fn expecting(mut self, expectation: Expectation) -> Self {
        self.expectation = expectation;
        self
    }

    /// The case, with the command that carried it out.
    pub fn command(mut self, command: &str) -> Self {
        self.command = command.to_string();
        self
    }

    /// The case, with the origin it talked to.
    pub fn origin(mut self, origin: &str) -> Self {
        self.origin = Some(origin.to_string());
        self
    }

    /// The case, with the digest of what came back.
    pub fn sha256(mut self, digest: &str) -> Self {
        self.sha256 = Some(digest.to_string());
        self
    }

    /// The case passed.
    pub fn passed(mut self) -> Self {
        self.result = Outcome::Pass;
        self.expected = true;
        self
    }

    /// The case failed, with the cause.
    ///
    /// The failure is one that nobody asked for.
    pub fn failed(mut self, cause: impl Into<String>) -> Self {
        self.result = Outcome::Fail;
        self.expected = false;
        self.cause = Some(cause.into());
        self
    }

    /// The case was refused, with the cause of the refusal.
    ///
    /// The operation failed and that was the point of the case: the
    /// registry rejected what it had to reject.
    pub fn refused(mut self, cause: impl Into<String>) -> Self {
        self.result = Outcome::Fail;
        self.expected = true;
        self.cause = Some(cause.into());
        self
    }

    /// The case did not run, with the reason.
    pub fn not_run(mut self, reason: impl Into<String>) -> Self {
        self.result = Outcome::NotRun;
        self.expected = false;
        self.cause = Some(reason.into());
        self
    }
}

/// The summary of a run.
///
/// The counts are **separate on purpose**: `not_run` is not a flavour of
/// `pass`, and adding them together would be the lie this module exists
/// to avoid.
#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    /// The cases that passed.
    pub pass: usize,
    /// The cases that failed.
    pub fail: usize,
    /// The cases that did not run.
    pub not_run: usize,
    /// The failures that were the point of their case: a refusal that was
    /// expected.
    pub expected_failures: usize,
    /// The failures nobody expected.
    pub unexpected_failures: usize,
    /// Whether the run may be called green.
    ///
    /// It may only be called green when nothing failed unexpectedly **and**
    /// nothing was skipped. A skipped case is not a green case.
    pub green: bool,
    /// Whether every case of the run was executed.
    pub complete: bool,
}

impl Summary {
    /// The summary of a list of cases.
    pub fn of(cases: &[LabCase]) -> Self {
        let pass = cases
            .iter()
            .filter(|case| case.result == Outcome::Pass)
            .count();
        let fail = cases
            .iter()
            .filter(|case| case.result == Outcome::Fail)
            .count();
        let not_run = cases
            .iter()
            .filter(|case| case.result == Outcome::NotRun)
            .count();
        let expected_failures = cases
            .iter()
            .filter(|case| case.result == Outcome::Fail && case.expected)
            .count();
        let unexpected_failures = fail - expected_failures;
        Self {
            pass,
            fail,
            not_run,
            expected_failures,
            unexpected_failures,
            green: unexpected_failures == 0 && not_run == 0 && pass > 0,
            complete: not_run == 0,
        }
    }

    /// The summary of the cases that measure one thing.
    ///
    /// The harness and the product are counted apart: a laboratory that
    /// counts well does not certify a registry that was never measured.
    pub fn of_scope(cases: &[LabCase], scope: Scope) -> Self {
        let cases: Vec<LabCase> = cases
            .iter()
            .filter(|case| case.scope == scope)
            .cloned()
            .collect();
        Self::of(&cases)
    }

    /// The cases that may be counted as green.
    ///
    /// It is the count of the passes, and nothing else: a case that did
    /// not run is never part of it.
    pub fn green_cases(&self) -> usize {
        self.pass
    }
}

// ---------------------------------------------------------------------------
// The budget of the real mode.
// ---------------------------------------------------------------------------

/// The variable that turns the real mode on.
pub const REAL_MODE_VARIABLE: &str = "HODEI_LAB_REAL";

/// The variable that caps the bytes of the real mode.
pub const MAX_BYTES_VARIABLE: &str = "HODEI_LAB_MAX_BYTES";

/// The variable that caps the seconds of the real mode.
pub const MAX_SECONDS_VARIABLE: &str = "HODEI_LAB_MAX_SECONDS";

/// The bytes a real run may bring when nobody says otherwise.
pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024;

/// The seconds a real run may last when nobody says otherwise.
pub const DEFAULT_MAX_SECONDS: u64 = 30;

/// Whether the real mode has been asked for.
pub fn real_mode_enabled() -> bool {
    std::env::var(REAL_MODE_VARIABLE).is_ok_and(|value| {
        let value = value.trim();
        value == "1" || value.eq_ignore_ascii_case("true")
    })
}

/// The budget of a real run: the most bytes and the most seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Budget {
    /// The most bytes a real fetch may bring.
    pub max_bytes: u64,
    /// The most seconds a real fetch may last.
    pub max_seconds: u64,
}

impl Budget {
    /// The budget of the environment, with the defaults when it says none.
    pub fn from_environment() -> Self {
        let max_bytes = std::env::var(MAX_BYTES_VARIABLE)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|bytes| *bytes > 0)
            .unwrap_or(DEFAULT_MAX_BYTES);
        let max_seconds = std::env::var(MAX_SECONDS_VARIABLE)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|seconds| *seconds > 0)
            .unwrap_or(DEFAULT_MAX_SECONDS);
        Self {
            max_bytes,
            max_seconds,
        }
    }

    /// Whether a transfer of `bytes` in `elapsed` fits in the budget.
    pub fn permits(&self, bytes: u64, elapsed: Duration) -> Result<(), String> {
        if bytes > self.max_bytes {
            return Err(format!(
                "the transfer brought {bytes} bytes and the budget allows {}",
                self.max_bytes
            ));
        }
        if elapsed > Duration::from_secs(self.max_seconds) {
            return Err(format!(
                "the transfer took {elapsed:?} and the budget allows {} seconds",
                self.max_seconds
            ));
        }
        Ok(())
    }

    /// The environment that bounds the fetch **inside the service**.
    ///
    /// The budget of the laboratory is not only a promise of the client:
    /// the service itself refuses to bring more than the budget allows.
    pub fn service_environment(&self) -> Vec<(String, String)> {
        vec![
            (
                "HODEI_PROXY__MAX_BYTES".to_string(),
                self.max_bytes.to_string(),
            ),
            (
                "HODEI_PROXY__TOTAL_TIMEOUT_SECS".to_string(),
                self.max_seconds.to_string(),
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// The isolation of the laboratory.
// ---------------------------------------------------------------------------

/// The HOME of the laboratory: a temporary directory the service may write
/// to without touching the HOME of whoever runs the laboratory.
pub struct LabHome {
    directory: tempfile::TempDir,
}

impl LabHome {
    /// A HOME that belongs to nobody.
    pub fn new() -> Self {
        Self {
            directory: tempfile::tempdir().expect("a home for the laboratory"),
        }
    }

    /// The path of the HOME of the laboratory.
    pub fn path(&self) -> &Path {
        self.directory.path()
    }

    /// The environment that points HOME and the XDG directories at the
    /// laboratory.
    ///
    /// `HOME` is what a program uses when it does not know better; the
    /// `XDG_*` are what a program that does know better uses. Both have to
    /// point here, or the program would write where it always writes.
    pub fn environment(&self) -> Vec<(String, String)> {
        let root = self.directory.path();
        [
            ("HOME", root.to_path_buf()),
            ("XDG_CONFIG_HOME", root.join("config")),
            ("XDG_CACHE_HOME", root.join("cache")),
            ("XDG_DATA_HOME", root.join("data")),
            ("XDG_STATE_HOME", root.join("state")),
        ]
        .into_iter()
        .map(|(name, path)| {
            let path = path.to_string_lossy().into_owned();
            std::fs::create_dir_all(&path).expect("the directories of the laboratory");
            (name.to_string(), path)
        })
        .collect()
    }
}

impl Default for LabHome {
    fn default() -> Self {
        Self::new()
    }
}

/// The tree of a HOME directory, at a moment.
#[derive(Clone, Debug)]
pub struct HomeSnapshot {
    home: PathBuf,
    entries: BTreeMap<String, (u64, Option<SystemTime>, bool)>,
}

impl HomeSnapshot {
    /// The tree of a HOME directory, up to a number of levels below it.
    ///
    /// The symlinks are not followed and the directories that cannot be
    /// read are skipped: a tree that cannot be walked is not a tree that
    /// somebody changed.
    pub fn capture(home: &Path, levels: usize) -> Self {
        Self::capture_matching(home, levels, "")
    }

    /// The tree of a HOME directory, keeping only the paths that mention
    /// a needle.
    ///
    /// The HOME of a person is a directory that **other processes write
    /// to**: a tree compared whole would report the traffic of the
    /// machine as if it were the traffic of the laboratory. The trace of
    /// the laboratory is what has its name in it, and that is what this
    /// keeps.
    pub fn capture_matching(home: &Path, levels: usize, needle: &str) -> Self {
        let mut entries = BTreeMap::new();
        walk(home, home, levels, needle, &mut entries);
        Self {
            home: home.to_path_buf(),
            entries,
        }
    }

    /// The HOME that was walked.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// How many entries the tree has.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The fingerprint of the tree: how many **files** it has and how many
    /// bytes they weigh.
    ///
    /// It is a number and not a list because a number does not depend on
    /// what else the machine is doing: a file that another process wrote
    /// elsewhere in the HOME does not change it, and a file that the
    /// laboratory wrote does.
    ///
    /// It is not, all the same, a certificate of anything: the HOME of a
    /// person is a directory that **other processes write to**, and a
    /// fingerprint compared before and after a run can blame the
    /// laboratory for the traffic of the machine. The proof that the
    /// laboratory did not touch the HOME is the environment of the
    /// service, not a number.
    pub fn fingerprint(&self) -> (usize, u64) {
        let files: Vec<_> = self.entries.values().filter(|(_, _, file)| *file).collect();
        (files.len(), files.iter().map(|(size, _, _)| size).sum())
    }

    /// What changed between two snapshots of the same tree.
    ///
    /// The list is empty when nothing changed. It names what appeared,
    /// what disappeared and what was modified, because a tree that
    /// "changed" without saying how is a tree nobody can act on.
    pub fn differences(&self, other: &HomeSnapshot) -> Vec<String> {
        let mut differences = Vec::new();
        for (path, entry) in &self.entries {
            match other.entries.get(path) {
                None => differences.push(format!("appeared: {path}")),
                Some(now) if now != entry => {
                    differences.push(format!("changed: {path}"));
                }
                Some(_) => {}
            }
        }
        for path in other.entries.keys() {
            if !self.entries.contains_key(path) {
                differences.push(format!("disappeared: {path}"));
            }
        }
        differences.sort();
        differences
    }
}

/// Walks a tree, up to a number of levels below the home, keeping the
/// paths that mention a needle.
fn walk(
    home: &Path,
    directory: &Path,
    levels: usize,
    needle: &str,
    entries: &mut BTreeMap<String, (u64, Option<SystemTime>, bool)>,
) {
    if levels == 0 {
        return;
    }
    let Ok(reader) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in reader.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_symlink() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(home) else {
            continue;
        };
        let name = relative.to_string_lossy().into_owned();
        if needle.is_empty() || name.to_lowercase().contains(&needle.to_lowercase()) {
            entries.insert(
                name,
                (metadata.len(), metadata.modified().ok(), metadata.is_file()),
            );
        }
        if metadata.is_dir() {
            walk(home, &path, levels - 1, needle, entries);
        }
    }
}

// ---------------------------------------------------------------------------
// The report.
// ---------------------------------------------------------------------------

/// The disclaimer of the report: what this laboratory does **not** certify.
pub const DISCLAIMER: &[&str] = &[
    "- **Ni la conformidad de un cliente real.** Lo que se ha ejecutado es el camino `raw` del registro contra un upstream local: no se ha hablado con `mvn`, `npm`, `pip`, `cargo`, `go` ni `dotnet`, y este informe no dice nada de ellos.",
    "- **Ni la red real.** Por defecto el laboratorio no sale a la red: el upstream es un servidor local. El modo real es opt-in (`HODEI_LAB_REAL=1`) y esta ejecución no lo ha usado.",
    "- **Ni la disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea: lo que se ha medido es lo que el registro devuelve y lo que el upstream recibe.",
    "- **Ni el borrado de contenido.** El registro no implementa el borrado de blobs, y nada de lo que se ha ejecutado lo ha medido.",
    "- **Ni el laboratorio.** Los casos de `scope: harness` miden el arnés --el recuento, el presupuesto, el aislamiento-- y no el producto: que el laboratorio cuente bien no dice nada de un registro que no se ha medido. Los dos recuentos van aparte.",
];

/// What the outcomes of the report mean.
pub const SEMANTICS: &str = "\
- `PASS`: el caso se ejecutó y la operación salió bien.\n\
- `FAIL`: el caso se ejecutó y la operación salió mal. El campo `expected` \
dice si ese fallo era el resultado medido --un rechazo que el caso pedía-- \
o un fallo que nadie pedía.\n\
- `NOT_RUN`: el caso no se ejecutó. No cuenta como verde, ni ahora ni \
después: un caso que no se ha ejecutado no ha demostrado nada. Un caso que \
se inscribió y no llegó a terminar queda `NOT_RUN`, y el que no aparece \
--porque la ejecución no lo alcanzó-- también.\n\
- `green`: no hay ningún fallo inesperado y no se ha saltado ningún caso. \
`expected_failures` son los rechazos que el caso pedía; \
`unexpected_failures` son los fallos que nadie pedía. `complete` dice si \
se ha ejecutado todo.\n\
- `scope`: `product` mide el registro; `harness` mide el laboratorio. Los \
dos recuentos van aparte, y el resumen del arnés no certifica el producto.";

/// The real mode, as the report writes it.
#[derive(Clone, Debug, Serialize)]
pub struct RealMode {
    /// Whether the real mode was on.
    pub enabled: bool,
    /// The budget of bytes of the run.
    pub max_bytes: u64,
    /// The budget of seconds of the run.
    pub max_seconds: u64,
}

/// The document that the report writes.
#[derive(Clone, Debug, Serialize)]
struct Document {
    title: String,
    disclaimer: Vec<String>,
    semantics: String,
    moment: String,
    service: String,
    tools: String,
    real_mode: RealMode,
    cases: Vec<LabCase>,
    summary: Summary,
    product: Summary,
    harness: Summary,
}

/// The report of the laboratory.
pub struct LabReport {
    title: String,
    path: PathBuf,
    budget: Budget,
    cases: Vec<LabCase>,
    expected: Vec<(String, String)>,
    disclaimer: Vec<String>,
}

impl LabReport {
    /// A report with a title, written where `the_path` says.
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            path: Self::the_path(),
            budget: Budget::from_environment(),
            cases: Vec::new(),
            expected: Vec::new(),
            disclaimer: DISCLAIMER.iter().map(|line| line.to_string()).collect(),
        }
    }

    /// The title of the report.
    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
    }

    /// What the report does **not** certify.
    ///
    /// The disclaimer belongs to the suite: a suite that talks to a real
    /// client cannot use the disclaimer of one that talks to none.
    pub fn set_disclaimer(&mut self, lines: &[&str]) {
        self.disclaimer = lines.iter().map(|line| line.to_string()).collect();
    }

    /// The cases that a complete run has to execute.
    ///
    /// The report describes **a run**, and a run that only reached one
    /// case has not executed the others: every case that is expected and
    /// does not appear counts as `NOT_RUN`, and a report with a case that
    /// did not run is never green.
    pub fn expect_cases(&mut self, expected: &[(&str, &str)]) {
        self.expected = expected
            .iter()
            .map(|(id, title)| (id.to_string(), title.to_string()))
            .collect();
    }

    /// The cases of the run, with the ones that did not run.
    pub fn effective_cases(&self) -> Vec<LabCase> {
        let mut cases = self.cases.clone();
        for (id, title) in &self.expected {
            if !cases.iter().any(|case| &case.id == id) {
                cases.push(
                    LabCase::new(id, title).not_run("the case did not run in this execution"),
                );
            }
        }
        cases
    }

    /// Where the report is written.
    ///
    /// The variable of the environment wins, so that a run can keep its
    /// report somewhere else without touching the code.
    ///
    /// Without it, the name comes from the **test binary** that is
    /// running: every suite writes its own report, and two suites do
    /// not overwrite each other's report.
    pub fn the_path() -> PathBuf {
        std::env::var_os("HODEI_LAB_REPORT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target")
                    .join("uat")
                    .join(the_report_name())
            })
    }

    /// The budget of the run.
    pub fn budget(&self) -> Budget {
        self.budget
    }

    /// The summary of the run.
    pub fn summary(&self) -> Summary {
        Summary::of(&self.effective_cases())
    }

    /// The cases of the run.
    pub fn cases(&self) -> &[LabCase] {
        &self.cases
    }

    /// Notes a case and writes the report.
    ///
    /// A case with the same identifier **replaces** the previous one: a
    /// case that is noted before it runs and noted again with its result
    /// leaves in the report what really happened, and not a `PASS` that
    /// nobody measured.
    pub fn record(&mut self, case: LabCase) {
        match self.cases.iter_mut().find(|one| one.id == case.id) {
            Some(existing) => *existing = case,
            None => self.cases.push(case),
        }
        self.write();
    }

    /// Writes the report.
    ///
    /// The writing is **atomic**: the report goes to a file next to it and
    /// is renamed over it. A report cut in half is a corrupt report, and a
    /// corrupt report that exists passes as if it were good.
    pub fn write(&self) {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).expect("the directory of the report");
        }
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, self.as_json()).expect("the report is written");
        std::fs::rename(&temporary, &self.path).expect("the report is put in its place");
    }

    /// The report, as JSON.
    pub fn as_json(&self) -> String {
        let cases = self.effective_cases();
        let document = Document {
            title: self.title.clone(),
            disclaimer: self.disclaimer.clone(),
            semantics: SEMANTICS.to_string(),
            moment: super::the_moment(),
            service: super::the_version(),
            tools: super::the_tools(),
            real_mode: RealMode {
                enabled: real_mode_enabled(),
                max_bytes: self.budget.max_bytes,
                max_seconds: self.budget.max_seconds,
            },
            summary: Summary::of(&cases),
            product: Summary::of_scope(&cases, Scope::Product),
            harness: Summary::of_scope(&cases, Scope::Harness),
            cases,
        };
        serde_json::to_string_pretty(&document).expect("the report is json")
    }
}

/// Notes a case in the report of the laboratory of the registry.
pub fn record_registry_lab(case: LabCase) {
    let mut report = the_registry_lab_report()
        .lock()
        .expect("the report of the laboratory");
    report.record(case);
}

/// Declares the cases that a complete run has to execute.
///
/// The declaration is what makes a partial run visible: a case that the
/// run did not reach counts as `NOT_RUN`, and a report with a case that
/// did not run is never green.
pub fn expect_registry_lab_cases(expected: &[(&str, &str)]) {
    let mut report = the_registry_lab_report()
        .lock()
        .expect("the report of the laboratory");
    report.expect_cases(expected);
}

/// Declares the title and the disclaimer of the report of this suite.
///
/// The disclaimer is what the report does **not** certify, and it belongs
/// to the suite: a suite that talks to a real client cannot use the
/// disclaimer of one that talks to none.
pub fn describe_registry_lab(title: &str, disclaimer: &[&str]) {
    let mut report = the_registry_lab_report()
        .lock()
        .expect("the report of the laboratory");
    report.set_title(title);
    report.set_disclaimer(disclaimer);
}

/// Runs a case of the laboratory of the registry.
///
/// The cases that a complete run has to execute are declared **before**
/// the case runs, so that a run which only reached some of them says so:
/// the ones that did not run count as `NOT_RUN`, and the report is never
/// green.
///
/// The case itself is **inscribed before it runs**, with `NOT_RUN`, and
/// replaced when it ends: a case that dies in the middle leaves a
/// `NOT_RUN` in the report, and a panic leaves a `FAIL` with its cause.
/// The panic is resumed afterwards, so that the suite still fails: a
/// laboratory that swallows the failure is not a laboratory.
pub fn run_registry_lab_case(
    expected: &[(&str, &str)],
    id: &str,
    title: &str,
    scope: Scope,
    expectation: Expectation,
    body: impl FnOnce() -> LabCase,
) {
    expect_registry_lab_cases(expected);
    record_registry_lab(
        LabCase::new(id, title)
            .scope(scope)
            .expecting(expectation)
            .not_run("iniciado"),
    );
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
    match outcome {
        Ok(mut case) => {
            // The scope and the expectation are declared by the caller:
            // what the case measured does not depend on how the case
            // built itself.
            case.scope = scope;
            case.expectation = expectation;
            // A failure that **nobody asked for** is a failure of the suite: the
            // laboratory exists so that a failure is seen, and a report
            // with `green: false` that `cargo test` does not see is a false
            // green. A refusal that the case asked for (a `refused`) is a
            // result: it does not fail the suite.
            let unexpected = case.result == Outcome::Fail && !case.expected;
            let cause = case.cause.clone();
            record_registry_lab(case);
            if unexpected {
                panic!(
                    "the case {id} failed: {}",
                    cause.unwrap_or_else(|| "the cause was not said".to_string())
                );
            }
        }
        Err(panic) => {
            let cause = the_cause_of_the_panic(&panic);
            record_registry_lab(
                LabCase::new(id, title)
                    .scope(scope)
                    .expecting(expectation)
                    .failed(cause),
            );
            std::panic::resume_unwind(panic);
        }
    }
}

/// The message of a panic.
fn the_cause_of_the_panic(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "the case panicked without a message".to_string()
    }
}

/// The name of the report of the suite that is running.
///
/// Cargo names a test binary `<target>-<hash>`, and the hash is
/// hexadecimal: what the report is about is the target, not the hash.
/// A binary that does not follow that shape keeps its whole name.
fn the_report_name() -> String {
    let stem = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "registry-lab".to_string());
    let name = match stem.rsplit_once('-') {
        Some((name, hash))
            if hash.len() == 16 && hash.chars().all(|character| character.is_ascii_hexdigit()) =>
        {
            name.to_string()
        }
        _ => stem,
    };
    format!("{}.json", name.replace('_', "-"))
}

/// The report of the laboratory of the registry, shared by its cases.
static REGISTRY_LAB_REPORT: OnceLock<Mutex<LabReport>> = OnceLock::new();

/// The report of the laboratory of the registry.
pub fn the_registry_lab_report() -> &'static Mutex<LabReport> {
    REGISTRY_LAB_REPORT
        .get_or_init(|| Mutex::new(LabReport::new("Laboratorio del registro — informe")))
}
