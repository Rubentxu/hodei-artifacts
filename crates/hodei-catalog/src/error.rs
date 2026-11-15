//! Los errores del catálogo.
//!
//! La distinción que más importa aquí es la de **Missing**, **Denied** y
//! **Corrupt**, porque los tres se parecen y no son lo mismo:
//!
//! - **Missing**: no hay nada ahí. La petición es válida y el recurso no
//!   existe.
//! - **Denied**: hay algo ahí y este principal no puede verlo. La petición
//!   es válida y el permiso no.
//! - **Corrupt**: hay algo ahí y no se puede leer bien. La petición es
//!   válida y el dato está roto.
//!
//! Confundirlos es peor que no responder: un `Missing` donde había un
//! `Denied` hace que el cliente repita; un `Corrupt` donde había un
//! `Missing` hace que el cliente borre algo que no era suyo.

use thiserror::Error;

use crate::model::RepositoryMode;

/// Lo que puede fallar en el catálogo.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogError {
    /// No existe lo que se pidió.
    #[error("there is no {kind} for {key}")]
    Missing {
        /// Qué se pidió.
        kind: &'static str,
        /// Con qué coordenadas se pidió.
        key: String,
    },

    /// El principal no tiene permiso.
    #[error("the principal is not allowed to {action} the {kind} {key}: {reason}")]
    Denied {
        /// Qué se quiso hacer.
        action: &'static str,
        /// Sobre qué.
        kind: &'static str,
        /// Con qué coordenadas.
        key: String,
        /// Por qué no.
        reason: String,
    },

    /// El registro existe pero no se puede leer bien.
    #[error("the {kind} {key} is corrupt: {detail}")]
    Corrupt {
        /// Qué se pidió.
        kind: &'static str,
        /// Con qué coordenadas.
        key: String,
        /// Qué está roto.
        detail: String,
    },

    /// Ya hay algo ahí con otro contenido.
    ///
    /// Es un conflicto y no un fallo: la base de datos se niega a
    /// sobrescribir, y el catálogo no inventa una regla para hacerlo.
    #[error("the {kind} {key} already exists with different content: {detail}")]
    Conflict {
        /// Qué se quiso escribir.
        kind: &'static str,
        /// Con qué coordenadas.
        key: String,
        /// En qué se diferencia de lo que hay.
        detail: String,
    },

    /// La condición que el cliente exigía no se cumple.
    ///
    /// No es un conflicto: el conflicto dice que hay algo distinto de lo
    /// que se publica, y la precondición dice que **lo que hay** no es lo
    /// que el cliente esperaba. La diferencia importa porque el cliente
    /// los trata distinto: un conflicto se resuelve cambiando lo que se
    /// publica, y una precondición releyendo lo que hay.
    #[error("the precondition for the {kind} {key} does not hold: {detail}")]
    PreconditionFailed {
        /// Sobre qué.
        kind: &'static str,
        /// Con qué coordenadas.
        key: String,
        /// En qué se diferencia de lo que el cliente esperaba.
        detail: String,
    },

    /// El modo del repositorio no lo sirve esta release.
    #[error("the repository mode {mode} is not served by this release")]
    ModeNotImplemented {
        /// El modo pedido.
        mode: RepositoryMode,
    },

    /// La petición no tiene sentido.
    #[error("the request is not valid: {detail}")]
    InvalidRequest {
        /// Por qué no.
        detail: String,
    },

    /// El almacén tiene un esquema que esta build no entiende.
    ///
    /// No es un fallo del almacenamiento: es un almacén que se escribió
    /// con una versión más nueva que esta. Declarar el esquema encima no
    /// lo arregla (las definiciones que ya están no se rehacen), así que
    /// la única respuesta honesta es no seguir y decirlo.
    #[error(
        "the catalog storage has the schema version {found} and this build understands {expected}"
    )]
    SchemaMismatch {
        /// La versión que hay en el almacén.
        found: i64,
        /// La versión que entiende esta build.
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
    /// El error de "no hay nada ahí".
    pub fn missing(kind: &'static str, key: impl Into<String>) -> Self {
        CatalogError::Missing {
            kind,
            key: key.into(),
        }
    }

    /// El error de "hay algo ahí y está roto".
    pub fn corrupt(kind: &'static str, key: impl Into<String>, detail: impl Into<String>) -> Self {
        CatalogError::Corrupt {
            kind,
            key: key.into(),
            detail: detail.into(),
        }
    }

    /// El error de "ya hay algo ahí con otro contenido".
    pub fn conflict(kind: &'static str, key: impl Into<String>, detail: impl Into<String>) -> Self {
        CatalogError::Conflict {
            kind,
            key: key.into(),
            detail: detail.into(),
        }
    }

    /// El error de "la petición no tiene sentido".
    pub fn invalid(detail: impl Into<String>) -> Self {
        CatalogError::InvalidRequest {
            detail: detail.into(),
        }
    }

    /// El error de "la condición que el cliente exigía no se cumple".
    pub fn precondition_failed(
        kind: &'static str,
        key: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        CatalogError::PreconditionFailed {
            kind,
            key: key.into(),
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
