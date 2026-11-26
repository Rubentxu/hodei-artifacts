//! Deployment readiness: which dependencies the active profile requires and
//! whether they are available.
//!
//! Readiness is **not** liveness. Liveness says the process
//! is alive; readiness says the process can serve: that the
//! dependencies the active profile requires --the graph and the content
//! store-- are at hand. A deployment without a graph cannot say
//! which repository exists, and a deployment without a content store has no
//! bytes to serve: in both cases readiness is a `503`, and the
//! process stays alive.
//!
//! The port lives here and not in the handler for two reasons:
//!
//! - the handler has no state: the network budget and the short cache
//!   are in the wrapper, and can be tested without a server;
//! - the response body is **coarse** on purpose: it says which
//!   dependency fails and how, but never where it is or what it
//!   authenticates with.

use async_trait::async_trait;
use hodei_graph_client::{GraphClientError, GraphHealthPort};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The network budget of **one** dependency.
///
/// It is short on purpose: readiness is queried by an orchestrator every
/// few seconds, and a check that hangs is a check that
/// piles up. One second is enough to notice that a local store is not
/// there and that a remote bucket does not answer.
pub const DEPENDENCY_BUDGET: Duration = Duration::from_secs(1);

/// The network budget of the **whole** probe.
///
/// With two required dependencies and a one-second budget each,
/// the whole probe does not exceed two seconds. A deployment that requires more
/// dependencies than budget only checks the ones that fit, and the
/// rest are declared `Timeout`: it is better to say it could not be checked
/// than to lie by saying they are fine.
pub const PROBE_BUDGET: Duration = Duration::from_secs(2);

/// The time a verdict is cached.
///
/// It is short on purpose: one second is enough so that two consecutive
/// probes --an orchestrator asking every so often-- do not spend the
/// budget twice, and it is short enough that the
/// recovery of a dependency shows up immediately.
pub const VERDICT_TTL: Duration = Duration::from_secs(1);

/// A dependency the active profile requires in order to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dependency {
    /// The content store: the bytes.
    ContentStore,
    /// The graph authority (Dgraph): reachable and with a valid ACL login.
    Graph,
    /// The trust store, when its read happens hot.
    ///
    /// In this deployment the trust policy is loaded into memory at
    /// startup: it cannot be lost hot, and a check that
    /// always passes is a lie. That is why nobody requires it.
    TrustPolicy,
}

impl Dependency {
    /// The name of the dependency, as it appears in the response body.
    ///
    /// It is the **name** and not the address: the body says which dependency
    /// fails, not where it is.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ContentStore => "content_store",
            Self::Graph => "graph",
            Self::TrustPolicy => "trust_policy",
        }
    }
}

/// Why a required dependency is not available.
///
/// Coarse on purpose: the response body cannot carry a URL, a bucket,
/// a path or a credential. The driver's raw error
/// can carry all three --an S3 error carries the endpoint and
/// sometimes the credential--, so it does not travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyFailure {
    /// It cannot be reached.
    Unreachable,
    /// It does not answer within the budget.
    Timeout,
    /// It is declared in a way that cannot be used.
    Misconfigured,
}

impl DependencyFailure {
    /// The state of the dependency, as it appears in the response body.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Timeout => "timeout",
            Self::Misconfigured => "misconfigured",
        }
    }
}

/// The deployment's verdict.
///
/// It is coarse like the failure reason: it says **which** dependency is not there,
/// and nothing more.
#[derive(Debug, Clone)]
pub struct ReadinessReport {
    /// Whether the deployment can serve.
    pub ready: bool,
    /// The required dependencies that fail, with their reason.
    pub failing: Vec<(Dependency, DependencyFailure)>,
}

/// The checker of the dependencies required by the active profile.
#[async_trait]
pub trait ReadinessProbe: Send + Sync {
    /// The dependencies the active profile requires.
    ///
    /// The ones that are not here are not checked and cannot make
    /// readiness fail: an optional dependency that the profile does not require is not
    /// a dependency.
    fn required(&self) -> Vec<Dependency>;

    /// Checks **one** dependency.
    ///
    /// A dependency error is **not** a panic or a `500`: it is a
    /// readiness state.
    async fn check(&self, dependency: Dependency) -> Result<(), DependencyFailure>;

    /// The deployment's verdict.
    ///
    /// The default method is the one that applies the budget: each
    /// dependency has its own deadline and the whole probe has its own.
    /// A probe that returns a verdict without a budget --a fake
    /// one-- says so here.
    async fn report(&self) -> ReadinessReport {
        let mut failing = Vec::new();
        let deadline = Instant::now() + PROBE_BUDGET;
        for dependency in self.required() {
            // The probe's budget overrides the dependency's:
            // what does not fit is not checked, and is declared `Timeout`.
            if Instant::now() >= deadline {
                failing.push((dependency, DependencyFailure::Timeout));
                continue;
            }
            let outcome = tokio::time::timeout(DEPENDENCY_BUDGET, self.check(dependency)).await;
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(failure)) => failing.push((dependency, failure)),
                Err(_elapsed) => failing.push((dependency, DependencyFailure::Timeout)),
            }
        }
        ReadinessReport {
            ready: failing.is_empty(),
            failing,
        }
    }
}

/// A cached verdict, with the instant at which it was taken.
struct Cached {
    at: Instant,
    report: ReadinessReport,
}

/// The probe with the short verdict cache.
///
/// The cache goes **here** and not in the handler: the handler has no state, and
/// the probe can be tested alone. Two consecutive readiness checks within the TTL
/// do not spend budget: the second returns the cached verdict.
pub struct CachedProbe {
    inner: Arc<dyn ReadinessProbe>,
    ttl: Duration,
    cache: Mutex<Option<Cached>>,
}

impl CachedProbe {
    /// The real probe, wrapped in the short cache.
    pub fn new(inner: Arc<dyn ReadinessProbe>) -> Self {
        Self::with_ttl(inner, VERDICT_TTL)
    }

    /// The real probe, wrapped in a cache with the given deadline.
    ///
    /// The default deadline is that of [`VERDICT_TTL`]; the one passed in is
    /// for tests, which cannot sleep a second per case.
    pub fn with_ttl(inner: Arc<dyn ReadinessProbe>, ttl: Duration) -> Self {
        Self {
            inner,
            ttl,
            cache: Mutex::new(None),
        }
    }

    /// The real probe this one wraps.
    pub fn the_inner(&self) -> &Arc<dyn ReadinessProbe> {
        &self.inner
    }
}

#[async_trait]
impl ReadinessProbe for CachedProbe {
    fn required(&self) -> Vec<Dependency> {
        self.inner.required()
    }

    async fn check(&self, dependency: Dependency) -> Result<(), DependencyFailure> {
        self.inner.check(dependency).await
    }

    /// The verdict, cached.
    async fn report(&self) -> ReadinessReport {
        {
            let cache = self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(cached) = cache.as_ref()
                && cached.at.elapsed() < self.ttl
            {
                return cached.report.clone();
            }
        }
        let report = self.inner.report().await;
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *cache = Some(Cached {
            at: Instant::now(),
            report: report.clone(),
        });
        report
    }
}

/// How the content store is checked.
///
/// The profile decides it at startup: a deployment with content in
/// a directory and a deployment with content in a bucket are not
/// checked the same way, and what says so is the configuration.
pub enum ContentCheck {
    /// A filesystem directory.
    Filesystem {
        /// The directory where the content lives.
        root: PathBuf,
    },
    /// An S3 bucket.
    S3 {
        /// The client, already configured with the credentials chain.
        client: aws_sdk_s3::Client,
        /// The bucket where the content lives.
        bucket: String,
    },
}

impl ContentCheck {
    /// Checks the content store.
    ///
    /// The directory is checked with a filesystem call:
    /// it exists and is a directory. Nothing inside is read: readiness
    /// does not need the content, it needs the place where it lives.
    pub async fn check(&self) -> Result<(), DependencyFailure> {
        match self {
            Self::Filesystem { root } => {
                if root.is_dir() {
                    return Ok(());
                }
                if root.exists() {
                    // There is something there, but it is not the store's directory.
                    return Err(DependencyFailure::Misconfigured);
                }
                Err(DependencyFailure::Unreachable)
            }
            Self::S3 { client, bucket } => {
                // `head_bucket` is the cheapest call that says at once
                // that the bucket exists and that the credentials work. The bucket is not
                // listed: a listing would download the object catalog,
                // and readiness does not need the objects.
                match client.head_bucket().bucket(bucket).send().await {
                    Ok(_) => Ok(()),
                    Err(error) => {
                        // The code says whether the bucket is **not** ours or
                        // whether it could not be reached. What is never said is
                        // why: the driver error can carry the
                        // endpoint and the credential.
                        let status = error
                            .raw_response()
                            .map(|response| response.status().as_u16());
                        Err(match status {
                            Some(403) | Some(404) => DependencyFailure::Misconfigured,
                            _ => DependencyFailure::Unreachable,
                        })
                    }
                }
            }
        }
    }
}

/// The active profile's probe: the dependencies the deployment requires and
/// how each one is checked.
///
/// The ones that are not here are not required: a dependency the profile does not
/// require cannot make readiness fail.
pub struct ProfileProbe {
    content: Option<ContentCheck>,
    graph: Option<Arc<dyn GraphHealthPort>>,
}

impl ProfileProbe {
    /// A profile that requires nothing.
    pub fn new() -> Self {
        Self {
            content: None,
            graph: None,
        }
    }

    /// The profile requires the graph authority (Dgraph). The check is the
    /// port's own readiness: reachable, ACL login accepted and one
    /// authenticated read-only round-trip; no data is scanned.
    pub fn with_graph(mut self, graph: Arc<dyn GraphHealthPort>) -> Self {
        self.graph = Some(graph);
        self
    }

    /// The profile requires the content store.
    pub fn with_content(mut self, content: ContentCheck) -> Self {
        self.content = Some(content);
        self
    }
}

impl Default for ProfileProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ReadinessProbe for ProfileProbe {
    fn required(&self) -> Vec<Dependency> {
        let mut required = Vec::new();
        if self.graph.is_some() {
            required.push(Dependency::Graph);
        }
        if self.content.is_some() {
            required.push(Dependency::ContentStore);
        }
        required
    }

    async fn check(&self, dependency: Dependency) -> Result<(), DependencyFailure> {
        match dependency {
            Dependency::ContentStore => {
                let Some(content) = self.content.as_ref() else {
                    return Err(DependencyFailure::Misconfigured);
                };
                content.check().await
            }
            Dependency::Graph => {
                let Some(graph) = self.graph.as_ref() else {
                    return Err(DependencyFailure::Misconfigured);
                };
                // The driver error can carry the endpoint and the user, so
                // only its class travels, never its text.
                match graph.ready().await {
                    Ok(readiness) if readiness.authenticated_query_served => Ok(()),
                    Ok(_) => Err(DependencyFailure::Unreachable),
                    Err(error) => Err(graph_failure(&error)),
                }
            }
            Dependency::TrustPolicy => Err(DependencyFailure::Misconfigured),
        }
    }
}

/// Classify a graph client error into the coarse readiness failure.
fn graph_failure(error: &GraphClientError) -> DependencyFailure {
    match error {
        GraphClientError::LoginRejected { .. } | GraphClientError::LoginWithoutToken => {
            DependencyFailure::Misconfigured
        }
        GraphClientError::ConnectTimeout { .. } | GraphClientError::RequestTimeout { .. } => {
            DependencyFailure::Timeout
        }
        GraphClientError::Request { source, .. }
            // gRPC codes 16 (Unauthenticated) and 7 (PermissionDenied): the
            // ACL refused the session, which is configuration, not outage.
            if matches!(source.code() as i32, 16 | 7) =>
        {
            DependencyFailure::Misconfigured
        }
        _ => DependencyFailure::Unreachable,
    }
}

/// The probe of a deployment that requires no dependency.
///
/// It is the state's default probe: a deployment that has not declared
/// that it requires anything is ready, and saying so is better than inventing a
/// dependency to check.
pub struct NothingRequired;

#[async_trait]
impl ReadinessProbe for NothingRequired {
    fn required(&self) -> Vec<Dependency> {
        Vec::new()
    }

    async fn check(&self, _dependency: Dependency) -> Result<(), DependencyFailure> {
        Err(DependencyFailure::Misconfigured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fake probe that counts the checks and fails where
    /// it is told to.
    struct Counting {
        required: Vec<Dependency>,
        checks: AtomicUsize,
        answer: Result<(), DependencyFailure>,
    }

    impl Counting {
        fn new(required: Vec<Dependency>, answer: Result<(), DependencyFailure>) -> Self {
            Self {
                required,
                checks: AtomicUsize::new(0),
                answer,
            }
        }

        fn checks(&self) -> usize {
            self.checks.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl ReadinessProbe for Counting {
        fn required(&self) -> Vec<Dependency> {
            self.required.clone()
        }

        async fn check(&self, _dependency: Dependency) -> Result<(), DependencyFailure> {
            self.checks.fetch_add(1, Ordering::Relaxed);
            self.answer
        }
    }

    /// A probe that never answers.
    struct Silent;

    #[async_trait]
    impl ReadinessProbe for Silent {
        fn required(&self) -> Vec<Dependency> {
            vec![Dependency::ContentStore]
        }

        async fn check(&self, _dependency: Dependency) -> Result<(), DependencyFailure> {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn una_dependencia_que_no_contesta_produce_timeout_y_no_un_500() {
        let probe = CachedProbe::with_ttl(Arc::new(Silent), Duration::from_millis(10));
        let report = probe.report().await;
        assert!(!report.ready);
        assert_eq!(
            report.failing,
            vec![(Dependency::ContentStore, DependencyFailure::Timeout)]
        );
    }

    #[tokio::test]
    async fn una_dependencia_opcional_que_no_se_exige_no_puede_fallar_el_readiness() {
        // The probe requires the graph and the check fails: the verdict
        // is that it is not ready. The dependency that is not required --the
        // content store-- does not appear and cannot fail.
        let probe = CachedProbe::with_ttl(
            Arc::new(Counting::new(
                vec![Dependency::Graph],
                Err(DependencyFailure::Unreachable),
            )),
            Duration::from_millis(10),
        );
        let report = probe.report().await;
        assert!(!report.ready);
        assert_eq!(
            report.failing,
            vec![(Dependency::Graph, DependencyFailure::Unreachable)]
        );
        assert_eq!(probe.required(), vec![Dependency::Graph]);
    }

    #[tokio::test]
    async fn el_cache_corto_no_repite_la_comprobacion() {
        let counting = Arc::new(Counting::new(vec![Dependency::ContentStore], Ok(())));
        let probe = CachedProbe::with_ttl(counting.clone(), Duration::from_secs(30));
        let first = probe.report().await;
        let second = probe.report().await;
        assert!(first.ready);
        assert!(second.ready);
        assert_eq!(counting.checks(), 1, "el cache corto no vuelve a comprobar");
    }

    #[tokio::test]
    async fn el_probe_no_consulta_mas_de_una_vez_cada_dependencia() {
        let counting = Arc::new(Counting::new(
            vec![Dependency::Graph, Dependency::ContentStore],
            Ok(()),
        ));
        let probe = CachedProbe::with_ttl(counting.clone(), Duration::from_millis(10));
        let report = probe.report().await;
        assert!(report.ready);
        assert_eq!(counting.checks(), 2, "una comprobacion por dependencia");
    }
}
