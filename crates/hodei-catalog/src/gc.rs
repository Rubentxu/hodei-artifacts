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
}

/// El libro del recolector sobre el catalogo.
///
/// No autoriza: es administracion del almacen, igual que `initialize`. La
/// puerta es la API de administracion del hub, que decide por ruta.
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
}
