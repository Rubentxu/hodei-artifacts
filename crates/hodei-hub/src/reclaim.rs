//! La recuperacion de bytes del CAS: lo que el recolector necesita del
//! almacen, y nada mas.
//!
//! El recolector decide **que** se puede borrar mirando el catalogo; el
//! almacen es quien sabe **cuando** un blob se escribio o se reutilizo por
//! ultima vez, y quien puede borrar sin pisar una escritura concurrente.
//! Por eso el borrado no es `rm`: es `delete_if_older_than`, que
//! comprueba la edad **dentro de la barrera** del almacen, la misma que
//! protege el commit de una subida.
//!
//! # La barrera
//!
//! Una subida que termina con un contenido que el almacen ya tiene no
//! escribe nada: reutiliza el blob. Sin una marca, el recolector que
//! acaba de decidir que ese blob es huerfano lo borraria justo antes de
//! que el catalogo lo referencie. La marca es la **fecha de modificacion
//! del blob**: cada commit o `put` que reutiliza un blob la renueva, y el
//! borrado la vuelve a leer antes de actuar. Los dos pasos corren bajo el
//! mismo cerrojo, asi que no hay hueco entre mirar y borrar.

use std::time::Duration;

use async_trait::async_trait;
use hodei_evidence::ContentDigest;

use crate::blob::BlobError;

/// Un blob que hay en disco, tal y como lo ve el recolector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// Los bytes que ocupa.
    pub length: u64,
    /// Cuanto hace que se escribio o se reutilizo por ultima vez.
    pub age: Duration,
}

/// Lo que paso al intentar borrar un blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reclaimed {
    /// El blob se borro.
    Deleted {
        /// Los bytes que ocupaba.
        length: u64,
    },
    /// El blob ya no estaba: otra pasada, o un reinicio, lo borro antes.
    Gone,
    /// El blob se reutilizo despues de decidir que era huerfano: se queda.
    Fresh,
}

/// Lo que el recolector necesita del almacen de bytes.
#[async_trait]
pub trait BlobReclaim: Send + Sync {
    /// Todos los blobs que el almacen tiene, con su edad.
    ///
    /// Solo lista lo que el almacen reconoce como suyo: un fichero con
    /// otro nombre, o en otro sitio, no es un blob y no se lista, de modo
    /// que tampoco se puede borrar.
    async fn list_blobs(&self) -> Result<Vec<StoredBlob>, BlobError>;

    /// Borra el blob **solo si** lleva sin tocarse al menos `grace`.
    ///
    /// La comprobacion y el borrado son atomicos frente a los commits del
    /// mismo almacen.
    async fn delete_if_older_than(
        &self,
        digest: &ContentDigest,
        grace: Duration,
    ) -> Result<Reclaimed, BlobError>;
}

/// Lo que el almacen dice de una accion condicional sobre una version
/// exacta de un blob (HSP-192, AC2).
///
/// La condicion es la marca de la version que el recolector observo al
/// marcar: `delete_if_untouched` borra solo si la version actual sigue
/// siendo la marcada; `quarantine_if_untouched` copia la version marcada
/// a un nombre NUEVO y solo entonces, bajo la misma condicion, elimina
/// la clave original. Si la condicion no se cumple, el almacen no toca
/// nada y lo dice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalOutcome {
    /// La version marcada era la vigente: la accion se aplico.
    Applied {
        /// Los bytes de la version actuada.
        length: u64,
    },
    /// El blob ya no estaba: otra pasada lo_actuo antes.
    Gone,
    /// La version cambio desde la marca: **no se toco nada**. La clave
    /// original pertenece a quien la reescribio y queda intacta.
    NotApplied,
}

/// La marca de una version de blob: lo que identifica "esta version
/// exacta" para las guardas condicionales.
///
/// En un almacén de ficheros es el par (mtime, tamano); en S3 es el
/// ETag de la ultima version. Dos escrituras concurrentes de distinto
/// contenido producen marcas distintas; dos escrituras del mismo
/// contenido pueden producir la misma marca, y eso es aceptable: la
/// guarda protege contra pisar una version que el recolector no vio,
/// no contra la reescritura idempotente del mismo byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionMarker {
    /// La clave opaca del objeto tal como la ve el almacen.
    pub key: String,
    /// La marca de la version (ETag, mtime+nano, tamano...).
    pub marker: String,
    /// Los bytes de la version.
    pub length: u64,
}

/// La capacidad de un object store para **delete condicional seguro**.
///
/// El recolector coordinado la exige antes de borrar una marca: si el
/// almacen no la tiene, la pasada NO borra la clave original —prohibe
/// su reutilizacion hasta reconciliar— o cuarentena versionando (nombre
/// nuevo, nunca reescritura). Esta pregunta es la que responde AC2, y
/// por eso vive en el tipo del almacen, no en una configuracion que se
/// pueda poner mal.
#[async_trait]
pub trait ConditionalBlob: Send + Sync {
    /// Si este almacen puede borrar o cuarentenar una version exacta
    /// sin carrera posible con una escritura concurrente.
    fn supports_conditional_delete(&self) -> bool;

    /// Lee la marca de la version actual del blob, si existe.
    async fn head_marker(&self, digest: &ContentDigest)
    -> Result<Option<VersionMarker>, BlobError>;

    /// Borra la clave del blob solo si su version actual sigue marcada
    /// por `marker`. Nunca borra una version que el recolector no vio.
    async fn delete_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
    ) -> Result<ConditionalOutcome, BlobError>;

    /// Cuarentena versionada: copia la version marcada a `quarantine_key`
    /// (un nombre NUEVO que la clave original no ocupa) y, solo si la
    /// copia es exacta, elimina la clave original bajo la condicion del
    /// `marker`.
    ///
    /// La cuarentena es la alternativa de AC2 cuando el delete
    /// condicional no existe o la carrera no se puede cerrar: los bytes
    /// marcados quedan accesibles bajo el nuevo nombre, y la clave
    /// original deja de estar controlada por el recolector.
    async fn quarantine_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
        quarantine_key: &str,
    ) -> Result<ConditionalOutcome, BlobError>;
}
