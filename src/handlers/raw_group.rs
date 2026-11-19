//! La resolucion de un repositorio de grupo en la superficie RAW.
//!
//! Un grupo no guarda nada: es una lista ordenada de repositorios (hosted,
//! proxy u otros grupos). Leer una ruta del grupo recorre los miembros en
//! orden y gana el primero que la tiene.
//!
//! # MISS y ERROR no son lo mismo
//!
//! - **MISS**: el miembro dice con certeza que no tiene la ruta (un hosted
//!   sin ese asset; un proxy cuyo upstream responde 404, o que la tiene
//!   recordada como ausente). Se sigue con el siguiente.
//! - **ERROR**: cualquier otra cosa (el catalogo no responde, el upstream
//!   falla o agota el tiempo, el ambito no se sirve...). La resolucion se
//!   detiene y se contesta ese error. Saltar al siguiente miembro serviria
//!   un contenido que quiza no es el que el primero habria dado: un
//!   fallo transitorio cambiaria silenciosamente lo que se descarga.
//!
//! # Permisos
//!
//! Cedar decide sobre cada repositorio: el grupo (`DownloadAsset` sobre su
//! HRN, en `the_content_of`) y **cada miembro** (aqui). Un miembro que el
//! principal no puede leer se omite de la resolucion, sin decir que existe:
//! el grupo no da acceso a lo que el principal no podria leer por su ruta
//! directa, y la ruta directa no hereda nada del grupo.
//!
//! La compuerta de confianza se aplica despues, sobre el miembro que gano
//! y su digest. Si la compuerta lo oculta, el resultado es un 404: no se
//! busca en el siguiente miembro, porque entonces la revocacion de un
//! digest haria aparecer otro contenido en su lugar.

use std::collections::HashMap;

use hodei_authz::VerifiedContext;
use hodei_catalog::{
    AssetRecord, CatalogError, GroupConfig, GroupError, Lookup, Node, RepositoryDescriptor,
    RepositoryMode,
};

use super::raw::{CacheStatus, ProxyServed, RawApiError, the_current_asset, the_proxy_asset};
use crate::app_state::AppState;

/// Como acabo la consulta a un miembro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// El miembro tenia la ruta.
    Hit,
    /// El miembro dijo con certeza que no la tiene.
    Miss,
    /// El miembro no se consulto: la reserva de namespaces del grupo lo
    /// deja fuera para esta ruta. Queda en el rastro porque la negativa se
    /// explica, no se disimula.
    Deny,
    /// El miembro fallo: la resolucion se detuvo ahi.
    Error,
}

impl Step {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Deny => "deny",
            Self::Error => "error",
        }
    }
}

/// Lo que gano una resolucion de grupo.
pub(super) struct Won {
    /// El miembro hoja que tenia la ruta.
    pub member: RepositoryDescriptor,
    /// Lo que el miembro sirve en esa ruta.
    pub asset: AssetRecord,
    /// Como se sirvio, si el miembro es un proxy.
    pub served: Option<(CacheStatus, Option<(String, String)>)>,
}

/// La resolucion: lo que gano (o el error) y el orden de lo consultado.
pub(super) struct Resolution {
    /// El resultado.
    pub outcome: Result<Won, RawApiError>,
    /// Cada miembro consultado, en orden, con su resultado.
    pub trail: Vec<(String, Step)>,
}

impl Resolution {
    /// El rastro en una linea: `internal:miss,mirror:hit`.
    pub fn trail_line(&self) -> String {
        trail_text(&self.trail)
    }
}

/// Un rastro en una linea: `internal:miss,mirror:hit`.
pub(super) fn trail_text(trail: &[(String, Step)]) -> String {
    trail
        .iter()
        .map(|(member, step)| format!("{member}:{}", step.as_str()))
        .collect::<Vec<_>>()
        .join(",")
}

/// Deja en la auditoria el rastro de una resolucion: que grupo, en que
/// version de su configuracion, que miembros se consultaron y como acabo.
pub(super) fn record_resolution(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    asset_path: &str,
    trail: &str,
    succeeded: bool,
    correlation: &str,
) {
    let result = if succeeded {
        crate::audit::AuditResult::Allowed
    } else {
        crate::audit::AuditResult::Failed
    };
    crate::handlers::record_audit(
        state,
        context.principal().subject().as_str(),
        context.account().as_str(),
        "resolve",
        &format!(
            "{}@v{}/{asset_path} [{trail}]",
            group.repo_key, group.config.version
        ),
        result,
        correlation,
    );
}

fn the_failure(error: GroupError) -> RawApiError {
    // Los miembros se validaron al guardar el grupo; si ahora no cuadran
    // (se reconfiguro otro grupo, un miembro ya no existe) el grupo esta
    // mal configurado y no se resuelve a medias.
    tracing::warn!(error = %error, "a group repository does not resolve");
    RawApiError::Unavailable
}

/// Si el principal puede leer este repositorio. Es la decision de Cedar
/// sobre su HRN, la misma que aplica su ruta directa.
fn can_read(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
) -> Result<bool, RawApiError> {
    state
        .authorize_resource
        .execute_canonical(
            context.clone(),
            crate::actions::download_asset(),
            &repository.hrn,
        )
        .map(|outcome| outcome.allows_access())
        .map_err(|_| RawApiError::Internal)
}

/// Lee los repositorios que alcanza el grupo y dice cuales puede leer el
/// principal.
async fn the_members(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    config: &GroupConfig,
) -> Result<
    (
        HashMap<String, Lookup>,
        HashMap<String, RepositoryDescriptor>,
    ),
    RawApiError,
> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or(RawApiError::Unavailable)?;
    let account = context.account().as_str();
    let mut known: HashMap<String, Lookup> = HashMap::new();
    let mut found: HashMap<String, RepositoryDescriptor> = HashMap::new();
    let mut pending: Vec<String> = config.members.clone();
    while let Some(key) = pending.pop() {
        if key == group.repo_key || known.contains_key(&key) {
            continue;
        }
        if known.len() >= hodei_catalog::group::MAX_NODES {
            return Err(the_failure(GroupError::TooManyNodes));
        }
        let lookup = match reader.repository(account, &key).await {
            Err(CatalogError::Missing { .. }) => Lookup::Missing,
            Err(CatalogError::Denied { .. }) => Lookup::Hidden,
            Err(_) => return Err(RawApiError::Unavailable),
            Ok(descriptor) => {
                if !can_read(state, context, &descriptor)? {
                    Lookup::Hidden
                } else {
                    let members = if descriptor.mode == RepositoryMode::Group {
                        // Un grupo interno que no se lee es un grupo roto:
                        // se falla cerrado, no se trata como vacio.
                        match GroupConfig::of(&descriptor.config) {
                            Ok(Some(nested)) => nested.members,
                            _ => return Err(RawApiError::Unavailable),
                        }
                    } else {
                        Vec::new()
                    };
                    pending.extend(members.iter().cloned());
                    let node = Node {
                        format: descriptor.format,
                        mode: descriptor.mode,
                        members,
                    };
                    found.insert(key.clone(), descriptor);
                    Lookup::Found(node)
                }
            }
        };
        known.insert(key, lookup);
    }
    Ok((known, found))
}

/// Resuelve una ruta en un grupo.
///
/// El grupo ya paso la decision de Cedar y el filtro de formato y ambito
/// de `the_repository`; aqui solo se recorren sus miembros.
pub(super) async fn the_group_asset(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
) -> Resolution {
    let mut trail: Vec<(String, Step)> = Vec::new();
    let outcome = resolve(state, context, group, asset_path, correlation, &mut trail).await;
    Resolution { outcome, trail }
}

/// Los miembros hoja de un grupo, en el orden en que se consultan, ya
/// leidos y filtrados por lo que el principal puede leer.
pub(super) async fn the_leaves(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
) -> Result<Vec<RepositoryDescriptor>, RawApiError> {
    let config = match GroupConfig::of(&group.config) {
        Ok(Some(config)) => config,
        _ => {
            tracing::warn!(group = %group.repo_key, "a group repository has no readable members");
            return Err(RawApiError::Unavailable);
        }
    };
    let (known, mut found) = the_members(state, context, group, &config).await?;
    let walk = hodei_catalog::walk_group(&group.repo_key, group.format, &config.members, &|key| {
        known.get(key).cloned().unwrap_or(Lookup::Missing)
    })
    .map_err(the_failure)?;
    walk.leaves
        .iter()
        .map(|key| found.remove(key).ok_or(RawApiError::Unavailable))
        .collect()
}

/// Lo que un grupo deja pasar de sus hojas para **una ruta**: las hojas
/// que se consultan y los miembros proxy que la reserva de namespaces del
/// grupo denego.
///
/// La reserva vive en la configuracion del grupo y se consulta siempre,
/// antes de mirar que tiene cada miembro: una ruta reservada no se sirve
/// desde ningun upstream publico del grupo, ni del primero ni del ultimo,
/// y un upstream caido no la convierte en fallback. Los miembros hosted
/// son la fuente interna y no los toca la reserva.
///
/// La politica del grupo, si no se entiende, no degrada a "sin reserva":
/// es un grupo mal configurado y no se resuelve a medias.
pub(super) async fn the_governed_leaves(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    asset_path: &str,
) -> Result<
    (
        Vec<RepositoryDescriptor>,
        Vec<String>,
        Option<hodei_catalog::Judgement>,
    ),
    RawApiError,
> {
    let leaves = the_leaves(state, context, group).await?;
    let Some(policy) = super::raw::the_namespace_policy(&group.config)? else {
        return Ok((leaves, Vec::new(), None));
    };
    let judgement = policy.judge(group.format, asset_path);
    if !judgement.is_reserved() {
        return Ok((leaves, Vec::new(), None));
    }
    let mut consulted = Vec::with_capacity(leaves.len());
    let mut denied = Vec::new();
    for member in leaves {
        if member.mode == RepositoryMode::Proxy {
            denied.push(member.repo_key.clone());
        } else {
            consulted.push(member);
        }
    }
    Ok((consulted, denied, Some(judgement)))
}

/// La explicacion de una negativa por reserva a nivel de grupo: la regla
/// que aplico y los miembros que quedaron fuera.
pub(super) fn the_group_reservation_explanation(
    group: &RepositoryDescriptor,
    judgement: &hodei_catalog::Judgement,
    denied: &[String],
) -> RawApiError {
    tracing::warn!(
        group = %group.repo_key,
        denied = ?denied,
        "a reserved namespace is not served from the public members of the group"
    );
    RawApiError::ReservedNamespace(super::raw::the_reservation_explanation(judgement, denied))
}

/// El miembro se lee como se leeria por su ruta directa: esta release solo
/// sirve el ambito global.
pub(super) fn global_only(member: &RepositoryDescriptor) -> Result<(), RawApiError> {
    if member.config.digest_scope == hodei_catalog::DigestScope::Global {
        Ok(())
    } else {
        Err(RawApiError::NotImplemented(
            "this service serves the content of a repository only in the global scope",
        ))
    }
}

async fn resolve(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    asset_path: &str,
    correlation: &str,
    trail: &mut Vec<(String, Step)>,
) -> Result<Won, RawApiError> {
    // Un proxy sin red y sin copia no la tiene a su alcance: se sigue con el
    // siguiente miembro. Si ninguno la sirve, ese es el error.
    let mut offline: Option<RawApiError> = None;
    // Los miembros que la reserva del grupo dejo fuera: con ellos, la
    // negativa final se explica en lugar de parecer un simple 404.
    let (leaves, denied, judgement) =
        the_governed_leaves(state, context, group, asset_path).await?;
    for key in &denied {
        trail.push((key.clone(), Step::Deny));
    }
    for member in leaves {
        let key = member.repo_key.clone();
        if let Err(error) = global_only(&member) {
            trail.push((key, Step::Error));
            return Err(error);
        }
        let looked = match member.mode {
            RepositoryMode::Proxy => {
                match the_proxy_asset(state, context, &member, asset_path, correlation).await {
                    Ok(ProxyServed {
                        asset,
                        status,
                        provenance,
                    }) => Ok(Some((asset, Some((status, provenance))))),
                    // Un 404 de un proxy es que el upstream no la tiene (o
                    // que no es suya): es un MISS, no un fallo.
                    Err(RawApiError::NotFound) => Ok(None),
                    Err(other) => Err(other),
                }
            }
            _ => the_current_asset(state, context, &member.repo_key, asset_path)
                .await
                .map(|found| found.map(|asset| (asset, None))),
        };
        match looked {
            Ok(Some((asset, served))) => {
                trail.push((key.clone(), Step::Hit));
                return Ok(Won {
                    member,
                    asset,
                    served,
                });
            }
            Ok(None) => trail.push((key.clone(), Step::Miss)),
            Err(error @ RawApiError::Offline(_)) => {
                trail.push((key.clone(), Step::Miss));
                offline = Some(error);
            }
            Err(error) => {
                trail.push((key.clone(), Step::Error));
                return Err(error);
            }
        }
    }
    if !denied.is_empty()
        && offline.is_none()
        && let Some(judgement) = judgement
    {
        return Err(the_group_reservation_explanation(
            group, &judgement, &denied,
        ));
    }
    Err(offline.unwrap_or(RawApiError::NotFound))
}
