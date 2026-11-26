//! The remote proxy: resolve content against an upstream without opening the
//! service's internal network.
//!
//! - [`egress`]: where egress is allowed (allow-list and addresses).
//! - [`fetch`]: the client that goes out, and the check of what arrives.
//! - [`resolution`]: fetch, verify, store and record provenance.
//! - [`cache`]: download coalescing, negative cache and metrics.
//! - [`token`]: the tokens an upstream demands and that have already been requested.
//! - [`dgraph_ledger`]: the durable receipt ledger over the graph (HSP-196).

pub mod cache;
pub mod dgraph_ledger;
pub mod egress;
pub mod fetch;
pub mod resolution;
pub mod token;

use std::sync::Arc;

use hodei_catalog::ProvenanceLedger;

use self::cache::{NegativeCache, PathKey, ProxyFailure, ProxyMetrics, SingleFlight};
use self::fetch::ProxyFetcher;
use self::resolution::Outcome;
use self::token::TokenCache;

/// What requests waiting for the same download share.
pub type FlightResult = Result<Outcome, ProxyFailure>;

/// What the service needs to resolve against an upstream: the egress
/// client, the ledger where provenance is recorded and the cache's process
/// state.
#[derive(Clone)]
pub struct ProxyPlane {
    /// The egress client.
    pub fetcher: ProxyFetcher,
    /// The provenance receipt ledger.
    pub ledger: Arc<dyn ProvenanceLedger>,
    /// The in-flight downloads, one per path.
    pub flights: Arc<SingleFlight<(PathKey, u64), FlightResult>>,
    /// The paths the upstream said do not exist.
    pub negatives: Arc<NegativeCache>,
    /// The counters.
    pub metrics: Arc<ProxyMetrics>,
    /// The tokens of upstreams that require authentication.
    pub tokens: Arc<TokenCache>,
}

impl ProxyPlane {
    /// A proxy plane with an empty cache.
    pub fn new(fetcher: ProxyFetcher, ledger: Arc<dyn ProvenanceLedger>) -> Self {
        Self {
            fetcher,
            ledger,
            flights: Arc::new(SingleFlight::new()),
            negatives: Arc::new(NegativeCache::new()),
            metrics: Arc::new(ProxyMetrics::default()),
            tokens: Arc::new(TokenCache::new()),
        }
    }

    /// Whether the entire service has no egress to the network.
    pub fn is_offline(&self) -> bool {
        self.fetcher.is_offline()
    }
}

impl std::fmt::Debug for ProxyPlane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyPlane")
            .field("fetcher", &self.fetcher)
            .finish_non_exhaustive()
    }
}
