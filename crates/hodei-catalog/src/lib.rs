//! El catálogo de artefactos de Hodei.
//!
//! Ahí se guarda **que** hay, no **donde** están los bytes: los bytes son
//! del CAS y se llaman por su digest; el catálogo dice que existe un
//! asset, que apunta a una versión, y que esa versión tiene un digest.
//!
//! # Las cuatro cosas que este catálogo mantiene separadas
//!
//! - **La identidad del repositorio** (`RepositoryDescriptor`): cuenta,
//!   clave, formato, modo y configuración versionada.
//! - **La identidad del contenido** (`ContentDigest`): el mismo digest es
//!   el mismo contenido, este o otro día, en esta o en otra cuenta.
//! - **Las coordenadas del paquete** (`PackageCoordinates`): nombre y
//!   versión, inmutables una vez publicadas.
//! - **La posición** (`AssetRecord`): la ruta dentro del repositorio, que
//!   se puede mover a otra versión sin tocar los bytes de ninguna.
//!
//! # Por qué la separación importa
//!
//! Una etiqueta mutable que se mueve no puede reescribir el blob que
//! apunta: si lo hiciera, `latest` cambiaría el contenido de una versión
//! ya publicada, y el digest dejaría de ser la identidad de nada. Por eso
//! la etiqueta es una fila aparte, y moverla solo escribe esa fila.
//!
//! Y por eso el digest tiene una **política de ámbito** (`DigestScope`):
//! el mismo contenido en dos cuentas puede ser un blob compartido o dos
//! blobs, según lo que el repositorio declare. La política se guarda
//! junto al repositorio, no se deduce.
//!
//! # Los errores que no se confunden
//!
//! `Missing`, `Denied` y `Corrupt` son tres cosas distintas y el catálogo
//! las mantiene distintas. Un `Missing` es "no hay nada ahí"; un `Denied`
//! es "hay algo ahí y este principal no puede verlo", y lo decide el
//! puerto de autorización antes de mirar la base, para no filtrar la
//! existencia de lo ajeno; un `Corrupt` es "hay algo ahí y está roto", y
//! se detecta comparando el registro con la versión que referencia y con
//! los bytes que el CAS dice que tiene.
//!
//! Lo que la sonda comprueba es la **longitud** de los bytes, no su
//! digest: rehashear un artefacto en cada lectura es un coste que un
//! registro no puede pagar, y la verificación profunda es del scrub.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod apt;
pub mod cargo;
pub mod conan;
pub mod conformance;
pub mod error;
pub mod galaxy;
pub mod gc;
pub mod gomod;
pub mod group;
pub mod maven;
pub mod model;
pub mod namespaces;
pub mod npm;
pub mod nuget;
pub mod ports;
pub mod proxy;
pub mod proxy_cache;
pub mod pypi;
pub mod search;
pub mod terraform;
pub mod trust;
pub mod yum;

pub use conformance::{
    Accreditation, AuditFailure, ConformanceCell, EvidenceMismatch, PROFILE_VERSION, accredits,
    audit, cell as conformance_cell, matrix as conformance_matrix, refuses, served_pairs,
};
pub use error::CatalogError;
pub use gc::{
    ContentHold, ContentLedger, ContentRoot, DeletionState, GcDeletion, GcLease, GcRun, HoldScope,
    Quarantined, RootKind, RootPage, RunCandidate, RunState,
};
pub use group::{
    GroupConfig, GroupConfigError, GroupError, Lookup, Node, Walk, walk as walk_group,
};
pub use model::{
    AssetDisposition, AssetRecord, BlobLocation, BlobReference, DigestScope, MutableTag,
    PackageCoordinates, PackageVersion, Precondition, Publication, PublishOutcome, ReferrerRecord,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, TrustState,
    repository_hrn,
};
pub use namespaces::{Judgement, MAX_RESERVED, NamespacePolicy, NamespacePolicyError, OnConflict};
pub use ports::{
    AccessDecision, AccessRequest, AllowAllAuthorizer, BlobIntegrityProbe, BlobProbeError,
    CatalogAction, CatalogAuthorizer, CatalogReader, CatalogResource, CatalogSearch, CatalogWriter,
    DenyAllAuthorizer, MAX_ASSETS_UNDER,
};
pub use proxy::{
    ChecksumEvidence, CredentialRef, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
    UpstreamConfig, UpstreamConfigError, origin_of,
};
pub use search::{
    DEFAULT_LIMIT, MAX_COUNTED, MAX_LIMIT, MAX_OFFSET, RepositoryAttribute, SearchError, SearchHit,
    SearchPage, SearchQuery,
};
pub use trust::{ServeChannel, TrustGrant, TrustLedger};
