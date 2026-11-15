//! Los errores del catalogo.
//!
//! La distincion que mas importa aqui es la de **Missing**, **Denied** y
//! **Corrupt**, porque los tres se parecen y no son lo mismo:
//!
//! - **Missing**: no hay nada ahi. La peticion es valida y el recurso no
//!   existe.
//! - **Denied**: hay algo ahi y este principal no puede verlo. La peticion
//!   es valida y el permiso no.
//! - **Corrupt**: hay algo ahi y no se puede leer bien. La peticion es
//!   valida y el dato esta roto.
//!
//! Confundirlos es peor que no responder: un `Missing` donde habia un
//! `Denied` hace que el cliente repita; un `Corrupt` donde habia un
//! `Missing` hace que el cliente borre algo que no era suyo.

use thiserror::Error;

use crate::model::RepositoryMode;

/// Lo que puede fallar en el catalogo.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogError {
    /// No existe lo que se pidio.
    #[error("there is no {kind} for {key}")]
    Missing {
        /// Que se pidio.
        kind: &'static str,
        /// Con que coordenadas se pidio.
        key: String,
    },

    /// El principal no tiene permiso.
    #[error("the principal is not allowed to {action} the {kind} {key}: {reason}")]
    Denied {
        /// Que se quiso hacer.
        action: &'static str,
        /// Sobre que.
        kind: &'static str,
        /// Con que coordenadas.
        key: String,
        /// Por que no.
        reason: String,
    },

    /// El registro existe pero no se puede leer bien.
    #[error("the {kind} {key} is corrupt: {detail}")]
    Corrupt {
        /// Que se pidio.
        kind: &'static str,
        /// Con que coordenadas.
        key: String,
        /// Que esta roto.
        detail: String,
    },

    /// Ya hay algo ahi con otro contenido.
    ///
    /// Es un conflicto y no un fallo: la base de datos se niega a
    /// sobrescribir, y el catalogo no inventa una regla para hacerlo.
    #[error("the {kind} {key} already exists with different content: {detail}")]
    Conflict {
        /// Que se quiso escribir.
        kind: &'static str,
        /// Con que coordenadas.
        key: String,
        /// En que se diferencia de lo que hay.
        detail: String,
    },

    /// El modo del repositorio no lo sirve esta release.
    #[error("the repository mode {mode} is not served by this release")]
    ModeNotImplemented {
        /// El modo pedido.
        mode: RepositoryMode,
    },

    /// La peticion no tiene sentido.
    #[error("the request is not valid: {detail}")]
    InvalidRequest {
        /// Por que no.
        detail: String,
    },

    /// El almacen tiene un esquema que esta build no entiende.
    ///
    /// No es un fallo del almacenamiento: es un almacen que se escribio
    /// con una version mas nueva que esta. Declarar el esquema encima no
    /// lo arregla (las definiciones que ya estan no se rehacen), asi que
    /// la unica respuesta honesta es no seguir y decirlo.
    #[error(
        "the catalog storage has the schema version {found} and this build understands {expected}"
    )]
    SchemaMismatch {
        /// La version que hay en el almacen.
        found: i64,
        /// La version que entiende esta build.
        expected: i64,
    },

    /// El almacenamiento no pudo responder.
    #[error("the catalog storage is unavailable: {detail}")]
    Unavailable {
        /// Lo que dijo el almacenamiento.
        detail: String,
    },
}

impl CatalogError {
    /// El error de "no hay nada ahi".
    pub fn missing(kind: &'static str, key: impl Into<String>) -> Self {
        CatalogError::Missing {
            kind,
            key: key.into(),
        }
    }

    /// El error de "hay algo ahi y esta roto".
    pub fn corrupt(kind: &'static str, key: impl Into<String>, detail: impl Into<String>) -> Self {
        CatalogError::Corrupt {
            kind,
            key: key.into(),
            detail: detail.into(),
        }
    }

    /// El error de "ya hay algo ahi con otro contenido".
    pub fn conflict(kind: &'static str, key: impl Into<String>, detail: impl Into<String>) -> Self {
        CatalogError::Conflict {
            kind,
            key: key.into(),
            detail: detail.into(),
        }
    }

    /// El error de "la peticion no tiene sentido".
    pub fn invalid(detail: impl Into<String>) -> Self {
        CatalogError::InvalidRequest {
            detail: detail.into(),
        }
    }

    /// El error de "el almacenamiento no pudo responder".
    pub fn unavailable(detail: impl Into<String>) -> Self {
        CatalogError::Unavailable {
            detail: detail.into(),
        }
    }
}
