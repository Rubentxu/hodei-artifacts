//! El proxy remoto: resolver contenido contra un upstream sin abrir la red
//! interna del servicio.
//!
//! - [`egress`]: a donde se puede salir (allow-list y direcciones).
//! - [`fetch`]: el cliente que sale, y la comprobacion de lo que llega.
//! - [`resolution`]: traer, verificar, guardar y dejar constancia.
//! - [`cache`]: coalescencia de descargas, cache negativa y metricas.
//! - [`token`]: los tokens que un upstream exige y ya se han pedido.
//! - [`dgraph_ledger`]: el libro de recibos durable sobre el grafo (HSP-196).

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

/// Lo que comparten las peticiones que esperan la misma descarga.
pub type FlightResult = Result<Outcome, ProxyFailure>;

/// Lo que el servicio necesita para resolver contra un upstream: el
/// cliente de salida, el libro donde queda la procedencia y el estado de
/// proceso de la cache.
#[derive(Clone)]
pub struct ProxyPlane {
    /// El cliente de salida.
    pub fetcher: ProxyFetcher,
    /// El libro de recibos de procedencia.
    pub ledger: Arc<dyn ProvenanceLedger>,
    /// Las descargas en vuelo, una por ruta.
    pub flights: Arc<SingleFlight<(PathKey, u64), FlightResult>>,
    /// Las rutas que el upstream dijo que no existen.
    pub negatives: Arc<NegativeCache>,
    /// Los contadores.
    pub metrics: Arc<ProxyMetrics>,
    /// Los tokens de los upstreams que exigen autenticacion.
    pub tokens: Arc<TokenCache>,
}

impl ProxyPlane {
    /// Un plano de proxy con la cache vacia.
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

    /// Si el servicio entero esta sin salida a la red.
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
