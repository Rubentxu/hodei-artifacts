//! El censo del contenido y el libro de la recoleccion.
//!
//! El catalogo es la unica fuente de **raices**: lo que dice que un blob
//! esta vivo es una fila real (un asset, una referencia, una version), no
//! un contador. Este modulo define lo que el recolector lee y escribe, y
//! no decide nada: quien decide es el plan del recolector, con estos
//! datos.
//!
//! # Lo que el libro guarda
//!
//! - Los **holds**: retenciones manuales (legal hold) sobre un digest, un
//!   repositorio o una cuenta. Pueden caducar.
//! - Las **pasadas**: la marca durable de una recoleccion. Una pasada
//!   marcada y no completada es lo que se reanuda tras un reinicio.
//! - Los **borrados**: el rastro de cada blob que se borro, con quien lo
//!   pidio y en que pasada.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::CatalogError;

/// De que fila del catalogo sale una raiz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootKind {
    /// La fila del asset: la ruta apunta a este contenido.
    Asset,
    /// La referencia al blob que la ruta sirve ahora.
    Reference,
    /// La version publicada: conserva el digest aunque el asset se mueva.
    Version,
    /// El `subject` de un manifiesto: un referrer cuelga de este digest.
    Subject,
}

impl RootKind {
    /// El nombre estable, tal y como sale en los informes.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Reference => "reference",
            Self::Version => "version",
            Self::Subject => "subject",
        }
    }
}

/// Una raiz: una fila real del catalogo que mantiene vivo un contenido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentRoot {
    /// De que fila sale.
    pub kind: RootKind,
    /// La cuenta propietaria.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// La ruta o la version que lo referencia.
    pub subject: String,
    /// El contenido que mantiene vivo.
    pub digest: ContentDigest,
}

/// A que alcanza una retencion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoldScope {
    /// Un contenido concreto.
    Digest {
        /// El contenido retenido.
        digest: ContentDigest,
    },
    /// Todo lo que referencia un repositorio.
    Repository {
        /// La cuenta.
        account: String,
        /// El repositorio.
        repository: String,
    },
    /// Todo lo que referencia una cuenta.
    Account {
        /// La cuenta.
        account: String,
    },
}

/// Una retencion: lo que alcanza no es reclamable mientras dure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentHold {
    /// El identificador de la retencion.
    pub id: String,
    /// A que alcanza.
    pub scope: HoldScope,
    /// Por que se puso.
    pub reason: String,
    /// Quien la puso.
    pub actor: String,
    /// Cuando se puso.
    pub created_at: DateTime<Utc>,
    /// Cuando caduca. Sin fecha, dura hasta que alguien la libere.
    pub expires_at: Option<DateTime<Utc>>,
}

impl ContentHold {
    /// Si la retencion sigue vigente en `now`.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_none_or(|expires| expires > now)
    }
}

/// En que punto esta una pasada de recoleccion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// Los candidatos estan marcados y el barrido no ha terminado.
    Marked,
    /// El barrido termino.
    Swept,
}

impl RunState {
    /// El nombre estable, tal y como va en la base.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Marked => "marked",
            Self::Swept => "swept",
        }
    }
}

/// Un candidato marcado en una pasada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCandidate {
    /// El contenido marcado.
    pub digest: ContentDigest,
    /// Los bytes que ocupa.
    pub length: u64,
}

/// Una pasada de recoleccion: la marca durable que sobrevive a un corte.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcRun {
    /// El identificador de la pasada.
    pub id: String,
    /// En que punto esta.
    pub state: RunState,
    /// El periodo de gracia con el que se marco, en segundos.
    pub grace_secs: u64,
    /// Quien la lanzo.
    pub actor: String,
    /// Los candidatos marcados.
    pub candidates: Vec<RunCandidate>,
    /// Cuando se marco.
    pub marked_at: DateTime<Utc>,
    /// Cuantos blobs se borraron.
    pub deleted: u64,
    /// Cuantos bytes se liberaron.
    pub bytes_freed: u64,
    /// Cuando termino el barrido.
    pub swept_at: Option<DateTime<Utc>>,
    /// El scope de autoridad bajo el que se marco esta pasada (modo
    /// coordinado, HSP-192). Vacio en el modo legacy.
    #[serde(default)]
    pub scope: Option<String>,
    /// La generacion del claim que abrio la pasada. Un zombie de una
    /// generacion anterior puede leer la marca pero no barre bajo ella.
    #[serde(default)]
    pub generation: Option<u64>,
}

/// Si el borrado ya ocurrio o solo se pretende.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionState {
    /// Se anoto la intencion y todavia no se sabe si el borrado ocurrio.
    Intended,
    /// El almacen confirmo el borrado.
    Confirmed,
}

impl DeletionState {
    /// El nombre estable, tal y como va en la base.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Intended => "intended",
            Self::Confirmed => "confirmed",
        }
    }
}

/// El rastro del borrado de un blob.
///
/// Se escribe **antes** de borrar (`Intended`) y se confirma despues: un
/// corte entre las dos cosas deja la intencion anotada, y el reinicio la
/// confirma al ver que el blob ya no esta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcDeletion {
    /// El contenido borrado.
    pub digest: ContentDigest,
    /// Los bytes que ocupaba.
    pub length: u64,
    /// La pasada que lo borro.
    pub run_id: String,
    /// Quien lo pidio.
    pub actor: String,
    /// La cuenta verificada del que lo pidio.
    pub account: String,
    /// Si el borrado se confirmo.
    pub state: DeletionState,
    /// Cuando se anoto.
    pub at: DateTime<Utc>,
    /// La marca de version del object store que la pasada observo al
    /// pretender el borrado (modo coordinado, HSP-192). Vacia en el
    /// modo legacy.
    #[serde(default)]
    pub marker: Option<String>,
}

/// Lo que una pasada dejo en cuarentena (HSP-192, AC2).
///
/// La cuarentena es versionada: los bytes marcados se copian a un nombre
/// NUEVO, nunca se reescribe la clave original. Mientras la cuarentena
/// no este reconciliada, la clave queda **reservada**: ni el propio GC
/// vuelve a tocarla ni la pasada la da por reclamada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quarantined {
    /// El contenido marcado.
    pub digest: ContentDigest,
    /// Los bytes que ocupaba la version cuarentenada.
    pub length: u64,
    /// La pasada que lo cuarenteno.
    pub run_id: String,
    /// La clave de cuarentena versionada donde esta el objeto.
    pub quarantine_key: String,
    /// La marca de la version cuarentenada (ETag, mtime... lo que el
    /// almacen use para decir "esta version exacta").
    pub marker: String,
    /// Cuando se cuarenteno.
    pub at: DateTime<Utc>,
}

/// Un paginado de raices leido del catalogo (HSP-192, AC3).
///
/// El censo no materializa el grafo completo: se recorre por paginas
/// acotadas por `limit`, con el cursor de la pagina anterior. Una
/// pagina llena de paginas `limit` avisa de que hay mas; una pagina mas
/// corta marca el final.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootPage {
    /// Las raices de esta pagina.
    pub roots: Vec<ContentRoot>,
    /// Si el catalogo dijo que caben mas raices despues de esta pagina.
    pub has_more: bool,
}

/// El claim fenced de una pasada (HSP-192, AC1).
///
/// El claim es DURABLE —vive en la autoridad del grafo, no en la
/// memoria del worker— y expira por `lease_until`. El token es la cota
/// de fencing: TODA accion de la pasada (marcar, borrar, cuarentenar,
/// confirmar) debe traer el token de la generacion vigente, y un token
/// viejo se rechaza aunque el proceso del worker viejo siga vivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcLease {
    /// La generacion monotonica del claim: sube cada vez que alguien
    /// toma el control, incluido un take-over del lease vencido.
    pub generation: u64,
    /// El identificador opaco de este claim: lo que el worker presenta
    /// en cada operacion. Un worker resucitado con la generacion anterior
    /// no puede presentar el token de la nueva.
    pub token: String,
    /// El instante en que el claim deja de valer.
    pub lease_until: DateTime<Utc>,
}

/// El libro del recolector sobre el catalogo.
///
/// No autoriza: es administracion del almacen, igual que `initialize`. La
/// puerta es la API de administracion del hub, que decide por ruta.
///
/// # Coordinacion entre replicas (HSP-192)
///
/// Los metodos nuevos tienen implementacion por defecto que falla
/// cerrado: un libro sin claim fenced **no puede** participar de una
/// pasada coordinada, y el error lo dice en el tipo. El modo legacy
/// (`sweep`) no los usa y sigue funcionando igual.
#[async_trait]
pub trait ContentLedger: Send + Sync {
    /// Todas las raices del catalogo.
    ///
    /// Si no se pueden leer todas, es un error: un censo parcial no es un
    /// censo, y quien recolecta con el no puede distinguir lo huerfano de
    /// lo que no se vio.
    async fn content_roots(&self) -> Result<Vec<ContentRoot>, CatalogError>;

    /// Las retenciones registradas, vigentes o no.
    async fn holds(&self) -> Result<Vec<ContentHold>, CatalogError>;

    /// Registra una retencion. Repetir el mismo `id` la reemplaza.
    async fn place_hold(&self, hold: &ContentHold) -> Result<(), CatalogError>;

    /// Libera una retencion. Devuelve si existia.
    async fn release_hold(&self, id: &str) -> Result<bool, CatalogError>;

    /// La pasada marcada y sin completar, si la hay.
    async fn pending_run(&self) -> Result<Option<GcRun>, CatalogError>;

    /// La ultima pasada completada, si la hay.
    async fn last_swept_run(&self) -> Result<Option<GcRun>, CatalogError>;

    /// Escribe la marca de una pasada.
    async fn open_run(&self, run: &GcRun) -> Result<(), CatalogError>;

    /// Cierra una pasada con sus totales.
    async fn complete_run(
        &self,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError>;

    /// Escribe o confirma el rastro del borrado de un blob.
    ///
    /// Al confirmar, escribe tambien el evento de auditoria. Confirmar
    /// dos veces no repite el evento.
    async fn record_deletion(&self, deletion: &GcDeletion) -> Result<(), CatalogError>;

    /// El rastro del borrado de un contenido, si lo hay.
    async fn deletion_of(&self, digest: &ContentDigest)
    -> Result<Option<GcDeletion>, CatalogError>;

    // ------------------------------------------------------------------
    // Coordinated, fenced pass (HSP-192). Defaults fail closed: a ledger
    // that cannot fence cannot participate; the legacy mode never calls
    // these and is untouched.
    // ------------------------------------------------------------------

    /// Whether this ledger fences a coordinated pass at all.
    ///
    /// The API surface reads this to refuse a coordinated request with a
    /// typed error instead of pretending. The default says no.
    fn supports_fenced_pass(&self) -> bool {
        false
    }

    /// Take or renew the durable lease of a scope (claim fenced).
    ///
    /// - If no lease exists, or the stored one is expired at `now`, the
    ///   implementation MUST create a NEW generation and token — this is
    ///   how a zombie holder of the previous generation gets fenced.
    /// - If a live lease exists held by `owner`, renew it and hand back
    ///   the SAME token (idempotent for its own holder).
    /// - If a live lease exists held by another owner, report busy.
    ///
    /// Success is decided by reading the claim back after the commit,
    /// never by the transaction merely having committed.
    async fn claim_gc_pass(
        &self,
        scope: &str,
        owner: &str,
        lease: Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        let _ = (scope, owner, lease, now);
        Err(CatalogError::unavailable(
            "claim_gc_pass: this ledger cannot fence a coordinated pass",
        ))
    }

    /// The live lease of a scope, if one exists and has not expired at
    /// `now`. A read, never a claim.
    async fn current_gc_lease(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcLease>, CatalogError> {
        let _ = (scope, now);
        Err(CatalogError::unavailable(
            "current_gc_lease: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Mark a run of candidates inside ONE fenced, guarded transaction.
    ///
    /// The implementation MUST, in the same transaction:
    /// - verify `lease` is still the live claim of `scope` (a stale
    ///   token aborts with `PreconditionFailed`);
    /// - verify that none of the candidate digests is referenced by the
    ///   live catalog (the reference guard): a digest that gained a root
    ///   after the census is NOT marked, and the whole mark aborts
    ///   rather than racing the publisher;
    /// - write the durable mark (the run) so a crash between mark and
    ///   sweep resumes from the ledger, not from memory.
    ///
    /// Returns the candidates as actually marked (post-commit read of
    /// the mark, per the HSP-205 verdict-after-commit rule).
    async fn mark_run_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        run: &GcRun,
        candidate_digests: &[ContentDigest],
    ) -> Result<Vec<RunCandidate>, CatalogError> {
        let _ = (scope, lease, run, candidate_digests);
        Err(CatalogError::unavailable(
            "mark_run_fenced: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Re-confirm the durable mark of `run_id` for the CURRENT live
    /// lease, for a resumed or long pass.
    ///
    /// Fencing is by the presented token, not by the stamp: a take-over
    /// of an expired claim MAY resume the pending mark of the dead
    /// generation (that is the point of the durable mark), but only
    /// while it presents the live token. A zombie whose lease was taken
    /// over gets `PreconditionFailed` here and cannot sweep.
    async fn mark_of_run(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
    ) -> Result<Option<GcRun>, CatalogError> {
        let _ = (scope, lease, run_id);
        Err(CatalogError::unavailable(
            "mark_of_run: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Write or confirm the fenced deletion trace.
    ///
    /// Same contract as [`Self::record_deletion`], but the write carries
    /// the lease and MUST abort if the generation no longer matches: a
    /// zombie worker cannot even record that it deleted.
    async fn record_deletion_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        deletion: &GcDeletion,
    ) -> Result<(), CatalogError> {
        let _ = (scope, lease, deletion);
        Err(CatalogError::unavailable(
            "record_deletion_fenced: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Record that a marked version was quarantined instead of deleted
    /// (AC2 path), reserving its catalog key until reconciliation.
    async fn record_quarantine(
        &self,
        scope: &str,
        lease: &GcLease,
        quarantine: &Quarantined,
    ) -> Result<(), CatalogError> {
        let _ = (scope, lease, quarantine);
        Err(CatalogError::unavailable(
            "record_quarantine: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Read one bounded page of catalog roots (AC3: the plan census must
    /// never materialize the whole graph).
    ///
    /// `after` is the cursor from the previous page (an opaque digest
    /// key); `None` starts at the beginning. Pages are disjoint and
    /// ordered by digest so consecutive passes converge.
    async fn root_page(
        &self,
        scope: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<RootPage, CatalogError> {
        let _ = (scope, after, limit);
        Err(CatalogError::unavailable(
            "root_page: this ledger cannot page the census",
        ))
    }

    /// The quarantines recorded under `run_id`, if any (for resume and
    /// for the reconciliation surface).
    async fn quarantines_of_run(&self, run_id: &str) -> Result<Vec<Quarantined>, CatalogError> {
        let _ = run_id;
        Ok(Vec::new())
    }

    /// The pending (marked, not completed) run of a coordinated scope.
    ///
    /// A worker that restarts resumes through this: the mark is durable
    /// in the authority, not in the process. A run marked by another
    /// scope's generation is not this scope's pending run.
    async fn pending_scoped_run(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcRun>, CatalogError> {
        let _ = (scope, now);
        Err(CatalogError::unavailable(
            "pending_scoped_run: this ledger cannot fence a coordinated pass",
        ))
    }

    /// Renew the lease of the CURRENT generation for its own owner.
    ///
    /// The worker calls this between marks to extend the claim while it
    /// sweeps. A token that is not the current generation's cannot
    /// renew: the answer is a typed precondition failure, and the pass
    /// stops deleting.
    async fn renew_gc_lease(
        &self,
        scope: &str,
        lease: &GcLease,
        owner: &str,
        lease_for: Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        let _ = (scope, lease, owner, lease_for, now);
        Err(CatalogError::unavailable(
            "renew_gc_lease: this ledger cannot fence a coordinated pass",
        ))
    }

    /// The post-commit reference probe (HSP-192: verify AFTER commit).
    ///
    /// Counts the catalog references to `digest` read FRESH from the
    /// authority, after the mark committed. A nonzero count means a
    /// publisher named the blob while the pass was marking: the
    /// candidate is dropped from the sweep, never deleted. This is the
    /// HSP-205 rule — the verdict comes from the state after the
    /// commit, not from the transaction merely having committed.
    async fn live_reference_count(&self, digest: &ContentDigest) -> Result<u64, CatalogError> {
        // The legacy ledgers can only see the direct roots from here.
        // Implementations that hold the graph MUST override and see the
        // whole picture, including manifest edges.
        let roots = self.content_roots().await?;
        Ok(roots.iter().filter(|root| &root.digest == digest).count() as u64)
    }

    /// Close a coordinated run as swept for its generation.
    async fn complete_scoped_run(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        quarantined: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        let _ = (scope, lease, run_id, deleted, bytes_freed, quarantined, at);
        Err(CatalogError::unavailable(
            "complete_scoped_run: this ledger cannot fence a coordinated pass",
        ))
    }
}
