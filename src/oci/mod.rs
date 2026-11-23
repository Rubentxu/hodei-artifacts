//! La superficie de distribucion OCI: el pull y el push.
//!
//! La distribucion es un **adaptador de entrada**: traduce el protocolo de
//! OCI a los casos de uso que ya existen, el catalogo y el almacen de
//! contenido. No hay un almacenamiento de imagen aparte, y no hay una
//! segunda verdad sobre lo que hay publicado:
//!
//! - Un **blob** es un asset del repositorio, en `blobs/<digest>`.
//! - Un **manifiesto** es un asset del repositorio, en `manifests/<digest>`.
//! - Una **etiqueta** es la referencia mutable que el catalogo resuelve a
//!   las coordenadas del asset.
//!
//! Y las reglas son las de las demas superficies, porque son las del
//! producto: la cuenta viene del principal **verificado**, la decision la
//! lleva Cedar sobre el repositorio, y un recurso de otra cuenta no
//! existe para esta.
//!
//! # Los dos cortes
//!
//! - **Pull** (`src/handlers/oci.rs`): `GET` y `HEAD` de blobs y
//!   manifiestos, por digest y por etiqueta, con la negociacion de tipos.
//! - **Push** (`src/oci/push.rs`): `POST`, `PATCH`, `GET`, `PUT` y
//!   `DELETE` de las sesiones de subida, y el `PUT` del manifiesto.
//!
//! Lo que los dos cortes comparten --el dialecto del error, el nombre del
//! repositorio, el digest, el reparto del path-- vive aqui, para que el
//! error OCI se diga **igual** en los dos lados. Un `DIGEST_INVALID` que
//! en el pull es un `400` y en el push es un `415` es un registro que
//! habla dos idiomas.

pub mod failure;
pub mod push;
pub mod query;
pub mod target;
pub mod token;
pub mod uploads;

use std::sync::Arc;

use axum::http::HeaderMap;
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    CatalogReader, CatalogWriter, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
};

use crate::app_state::AppState;
use failure::OciFailure;

/// El principal verificado de la peticion.
///
/// La autenticacion es la primera puerta, igual que en la superficie
/// cruda: una peticion sin credencial no llega a mirar la ruta.
pub async fn the_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedContext, OciFailure> {
    let token = crate::handlers::bearer_token(headers).ok_or_else(|| {
        // Sin credencial no hay nada que verificar. El reto --que dice
        // **como** conseguir una-- lo pone el borde, que es quien sabe
        // donde vive el despliegue: aqui todavia no se sabe.
        OciFailure::unauthorized()
    })?;
    let principal = match hodei_authz::verify_principal(
        state.authenticator.as_ref(),
        &hodei_authz::Credentials::bearer(token),
    )
    .await
    {
        Ok(principal) => principal,
        Err(hodei_authz::AuthenticationError::ProviderUnavailable(_)) => {
            return Err(OciFailure::unavailable(
                "the authentication provider is not available",
            ));
        }
        Err(_) => {
            return Err(OciFailure::unauthorized());
        }
    };
    Ok(VerifiedContext::from_principal(principal))
}

/// El catalogo, con lo que el necesita para leer.
pub type TheCatalog<'a> = (&'a Arc<dyn CatalogReader>, &'a Arc<dyn CatalogWriter>);

/// El catalogo.
pub fn the_catalog(state: &AppState) -> Result<TheCatalog<'_>, OciFailure> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let writer = state
        .catalog_writer
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    Ok((reader, writer))
}

/// El almacen de contenido.
pub fn the_content_store(
    state: &AppState,
) -> Result<&Arc<dyn hodei_hub::blob::BlobStore>, OciFailure> {
    state
        .content_store
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the content store is not wired"))
}

/// El repositorio, en la cuenta verificada.
///
/// Un repositorio de otra cuenta no existe para esta, y se responde con
/// el `NAME_UNKNOWN` de la distribucion: no dice si existe en otro sitio.
pub async fn the_repository(
    state: &AppState,
    context: &VerifiedContext,
    name: &str,
) -> Result<RepositoryDescriptor, OciFailure> {
    let (reader, _) = the_catalog(state)?;
    match reader.repository(context.account().as_str(), name).await {
        // Un repositorio de un formato que esta release no sirve, o que
        // no es `hosted`, no es un repositorio de esta superficie: se
        // responde con el mismo NAME_UNKNOWN que uno que no existe, para
        // no decir que existe un repositorio que no se puede usar.
        //
        // El formato que se acepta es el que guarda el contenido de esta
        // superficie --el crudo, el de imagen y el de chart--: un chart
        // Helm por OCI es un artefacto OCI, y `helm push` y `helm pull`
        // hablan la distribucion estandar, sin superficie propia. Los
        // demas (maven2, npm, pypi…) no significan nada aqui: una ruta de
        // otro formato no es un recurso de la distribucion.
        Ok(descriptor)
            if !matches!(
                descriptor.format,
                RepositoryFormat::Raw | RepositoryFormat::Oci | RepositoryFormat::Helm
            ) || descriptor.mode != RepositoryMode::Hosted =>
        {
            Err(OciFailure::name_unknown(format!(
                "the repository {name:?} is not known to this account"
            )))
        }
        Ok(descriptor) => Ok(descriptor),
        Err(hodei_catalog::CatalogError::Missing { .. })
        | Err(hodei_catalog::CatalogError::Denied { .. }) => Err(OciFailure::name_unknown(
            format!("the repository {name:?} is not known to this account"),
        )),
        Err(_other) => Err(OciFailure::unavailable("the catalog is not available")),
    }
}

/// La decision de Cedar sobre el repositorio.
///
/// El recurso es el repositorio, igual que en la superficie cruda: una
/// accion sobre un recurso que no existe no se puede evaluar, y la
/// decision la lleva el gate con el recurso a la vista.
pub fn the_decision(
    state: &AppState,
    context: &VerifiedContext,
    action: hodei_authz::model::ActionId,
    repository: &RepositoryDescriptor,
) -> Result<(), OciFailure> {
    let outcome = state
        .authorize_resource
        .execute_canonical(context.clone(), action, &repository.hrn)
        .map_err(|_| OciFailure::unavailable("the authorization engine is not available"))?;
    if !outcome.allows_access() {
        return Err(OciFailure::denied(
            "the principal is not allowed to act on this repository",
        ));
    }
    Ok(())
}

/// Un error del catalogo, traducido al dialecto de la distribucion.
pub fn the_catalog_error(error: &hodei_catalog::CatalogError) -> OciFailure {
    use hodei_catalog::CatalogError;
    match error {
        // Un conflicto es un conflicto: el path ya apunta a otra cosa.
        //
        // El motivo viaja: un cliente que recibe "conflicto" sin saber
        // **que** choca no puede decidir si republica con otras
        // coordenadas o si su contenido es el que sobra.
        CatalogError::Conflict { kind, key, detail } => OciFailure::blob_upload_invalid(format!(
            "the {kind} {key} already holds another content: {detail}"
        )),
        // La precondicion que el cliente exigia no se cumple: no es un
        // conflicto, es una precondicion.
        CatalogError::PreconditionFailed { .. } => {
            OciFailure::blob_upload_invalid("the precondition is not met")
        }
        CatalogError::InvalidRequest { .. } => {
            OciFailure::blob_upload_invalid("the publication is not well formed")
        }
        CatalogError::Missing { .. } => OciFailure::name_unknown("the repository is not there"),
        CatalogError::Denied { .. } => OciFailure::denied("the principal is not allowed"),
        CatalogError::ModeNotImplemented { .. } => {
            OciFailure::name_unknown("the repository is not served by this release")
        }
        CatalogError::SchemaMismatch { .. } | CatalogError::Corrupt { .. } => {
            OciFailure::unavailable("the catalog is not available")
        }
        CatalogError::Unavailable { .. } => OciFailure::unavailable("the catalog is not available"),
    }
}
