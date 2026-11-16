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
