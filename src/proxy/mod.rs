//! El proxy remoto: resolver contenido contra un upstream sin abrir la red
//! interna del servicio.
//!
//! - [`egress`]: a donde se puede salir (allow-list y direcciones).
//! - [`fetch`]: el cliente que sale, y la comprobacion de lo que llega.
//! - [`resolution`]: traer, verificar, guardar y dejar constancia.

pub mod egress;
pub mod fetch;
pub mod resolution;

use std::sync::Arc;

use hodei_catalog::ProvenanceLedger;

use self::fetch::ProxyFetcher;

/// Lo que el servicio necesita para resolver contra un upstream: el
/// cliente de salida y el libro donde queda la procedencia.
#[derive(Clone)]
pub struct ProxyPlane {
    /// El cliente de salida.
    pub fetcher: ProxyFetcher,
    /// El libro de recibos de procedencia.
    pub ledger: Arc<dyn ProvenanceLedger>,
}

impl std::fmt::Debug for ProxyPlane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyPlane")
            .field("fetcher", &self.fetcher)
            .finish_non_exhaustive()
    }
}
