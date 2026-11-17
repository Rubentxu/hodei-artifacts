//! El paquete de resolucion: lo que un proxy trae consigo a un sitio sin red.
//!
//! Un paquete es un **directorio** con un manifiesto y los blobs que
//! resuelve, igual que una copia de recuperacion. No es una copia del
//! sistema: es el resultado de resolver un repositorio contra su upstream,
//! con la procedencia y las decisiones que lo respaldaban en el momento de
//! exportarlo, para que otro registro lo importe y sirva ese contenido sin
//! salir a la red.
//!
//! Las garantias se deciden aqui y se verifican alla:
//!
//! - Un paquete se **verifica entero antes de hacer visible nada**: ningun
//!   path queda publicable si falta o miente un solo blob.
//! - El upstream del origen es **procedencia, no autoridad**: al importar,
//!   los recibos quedan a nombre de la configuracion del destino, que es
//!   la que puede confirmarse, y la URL original se conserva solo como
//!   evidencia.
//! - Un paquete nunca lleva credenciales: el manifiesto no guarda URLs de
//!   configuracion, y los recibos ya nacen sin secretos.

pub mod manifest;

pub use manifest::{
    AssetEntry, BlobEntry, BundleManifest, DecisionEvidence, Origin, Withheld, BLOBS_DIRECTORY,
    FORMAT, MANIFEST_NAME,
};

use thiserror::Error;

/// Como termino una operacion de paquete.
#[derive(Debug, Error)]
pub enum BundleError {
    /// El paquete, o lo que se le pide, no es consistente.
    #[error("the bundle is not sound: {0}")]
    Unsound(String),
    /// La configuracion del destino no permite importar este paquete.
    #[error("the import is refused: {0}")]
    Refused(String),
    /// El disco o el almacen fallaron.
    #[error("storage failed: {0}")]
    Io(String),
    /// El catalogo o el libro de procedencia fallaron.
    #[error("the catalog failed: {0}")]
    Catalog(String),
    /// Un manifiesto o un recibo no se entiende.
    #[error("the bundle cannot be read: {0}")]
    Read(String),
}

impl From<std::io::Error> for BundleError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<serde_json::Error> for BundleError {
    fn from(error: serde_json::Error) -> Self {
        Self::Read(error.to_string())
    }
}

impl From<hodei_catalog::CatalogError> for BundleError {
    fn from(error: hodei_catalog::CatalogError) -> Self {
        Self::Catalog(error.to_string())
    }
}
