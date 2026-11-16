//! Las piezas de estado de la cache del proxy: coalescencia de descargas,
//! cache negativa, metricas y el error que se comparte entre peticiones.
//!
//! La **decision** de cache (frescura, revalidacion, offline, cuarentena) es
//! pura y vive en `hodei_catalog::proxy_cache`. Aqui solo esta lo que
//! necesita memoria de proceso:
//!
//! - [`SingleFlight`]: peticiones concurrentes por la misma ruta comparten
//!   una sola descarga. Una descarga por ruta, con el limite global de
//!   salidas simultaneas que ya impone el cliente de salida.
//! - [`NegativeCache`]: una ruta que el upstream dijo que no existe no se
//!   vuelve a preguntar durante un tiempo corto.
//! - [`ProxyMetrics`]: contadores de lo que paso con cada peticion.
//!
//! Nada de esto es fuente de verdad: si el proceso se reinicia, lo unico
//! que se pierde es una optimizacion. La frescura y la procedencia salen
//! siempre del libro de recibos.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::watch;

/// Que papel jugo una peticion en una descarga compartida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flight<V> {
    /// Esta peticion hizo el trabajo.
    Led(V),
    /// Esta peticion esperaba y comparte el resultado de otra.
    Followed(V),
}

impl<V> Flight<V> {
    /// El resultado, sea cual sea el papel.
    pub fn into_inner(self) -> V {
        match self {
            Self::Led(value) | Self::Followed(value) => value,
        }
    }

    /// Si esta peticion compartio el trabajo de otra.
    pub fn is_followed(&self) -> bool {
        matches!(self, Self::Followed(_))
    }
}

/// Coalescencia: una sola ejecucion en vuelo por clave.
///
/// El primero en llegar (el *lider*) ejecuta el trabajo; los que llegan
/// mientras dura esperan y reciben una copia de su resultado, con exito o
/// con error. Si el lider se cancela (el cliente cierra la conexion), los
/// seguidores no se quedan colgados ni heredan la cancelacion: uno de ellos
/// pasa a ser el nuevo lider.
///
/// El resultado compartido es el del lider, incluido a nombre de quien
/// quedo el recibo: todos los que esperaron ya tenian permiso de leer esa
/// ruta, porque la autorizacion va antes de llegar aqui.
pub struct SingleFlight<K, V> {
    inflight: Mutex<HashMap<K, watch::Receiver<Option<V>>>>,
}

impl<K, V> Default for SingleFlight<K, V> {
    fn default() -> Self {
        Self {
            inflight: Mutex::new(HashMap::new()),
        }
    }
}

impl<K, V> std::fmt::Debug for SingleFlight<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingleFlight").finish_non_exhaustive()
    }
}

/// Quita la entrada si el lider termina sin publicar (cancelacion o panico).
struct LeaderGuard<'a, K: Eq + Hash, V> {
    flights: &'a SingleFlight<K, V>,
    key: Option<K>,
}

impl<K: Eq + Hash, V> LeaderGuard<'_, K, V> {
    /// Quita la entrada y la deja sin efecto en el `Drop`.
    fn release(&mut self) {
        if let Some(key) = self.key.take() {
            self.flights.remove(&key);
        }
    }
}

impl<K: Eq + Hash, V> Drop for LeaderGuard<'_, K, V> {
    fn drop(&mut self) {
        self.release();
    }
}

impl<K: Eq + Hash, V> SingleFlight<K, V> {
    fn remove(&self, key: &K) {
        // Un mutex envenenado solo puede venir de un panico con el cerrojo
        // cogido; el mapa sigue siendo valido.
        let mut inflight = self
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inflight.remove(key);
    }

    /// Cuantas ejecuciones hay en vuelo.
    pub fn in_flight(&self) -> usize {
        self.inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

impl<K: Eq + Hash + Clone, V: Clone> SingleFlight<K, V> {
    /// Una coalescencia vacia.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ejecuta `work` si nadie lo esta haciendo para `key`; si no, espera a
    /// quien lo hace y devuelve su resultado.
    pub async fn run<F, Fut>(&self, key: K, work: F) -> Flight<V>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = V>,
    {
        let mut work = Some(work);
        loop {
            enum Role<V> {
                Leader(watch::Sender<Option<V>>),
                Follower(watch::Receiver<Option<V>>),
            }
            let role = {
                let mut inflight = self
                    .inflight
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match inflight.get(&key) {
                    Some(receiver) => Role::Follower(receiver.clone()),
                    None => {
                        let (sender, receiver) = watch::channel(None);
                        inflight.insert(key.clone(), receiver);
                        Role::Leader(sender)
                    }
                }
            };
            match role {
                Role::Leader(sender) => {
                    let mut guard = LeaderGuard {
                        flights: self,
                        key: Some(key.clone()),
                    };
                    // Un lider termina la funcion: el trabajo solo se pide una vez.
                    let Some(work) = work.take() else {
                        continue;
                    };
                    let value = work().await;
                    // Primero se retira la entrada: quien llegue ahora abre
                    // una descarga nueva en vez de leer una ya terminada.
                    guard.release();
                    // Sin seguidores no hay nadie a quien avisar y `send`
                    // falla: no es un error.
                    let _ = sender.send(Some(value.clone()));
                    return Flight::Led(value);
                }
                Role::Follower(mut receiver) => {
                    // Si el lider se cancelo sin resultado, `wait_for`
                    // falla y se reintenta: quien llegue primero lo releva.
                    if let Ok(value) = receiver.wait_for(|value| value.is_some()).await
                        && let Some(value) = value.clone()
                    {
                        return Flight::Followed(value);
                    }
                }
            }
        }
    }
}

/// La clave de una descarga o de una ausencia: cuenta, repositorio y ruta.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathKey {
    /// La cuenta propietaria.
    pub account: String,
    /// La clave del repositorio.
    pub repository: String,
    /// La ruta.
    pub asset_path: String,
}

impl PathKey {
    /// La clave de una ruta de un repositorio de una cuenta.
    pub fn of(account: &str, repository: &str, asset_path: &str) -> Self {
        Self {
            account: account.to_string(),
            repository: repository.to_string(),
            asset_path: asset_path.to_string(),
        }
    }
}

/// Cuantas ausencias recuerda como maximo la cache negativa.
const NEGATIVE_CAPACITY: usize = 4096;

/// La cache de lo que el upstream dijo que no existe.
///
/// En memoria y acotada. La clave lleva la **version de la configuracion**
/// del repositorio: cambiar el upstream, los scopes o la politica descarta
/// de hecho todas las ausencias anteriores, porque la pregunta ya no es la
/// misma. Solo recuerda ausencias de rutas **sin contenido**: una ruta que
/// el catalogo ya sirve nunca se esconde tras una ausencia.
#[derive(Debug, Default)]
pub struct NegativeCache {
    entries: Mutex<HashMap<(PathKey, u64), Instant>>,
}

impl NegativeCache {
    /// Una cache vacia.
    pub fn new() -> Self {
        Self::default()
    }

    /// Si hay una ausencia vigente para esa ruta y version de configuracion.
    pub fn contains(&self, key: &PathKey, config_version: u64, now: Instant) -> bool {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries
            .get(&(key.clone(), config_version))
            .is_some_and(|expires| *expires > now)
    }

    /// Recuerda una ausencia durante `ttl`. Con la cache llena (tras purgar
    /// las vencidas) no guarda nada: es una optimizacion, no un registro.
    pub fn remember(&self, key: &PathKey, config_version: u64, ttl: Duration, now: Instant) {
        if ttl.is_zero() {
            return;
        }
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.len() >= NEGATIVE_CAPACITY {
            entries.retain(|_, expires| *expires > now);
            if entries.len() >= NEGATIVE_CAPACITY {
                return;
            }
        }
        entries.insert((key.clone(), config_version), now + ttl);
    }

    /// Olvida la ausencia de una ruta, sea cual sea la version.
    pub fn forget(&self, key: &PathKey) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|(stored, _), _| stored != key);
    }

    /// Cuantas ausencias hay guardadas (vencidas incluidas).
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Si no hay ninguna.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Los contadores del proxy. Atomicos y sin etiquetas: son un pulso del
/// proceso, no una serie temporal; el rastro (`tracing`) lleva el detalle.
#[derive(Debug, Default)]
pub struct ProxyMetrics {
    hits: AtomicU64,
    misses: AtomicU64,
    revalidated: AtomicU64,
    refreshed: AtomicU64,
    stale: AtomicU64,
    negative_hits: AtomicU64,
    offline_hits: AtomicU64,
    offline_misses: AtomicU64,
    quarantined: AtomicU64,
    coalesced: AtomicU64,
    upstream_requests: AtomicU64,
    upstream_errors: AtomicU64,
}

/// Una lectura de los contadores en un instante.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    /// Servido de la cache sin salir.
    pub hits: u64,
    /// No estaba y se trajo.
    pub misses: u64,
    /// Estaba caducado y el upstream lo confirmo.
    pub revalidated: u64,
    /// Estaba caducado y el upstream entrego bytes nuevos.
    pub refreshed: u64,
    /// El upstream fallo y se sirvio lo que habia, dentro de la ventana.
    pub stale: u64,
    /// Una ausencia recordada ahorro una pregunta.
    pub negative_hits: u64,
    /// Servido en modo offline.
    pub offline_hits: u64,
    /// Rechazado en modo offline por no estar en la cache.
    pub offline_misses: u64,
    /// Retenido por entregar bytes distintos a los de un contenido inmutable.
    pub quarantined: u64,
    /// Peticiones que compartieron la descarga de otra.
    pub coalesced: u64,
    /// Salidas al upstream.
    pub upstream_requests: u64,
    /// Salidas al upstream que fallaron.
    pub upstream_errors: u64,
}

macro_rules! counter {
    ($record:ident, $field:ident) => {
        /// Suma uno.
        pub fn $record(&self) {
            self.$field.fetch_add(1, Ordering::Relaxed);
        }
    };
}

impl ProxyMetrics {
    counter!(record_hit, hits);
    counter!(record_miss, misses);
    counter!(record_revalidated, revalidated);
    counter!(record_refreshed, refreshed);
    counter!(record_stale, stale);
    counter!(record_negative_hit, negative_hits);
    counter!(record_offline_hit, offline_hits);
    counter!(record_offline_miss, offline_misses);
    counter!(record_quarantined, quarantined);
    counter!(record_coalesced, coalesced);
    counter!(record_upstream_request, upstream_requests);
    counter!(record_upstream_error, upstream_errors);

    /// Una lectura de todos los contadores.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        MetricsSnapshot {
            hits: read(&self.hits),
            misses: read(&self.misses),
            revalidated: read(&self.revalidated),
            refreshed: read(&self.refreshed),
            stale: read(&self.stale),
            negative_hits: read(&self.negative_hits),
            offline_hits: read(&self.offline_hits),
            offline_misses: read(&self.offline_misses),
            quarantined: read(&self.quarantined),
            coalesced: read(&self.coalesced),
            upstream_requests: read(&self.upstream_requests),
            upstream_errors: read(&self.upstream_errors),
        }
    }
}

/// Por que fallo una resolucion, en una forma que se puede compartir entre
/// las peticiones que esperaron la misma descarga.
///
/// No lleva la URL ni la respuesta del upstream: el cliente no tiene por
/// que ver la red interna del servicio.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProxyFailure {
    /// El upstream no tiene ese contenido.
    #[error("the upstream does not have that content")]
    NotFound,
    /// La ruta cae fuera de los alcances del upstream.
    #[error("the path is outside the scopes of the upstream")]
    OutOfScope,
    /// Hay demasiadas salidas a la vez.
    #[error("too many fetches are in flight")]
    Busy,
    /// El upstream no respondio a tiempo.
    #[error("the upstream timed out")]
    Timeout,
    /// La politica de salida no deja ir al upstream.
    #[error("the upstream is not reachable from this service")]
    NotReachable,
    /// El contenido no se pudo verificar contra lo que declaro el upstream.
    #[error("the upstream content could not be verified")]
    Unverified,
    /// El contenido llego incompleto o excede el tope.
    #[error("the upstream content is incomplete or too large")]
    Incomplete,
    /// El upstream no entrego (error de red, estado inesperado, credencial).
    #[error("the upstream did not deliver")]
    UpstreamFailed,
    /// El servicio no sale a la red.
    #[error("the service is offline")]
    Offline,
    /// El almacen o el libro fallaron: no es culpa del upstream.
    #[error("the service could not store the content")]
    Local(LocalFailure),
}

/// Un fallo propio del servicio, sin detalle que filtrar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFailure {
    /// El almacen o el catalogo no estan disponibles.
    Unavailable,
    /// Algo roto por dentro.
    Internal,
    /// El catalogo rechazo la publicacion por una condicion.
    Rejected,
}

impl ProxyFailure {
    /// Si el fallo es del upstream: lo que cuenta como error de salida y lo
    /// unico que deja servir un contenido caducado dentro de la ventana
    /// `stale_if_error`.
    ///
    /// Que el upstream diga `404` no cuenta: es una respuesta, no un error,
    /// y el contenido que el upstream retiro no se sigue presentando como
    /// vigente.
    pub fn is_upstream_error(&self) -> bool {
        matches!(
            self,
            Self::Busy
                | Self::Timeout
                | Self::NotReachable
                | Self::Unverified
                | Self::Incomplete
                | Self::UpstreamFailed
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use tokio::sync::Notify;

    use super::*;

    #[tokio::test]
    async fn concurrent_requests_for_one_key_share_a_single_execution() {
        let flights: Arc<SingleFlight<&'static str, u32>> = Arc::new(SingleFlight::new());
        let runs = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());

        let mut tasks = Vec::new();
        for _ in 0..8 {
            let flights = flights.clone();
            let runs = runs.clone();
            let release = release.clone();
            tasks.push(tokio::spawn(async move {
                flights
                    .run("path", || async move {
                        runs.fetch_add(1, Ordering::SeqCst);
                        release.notified().await;
                        7
                    })
                    .await
            }));
        }
        // Deja que todas lleguen y se pongan a esperar al lider.
        while flights.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        release.notify_one();

        let mut led = 0;
        let mut followed = 0;
        for task in tasks {
            match task.await.expect("the task finishes") {
                Flight::Led(value) => {
                    assert_eq!(value, 7);
                    led += 1;
                }
                Flight::Followed(value) => {
                    assert_eq!(value, 7);
                    followed += 1;
                }
            }
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1, "only one execution");
        assert_eq!((led, followed), (1, 7));
        assert_eq!(flights.in_flight(), 0, "the entry is released");
    }

    #[tokio::test]
    async fn distinct_keys_do_not_wait_for_each_other() {
        let flights: SingleFlight<&'static str, u32> = SingleFlight::new();
        let a = flights.run("a", || async { 1 });
        let b = flights.run("b", || async { 2 });
        let (a, b) = tokio::join!(a, b);
        assert_eq!((a.into_inner(), b.into_inner()), (1, 2));
    }

    #[tokio::test]
    async fn a_finished_flight_does_not_serve_a_later_request() {
        let flights: SingleFlight<&'static str, u32> = SingleFlight::new();
        let first = flights.run("path", || async { 1 }).await;
        let second = flights.run("path", || async { 2 }).await;
        assert_eq!(first, Flight::Led(1));
        assert_eq!(second, Flight::Led(2), "a new request runs again");
    }

    #[tokio::test]
    async fn an_error_is_shared_like_any_other_result() {
        let flights: Arc<SingleFlight<&'static str, Result<u32, ProxyFailure>>> =
            Arc::new(SingleFlight::new());
        let release = Arc::new(Notify::new());
        let leader = {
            let flights = flights.clone();
            let release = release.clone();
            tokio::spawn(async move {
                flights
                    .run("path", || async move {
                        release.notified().await;
                        Err(ProxyFailure::Timeout)
                    })
                    .await
            })
        };
        while flights.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
        let follower = {
            let flights = flights.clone();
            tokio::spawn(async move { flights.run("path", || async { Ok(1) }).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        release.notify_one();
        assert_eq!(
            leader.await.expect("leader"),
            Flight::Led(Err(ProxyFailure::Timeout))
        );
        assert_eq!(
            follower.await.expect("follower"),
            Flight::Followed(Err(ProxyFailure::Timeout))
        );
    }

    #[tokio::test]
    async fn a_cancelled_leader_hands_the_work_to_a_follower() {
        let flights: Arc<SingleFlight<&'static str, u32>> = Arc::new(SingleFlight::new());
        let never = Arc::new(Notify::new());
        let leader = {
            let flights = flights.clone();
            let never = never.clone();
            tokio::spawn(async move {
                flights
                    .run("path", || async move {
                        never.notified().await;
                        1
                    })
                    .await
            })
        };
        while flights.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
        let follower = {
            let flights = flights.clone();
            tokio::spawn(async move { flights.run("path", || async { 2 }).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        // El cliente del lider se va: su peticion se cancela.
        leader.abort();
        let outcome = tokio::time::timeout(Duration::from_secs(5), follower)
            .await
            .expect("the follower does not hang")
            .expect("follower");
        assert_eq!(outcome, Flight::Led(2), "the follower took over");
        assert_eq!(flights.in_flight(), 0);
    }

    #[test]
    fn an_absence_lasts_its_ttl_and_only_for_its_config_version() {
        let negatives = NegativeCache::new();
        let key = PathKey::of("acme", "central", "org/missing.jar");
        let now = Instant::now();
        negatives.remember(&key, 3, Duration::from_secs(30), now);

        assert!(negatives.contains(&key, 3, now + Duration::from_secs(29)));
        assert!(!negatives.contains(&key, 3, now + Duration::from_secs(30)));
        assert!(
            !negatives.contains(&key, 4, now),
            "a new config version asks again"
        );
        assert!(
            !negatives.contains(&PathKey::of("other", "central", "org/missing.jar"), 3, now),
            "another account does not share it"
        );
    }

    #[test]
    fn a_zero_ttl_remembers_nothing_and_forget_clears_every_version() {
        let negatives = NegativeCache::new();
        let key = PathKey::of("acme", "central", "a");
        let now = Instant::now();
        negatives.remember(&key, 1, Duration::ZERO, now);
        assert!(negatives.is_empty());

        negatives.remember(&key, 1, Duration::from_secs(60), now);
        negatives.remember(&key, 2, Duration::from_secs(60), now);
        negatives.forget(&key);
        assert!(negatives.is_empty());
    }

    #[test]
    fn the_negative_cache_is_bounded_and_purges_expired_entries_first() {
        let negatives = NegativeCache::new();
        let now = Instant::now();
        for index in 0..NEGATIVE_CAPACITY {
            let key = PathKey::of("acme", "central", &format!("p{index}"));
            negatives.remember(&key, 1, Duration::from_secs(10), now);
        }
        assert_eq!(negatives.len(), NEGATIVE_CAPACITY);

        // Llena y sin vencidas: no entra nada mas.
        let extra = PathKey::of("acme", "central", "extra");
        negatives.remember(&extra, 1, Duration::from_secs(10), now);
        assert!(!negatives.contains(&extra, 1, now));

        // Con las anteriores vencidas, se purgan y entra.
        let later = now + Duration::from_secs(11);
        negatives.remember(&extra, 1, Duration::from_secs(10), later);
        assert!(negatives.contains(&extra, 1, later));
        assert_eq!(negatives.len(), 1);
    }

    #[test]
    fn the_counters_start_at_zero_and_count_what_happens() {
        let metrics = ProxyMetrics::default();
        assert_eq!(metrics.snapshot(), MetricsSnapshot::default());
        metrics.record_hit();
        metrics.record_hit();
        metrics.record_miss();
        metrics.record_stale();
        metrics.record_upstream_error();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.hits, 2);
        assert_eq!(snapshot.misses, 1);
        assert_eq!(snapshot.stale, 1);
        assert_eq!(snapshot.upstream_errors, 1);
        assert_eq!(snapshot.quarantined, 0);
    }

    #[test]
    fn only_upstream_failures_allow_a_stale_answer() {
        for failure in [
            ProxyFailure::Busy,
            ProxyFailure::Timeout,
            ProxyFailure::NotReachable,
            ProxyFailure::Unverified,
            ProxyFailure::Incomplete,
            ProxyFailure::UpstreamFailed,
        ] {
            assert!(failure.is_upstream_error(), "{failure:?}");
        }
        for failure in [
            ProxyFailure::NotFound,
            ProxyFailure::OutOfScope,
            ProxyFailure::Offline,
            ProxyFailure::Local(LocalFailure::Internal),
        ] {
            assert!(!failure.is_upstream_error(), "{failure:?}");
        }
    }
}
