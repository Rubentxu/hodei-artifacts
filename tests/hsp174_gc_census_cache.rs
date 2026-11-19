//! El dry-run comparte censo (HSP-174): ni veinte censos por veinte
//! peticiones, ni veinte planes apilados en memoria.
//!
//! El almacen y el libro son mocks contadores: cuentan cuantas veces el
//! servicio mira el mundo. Lo que se prueba es la contabilidad, no el
//! veredicto --el veredicto ya lo prueban los tests del servicio sobre un
//! almacen real--; aqui se prueba que N lecturas del plan valen UN censo
//! mientras el mundo no cambia, y que lo que borra se niega a leer un
//! plano compartido.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use hodei_artifacts_api::gc::{GcService, Verdict};
use hodei_catalog::{
    CatalogError, ContentHold, ContentLedger, ContentRoot, GcDeletion, GcRun, HoldScope, RootKind,
    RunState,
};
use hodei_evidence::ContentDigest;
use hodei_hub::{BlobError, BlobReclaim, BlobStore, ReadPolicy, Reclaimed, StoredBlob};

const GRACE: Duration = Duration::from_secs(3600);

fn digest(seed: char) -> ContentDigest {
    ContentDigest::sha256(seed.to_string().repeat(64))
}

fn stored(seed: char) -> StoredBlob {
    StoredBlob {
        digest: digest(seed),
        length: 10,
        age: Duration::from_secs(60),
    }
}

fn root(seed: char) -> ContentRoot {
    ContentRoot {
        kind: RootKind::Asset,
        account: "acme".to_string(),
        repository: "app".to_string(),
        subject: "blobs/thing".to_string(),
        digest: digest(seed),
    }
}

#[derive(Default)]
struct Counters {
    list: AtomicUsize,
    roots: AtomicUsize,
    holds: AtomicUsize,
}

struct Ledger {
    counters: Arc<Counters>,
    roots: Vec<ContentRoot>,
    holds: std::sync::Mutex<Vec<ContentHold>>,
    runs: std::sync::Mutex<Vec<GcRun>>,
    tombstones: std::sync::Mutex<Vec<GcDeletion>>,
}

impl Ledger {
    fn new(roots: Vec<ContentRoot>) -> Arc<Self> {
        Arc::new(Self {
            counters: Arc::new(Counters::default()),
            roots,
            holds: std::sync::Mutex::new(Vec::new()),
            runs: std::sync::Mutex::new(Vec::new()),
            tombstones: std::sync::Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl ContentLedger for Ledger {
    async fn content_roots(&self) -> Result<Vec<ContentRoot>, CatalogError> {
        self.counters.roots.fetch_add(1, Ordering::SeqCst);
        Ok(self.roots.clone())
    }
    async fn holds(&self) -> Result<Vec<ContentHold>, CatalogError> {
        self.counters.holds.fetch_add(1, Ordering::SeqCst);
        Ok(self.holds.lock().unwrap().clone())
    }
    async fn place_hold(&self, hold: &ContentHold) -> Result<(), CatalogError> {
        let mut holds = self.holds.lock().unwrap();
        holds.retain(|existing| existing.id != hold.id);
        holds.push(hold.clone());
        Ok(())
    }
    async fn release_hold(&self, id: &str) -> Result<bool, CatalogError> {
        let mut holds = self.holds.lock().unwrap();
        let before = holds.len();
        holds.retain(|hold| hold.id != id);
        Ok(holds.len() != before)
    }
    async fn pending_run(&self) -> Result<Option<GcRun>, CatalogError> {
        Ok(self
            .runs
            .lock()
            .unwrap()
            .iter()
            .find(|run| run.state == RunState::Marked)
            .cloned())
    }
    async fn last_swept_run(&self) -> Result<Option<GcRun>, CatalogError> {
        Ok(self
            .runs
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|run| run.state == RunState::Swept)
            .cloned())
    }
    async fn open_run(&self, run: &GcRun) -> Result<(), CatalogError> {
        let mut runs = self.runs.lock().unwrap();
        runs.retain(|existing| existing.id != run.id);
        runs.push(run.clone());
        Ok(())
    }
    async fn complete_run(
        &self,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: chrono::DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        for run in self
            .runs
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|run| run.id == run_id)
        {
            run.state = RunState::Swept;
            run.deleted = deleted;
            run.bytes_freed = bytes_freed;
            run.swept_at = Some(at);
        }
        Ok(())
    }
    async fn record_deletion(&self, deletion: &GcDeletion) -> Result<(), CatalogError> {
        let mut tombstones = self.tombstones.lock().unwrap();
        tombstones.retain(|existing| existing.digest != deletion.digest);
        tombstones.push(deletion.clone());
        Ok(())
    }
    async fn deletion_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<GcDeletion>, CatalogError> {
        Ok(self
            .tombstones
            .lock()
            .unwrap()
            .iter()
            .find(|existing| &existing.digest == digest)
            .cloned())
    }
}

/// El almacen contador. `slow` se suena en cada listado para que las
/// llamadas concurrentes se apilen de verdad y no se solapen por
/// casualidad.
struct Store {
    counters: Arc<Counters>,
    slow: Duration,
}

impl Store {
    fn new(counters: Arc<Counters>, slow: Duration) -> Arc<Self> {
        Arc::new(Self { counters, slow })
    }
}

#[async_trait]
impl BlobReclaim for Store {
    async fn list_blobs(&self) -> Result<Vec<StoredBlob>, BlobError> {
        self.counters.list.fetch_add(1, Ordering::SeqCst);
        if !self.slow.is_zero() {
            tokio::time::sleep(self.slow).await;
        }
        Ok(vec![stored('a'), stored('b')])
    }
    async fn delete_if_older_than(
        &self,
        _digest: &ContentDigest,
        _grace: Duration,
    ) -> Result<Reclaimed, BlobError> {
        Ok(Reclaimed::Gone)
    }
}

#[async_trait]
impl BlobStore for Store {
    async fn put(&self, _bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        Err(BlobError::Unavailable(
            "the mock does not write".to_string(),
        ))
    }
    async fn get(&self, _digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        Err(BlobError::NotFound)
    }
    async fn exists(&self, _digest: &ContentDigest) -> Result<bool, BlobError> {
        Ok(true)
    }
    async fn begin(&self) -> Result<Box<dyn hodei_hub::BlobWriteSession>, BlobError> {
        Err(BlobError::Unavailable(
            "the mock does not write".to_string(),
        ))
    }
    async fn get_with(
        &self,
        _digest: &ContentDigest,
        _policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        Err(BlobError::NotFound)
    }
    async fn length(&self, _digest: &ContentDigest) -> Result<u64, BlobError> {
        Err(BlobError::NotFound)
    }
    async fn read_range(
        &self,
        _digest: &ContentDigest,
        _offset: u64,
        _length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        Err(BlobError::NotFound)
    }
    async fn verify(&self, _digest: &ContentDigest) -> Result<(), BlobError> {
        Ok(())
    }
    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        Some(self)
    }
}

fn service(roots: Vec<ContentRoot>, slow: Duration, grace: Duration) -> (Arc<Counters>, GcService) {
    let ledger = Ledger::new(roots);
    let counters = ledger.counters.clone();
    let store = Store::new(counters.clone(), slow);
    let service = GcService::new(ledger, store, grace).expect("service");
    (counters, service)
}

/// N lecturas del plan, un censo: mientras el mundo no cambia y la
/// ventana no caduca, el listado del almacen se pide una sola vez.
#[tokio::test]
async fn repeated_dry_runs_share_one_census() {
    let (counters, service) = service(vec![root('a')], Duration::ZERO, GRACE);
    for _ in 0..5 {
        let plan = service.dry_run().await.expect("plan");
        assert_eq!(plan.entries.len(), 2, "both blobs are accounted");
    }
    assert_eq!(counters.list.load(Ordering::SeqCst), 1);
    assert_eq!(counters.roots.load(Ordering::SeqCst), 1);
    assert_eq!(counters.holds.load(Ordering::SeqCst), 1);
}

/// Diez peticiones concurrentes, un vuelo: mientras una lee el mundo,
/// las demas esperan y recogen el mismo plano --no diez grafos apilados.
#[tokio::test]
async fn concurrent_dry_runs_take_flight_together() {
    let (counters, service) = service(vec![root('a')], Duration::from_millis(150), GRACE);
    let service = Arc::new(service);
    let mut tasks = Vec::new();
    for _ in 0..10 {
        let service = service.clone();
        tasks.push(tokio::spawn(async move {
            service.dry_run().await.expect("plan").entries.len()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.expect("task"), 2);
    }
    assert_eq!(
        counters.list.load(Ordering::SeqCst),
        1,
        "ten concurrent dry runs must not stack ten censuses"
    );
}

/// Lo que cambia el mundo se nota en el siguiente plano: la retencion
/// invalida el compartido y el censo se vuelve a pedir.
#[tokio::test]
async fn a_placed_hold_invalidates_the_shared_plan() {
    let (counters, service) = service(vec![root('a')], Duration::ZERO, GRACE);
    service.dry_run().await.expect("first");
    service
        .place_hold(
            "ops",
            HoldScope::Digest {
                digest: digest('a'),
            },
            "legal hold",
            None,
        )
        .await
        .expect("hold");
    let plan = service.dry_run().await.expect("second");
    assert_eq!(counters.list.load(Ordering::SeqCst), 2);
    let entry = plan
        .entries
        .iter()
        .find(|entry| entry.digest == digest('a').to_string())
        .expect("entry");
    assert!(
        matches!(entry.verdict, Verdict::Retained { .. }),
        "the hold must reach the fresh plan"
    );
}

/// Lo que borra no comparte plano: el barrido toma su propio censo y el
/// plan siguiente vuelve a mirar el mundo.
#[tokio::test]
async fn the_sweep_never_reads_the_shared_plan() {
    let (counters, service) = service(vec![root('a')], Duration::ZERO, GRACE);
    service.dry_run().await.expect("plan");
    assert_eq!(counters.list.load(Ordering::SeqCst), 1);
    service.sweep("ops", "acme").await.expect("sweep");
    // Mark and re-check: the sweep is two fresh censuses of its own.
    assert_eq!(
        counters.list.load(Ordering::SeqCst),
        3,
        "the sweep censuses"
    );
    service.dry_run().await.expect("plan again");
    assert_eq!(
        counters.list.load(Ordering::SeqCst),
        4,
        "after deleting, the world is new"
    );
}

/// La ventana caduca: con una gracia de un milisegundo el plano no puede
/// vivir mas que ella, y dos lecturas separadas son dos censos.
#[tokio::test]
async fn an_expired_window_censuses_again() {
    let (counters, service) = service(vec![root('a')], Duration::ZERO, Duration::from_millis(1));
    service.dry_run().await.expect("first");
    tokio::time::sleep(Duration::from_millis(20)).await;
    service.dry_run().await.expect("second");
    assert_eq!(counters.list.load(Ordering::SeqCst), 2);
}
