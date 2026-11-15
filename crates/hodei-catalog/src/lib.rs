//! El catalogo de artefactos de Hodei.
//!
//! Ahi se guarda **que** hay, no **donde** estan los bytes: los bytes son
//! del CAS y se llaman por su digest; el catalogo dice que existe un
//! asset, que apunta a una version, y que esa version tiene un digest.
//!
//! # Las cuatro cosas que este catalogo mantiene separadas
//!
//! - **La identidad del repositorio** (`RepositoryDescriptor`): cuenta,
//!   clave, formato, modo y configuracion versionada.
//! - **La identidad del contenido** (`ContentDigest`): el mismo digest es
//!   el mismo contenido, este o otro dia, en esta o en otra cuenta.
//! - **Las coordenadas del paquete** (`PackageCoordinates`): nombre y
//!   version, inmutables una vez publicadas.
//! - **La posicion** (`AssetRecord`): la ruta dentro del repositorio, que
//!   se puede mover a otra version sin tocar los bytes de ninguna.
//!
//! # Por que la separacion importa
//!
//! Una etiqueta mutable que se mueve no puede reescribir el blob que
//! apunta: si lo hiciera, `latest` cambiaria el contenido de una version
//! ya publicada, y el digest dejaria de ser la identidad de nada. Por eso
//! la etiqueta es una fila aparte, y moverla solo escribe esa fila.
//!
//! Y por eso el digest tiene una **politica de ambito** (`DigestScope`):
//! el mismo contenido en dos cuentas puede ser un blob compartido o dos
//! blobs, segun lo que el repositorio declare. La politica se guarda
//! junto al repositorio, no se deduce.
//!
//! # Los errores que no se confunden
//!
//! `Missing`, `Denied` y `Corrupt` son tres cosas distintas y el catalogo
//! las mantiene distintas. Un `Missing` es "no hay nada ahi"; un `Denied`
//! es "hay algo ahi y este principal no puede verlo", y lo decide el
//! puerto de autorizacion antes de mirar la base, para no filtrar la
//! existencia de lo ajeno; un `Corrupt` es "hay algo ahi y esta roto", y
//! se detecta comparando el registro con la version que referencia y con
//! los bytes que el CAS dice que tiene.
//!
//! Lo que la sonda comprueba es la **longitud** de los bytes, no su
//! digest: rehashear un artefacto en cada lectura es un coste que un
//! registro no puede pagar, y la verificacion profunda es del scrub.

#![forbid(unsafe_code)]

pub mod error;
pub mod model;
pub mod ports;

pub use error::CatalogError;
pub use model::{
    AssetRecord, BlobLocation, BlobReference, DigestScope, MutableTag, PackageCoordinates,
    PackageVersion, Publication, PublishOutcome, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
pub use ports::{
    AccessDecision, AccessRequest, AllowAllAuthorizer, BlobIntegrityProbe, BlobProbeError,
    CatalogAction, CatalogAuthorizer, CatalogReader, CatalogResource, CatalogWriter,
    DenyAllAuthorizer,
};
