//! La resolucion de un repositorio de grupo en la superficie OCI.
//!
//! Un grupo OCI no guarda nada: es una **lista ordenada** de repositorios
//! del mismo formato, y la precedencia la da el orden en que estan
//! configurados. Gana el primer miembro que tiene la referencia; no hay
//! fusion, ni votacion, ni "el mas reciente": dos lecturas de la misma
//! referencia con la misma configuracion dan la misma fuente.
//!
//! # MISS y ERROR no son lo mismo
//!
//! - **MISS**: el miembro dice con certeza que no lo tiene (no tiene esa
//!   etiqueta, o su fila de asset no esta). Se sigue con el siguiente.
//! - **ERROR**: cualquier otra cosa (el catalogo no responde, el libro de
//!   confianza falla, los bytes no estan donde la fila los dice). La
//!   resolucion se **detiene** y se contesta ese error. Saltar al siguiente
//!   miembro serviria un contenido que quiza no es el que el primero habria
//!   dado: un fallo transitorio cambiaria en silencio lo que se descarga.
//!
//! # Coherencia y procedencia
//!
//! Un manifiesto se sirve **entero** de un solo miembro, y los blobs van
//! direccionados por su digest: solo se sirven despues de comprobar que los
//! bytes **son** el digest pedido. Toda respuesta de un grupo dice que
//! miembro la sirvio (`X-Hodei-Resolved-From`) y en que version de la
//! configuracion del grupo se resolvio (`X-Hodei-Group-Version`).
//!
//! # Permisos y confianza
//!
//! Cedar decide sobre el grupo **y sobre cada miembro**: un miembro que el
//! principal no puede leer se omite sin decir que existe. La compuerta de
//! confianza se aplica al digest del **miembro que gano**: si lo oculta, el
//! resultado es un "no existe" y el grupo **no** pasa al siguiente miembro,
//! porque entonces la revocacion de un digest haria aparecer otro contenido
//! en su lugar.

use std::collections::HashMap;

use axum::http::HeaderMap;
use hodei_authz::VerifiedContext;
use hodei_catalog::{
    CatalogError, GroupConfig, GroupError, Lookup, Node, RepositoryDescriptor, RepositoryMode,
};
use hodei_evidence::ContentDigest;

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;
use crate::oci::target::the_digest_of_reference;

/// Lo que gano una resolucion de grupo.
pub struct Won {
    /// El miembro hoja que sirvio el contenido.
    pub member: RepositoryDescriptor,
    /// El digest del contenido servido.
    pub digest: ContentDigest,
    /// Los bytes del contenido.
    pub bytes: Vec<u8>,
}

/// Como acaba la consulta a un miembro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// El miembro tenia la referencia.
    Hit,
    /// El miembro dijo con certeza que no la tiene.
    Miss,
    /// El miembro fallo: la resolucion se detuvo ahi.
    Error,
}

impl Step {
    /// El paso, en una linea.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Error => "error",
        }
    }
}

/// La resolucion: lo que gano (o el error) y el orden de lo consultado.
pub struct Resolution {
    /// El resultado.
    pub outcome: Result<Won, OciFailure>,
    /// Cada miembro consultado, en orden, con su resultado.
    pub trail: Vec<(String, Step)>,
}

impl Resolution {
    /// El rastro en una linea: `first:miss,second:hit`.
    pub fn trail_line(&self) -> String {
        self.trail
            .iter()
            .map(|(member, step)| format!("{member}:{}", step.as_str()))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Un error de configuracion de grupo, traducido al dialecto de la
/// distribucion.
///
/// Los miembros se validaron al guardar el grupo; si ahora no cuadran (se
/// reconfiguro otro grupo, un miembro ya no existe) el grupo esta mal
/// configurado y no se resuelve a medias.
fn the_group_failure(error: GroupError) -> OciFailure {
    tracing::warn!(error = %error, "an oci group does not resolve");
    OciFailure::unavailable("the group is not well configured")
}

/// Si el principal puede leer este repositorio. Es la decision de Cedar
/// sobre su HRN, la misma que aplica su ruta directa.
fn can_read(
    state: &AppState,
    context: &VerifiedContext,
    repository: &RepositoryDescriptor,
) -> Result<bool, OciFailure> {
    state
        .authorize_resource
        .execute_canonical(
            context.clone(),
            crate::actions::download_asset(),
            &repository.hrn,
        )
        .map(|outcome| outcome.allows_access())
        .map_err(|_| OciFailure::unavailable("the authorization engine is not available"))
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
    OciFailure,
> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let account = context.account().as_str();
    let mut known: HashMap<String, Lookup> = HashMap::new();
    let mut found: HashMap<String, RepositoryDescriptor> = HashMap::new();
    let mut pending: Vec<String> = config.members.clone();
    while let Some(key) = pending.pop() {
        if key == group.repo_key || known.contains_key(&key) {
            continue;
        }
        if known.len() >= hodei_catalog::group::MAX_NODES {
            return Err(the_group_failure(GroupError::TooManyNodes));
        }
        let lookup = match reader.repository(account, &key).await {
            // Un miembro de otra cuenta no existe para esta: se omite
            // como si no existiera, sin decir que existe en otro sitio.
            Err(CatalogError::Missing { .. }) => Lookup::Missing,
            Err(CatalogError::Denied { .. }) => Lookup::Hidden,
            Err(_) => return Err(OciFailure::unavailable("the catalog is not available")),
            Ok(descriptor) => {
                if !can_read(state, context, &descriptor)? {
                    Lookup::Hidden
                } else {
                    let members = if descriptor.mode == RepositoryMode::Group {
                        match GroupConfig::of(&descriptor.config) {
                            Ok(Some(nested)) => nested.members,
                            _ => {
                                return Err(OciFailure::unavailable(
                                    "the group is not well configured",
                                ));
                            }
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

/// Los miembros hoja de un grupo, en el orden en que se consultan, ya
/// leidos y filtrados por lo que el principal puede leer.
pub async fn the_leaves(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
) -> Result<Vec<RepositoryDescriptor>, OciFailure> {
    let config = match GroupConfig::of(&group.config) {
        Ok(Some(config)) => config,
        _ => {
            tracing::warn!(group = %group.repo_key, "an oci group has no readable members");
            return Err(OciFailure::unavailable("the group is not well configured"));
        }
    };
    let (known, mut found) = the_members(state, context, group, &config).await?;
    let walk = hodei_catalog::walk_group(&group.repo_key, group.format, &config.members, &|key| {
        known.get(key).cloned().unwrap_or(Lookup::Missing)
    })
    .map_err(the_group_failure)?;
    walk.leaves
        .iter()
        .map(|key| {
            found
                .remove(key)
                .ok_or_else(|| OciFailure::unavailable("the catalog is not available"))
        })
        .collect()
}

/// Lo que un grupo deja pasar de sus hojas para **una ruta**: las hojas que
/// se consultan y los miembros proxy que la reserva de namespaces del grupo
/// denego.
///
/// La reserva vive en la configuracion del grupo y se consulta siempre,
/// antes de mirar que tiene cada miembro: una ruta reservada no se sirve
/// desde ningun upstream publico del grupo, y un upstream caido no la
/// convierte en fallback. Los miembros hosted son la fuente interna y no
/// los toca la reserva.
///
/// El juicio es sobre el **nombre del repositorio** del miembro: en OCI el
/// namespace es el nombre del repositorio (`team/demo`), no la ruta del
/// asset (`manifests/<digest>`). Un juicio sobre la ruta del asset era
/// inerte: el primer segmento de la ruta es siempre `manifests`, `blobs` o
/// `tags`, y solo casaria con una reserva llamada asi.
pub async fn the_governed_leaves(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
) -> Result<(Vec<RepositoryDescriptor>, Vec<String>), OciFailure> {
    let leaves = the_leaves(state, context, group).await?;
    let policy = hodei_catalog::NamespacePolicy::of(&group.config).map_err(|error| {
        tracing::warn!(error = %error, "the namespace policy of a group cannot be read");
        OciFailure::unavailable("the group is not well configured")
    })?;
    let Some(policy) = policy else {
        return Ok((leaves, Vec::new()));
    };
    let mut consulted = Vec::with_capacity(leaves.len());
    let mut denied = Vec::new();
    for member in leaves {
        // La reserva se juzga por el nombre del repositorio del miembro.
        let judgement = policy.judge(group.format, &member.repo_key);
        if judgement.is_reserved() && member.mode == RepositoryMode::Proxy {
            denied.push(member.repo_key.clone());
        } else {
            consulted.push(member);
        }
    }
    Ok((consulted, denied))
}

/// Si el fallo de un miembro es un **MISS**.
///
/// Un MISS es lo que el miembro dice con certeza que no tiene: un
/// manifiesto o un blob que no existen. Lo demas --un upstream caido, un
/// catalogo que no responde, una confianza que no se puede leer-- es un
/// ERROR, y el ERROR **detiene** la resolucion: un fallo transitorio no
/// puede cambiar en silencio lo que se descarga.
fn is_a_miss(failure: &OciFailure) -> bool {
    matches!(failure.the_code(), "MANIFEST_UNKNOWN" | "BLOB_UNKNOWN")
}

/// El digest al que apunta una referencia en un miembro.
///
/// Una referencia con algoritmo es un digest; una referencia sin el es una
/// etiqueta, y la etiqueta es la referencia mutable que el catalogo
/// resuelve a las coordenadas del asset.
async fn the_digest_of(
    reader: &dyn hodei_catalog::CatalogReader,
    account: &str,
    member: &str,
    reference: &str,
) -> Result<String, OciFailure> {
    if reference.contains(':') {
        return the_digest_of_reference(reference);
    }
    let tag = reader
        .tag(account, member, reference)
        .await
        .map_err(|error| match error {
            // El miembro dice con certeza que no tiene esa etiqueta: es un
            // MISS, y se sigue con el siguiente.
            CatalogError::Missing { .. } => OciFailure::manifest_unknown("unknown"),
            _other => OciFailure::unavailable("the catalog is not available"),
        })?
        .coordinates
        .package_version;
    the_digest_of_reference(&format!("sha256:{tag}"))
}

/// La fila del asset que sirve el contenido de un miembro.
///
/// `Ok(None)` es un MISS: el miembro no tiene esa referencia. Un error es
/// un ERROR: la resolucion se detiene.
async fn the_asset(
    reader: &dyn hodei_catalog::CatalogReader,
    account: &str,
    member: &str,
    asset_path: &str,
) -> Result<Option<hodei_catalog::AssetRecord>, OciFailure> {
    match reader.asset(account, member, asset_path).await {
        Ok(asset) => Ok(Some(asset)),
        Err(CatalogError::Missing { .. }) => Ok(None),
        Err(_) => Err(OciFailure::unavailable("the catalog is not available")),
    }
}

/// La fila del asset del digest pedido.
///
/// La fila dice **que** hay y el digest pedido dice **que** se pide: si no
/// coinciden, el miembro no tiene lo pedido, y es un MISS.
fn the_asset_of(
    asset: hodei_catalog::AssetRecord,
    digest: &str,
) -> Option<hodei_catalog::AssetRecord> {
    if asset.digest.value == digest {
        Some(asset)
    } else {
        None
    }
}

/// Los bytes que la fila referencia.
///
/// El digest de la URL dice **que** se pide; la fila dice **que** hay. Si
/// se sirvieran los bytes del digest de la URL, una fila que solo nombra
/// una ruta serviria el contenido de otro blob, que puede ser de otra
/// cuenta: el CAS es un arbol comun.
async fn the_bytes(
    state: &AppState,
    asset: &hodei_catalog::AssetRecord,
) -> Result<Vec<u8>, OciFailure> {
    let store = crate::oci::the_content_store(state)?;
    store
        .get(&asset.digest)
        .await
        .map_err(|_| OciFailure::unavailable("the catalog and the store disagree"))
}

/// Un manifiesto del grupo, por digest o por etiqueta.
///
/// Recorre los miembros en el orden configurado y gana el primero que
/// tiene la referencia. Un MISS sigue al siguiente; un ERROR detiene la
/// resolucion.
pub async fn the_manifest(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    reference: &str,
    headers: &HeaderMap,
) -> Resolution {
    let mut trail: Vec<(String, Step)> = Vec::new();
    let outcome = resolve_manifest(state, context, group, reference, headers, &mut trail).await;
    Resolution { outcome, trail }
}

async fn resolve_manifest(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    reference: &str,
    headers: &HeaderMap,
    trail: &mut Vec<(String, Step)>,
) -> Result<Won, OciFailure> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let account = context.account().as_str();

    // La reserva se consulta antes que los miembros: una ruta reservada no
    // se sirve desde los miembros publicos del grupo.
    let (leaves, _denied) = the_governed_leaves(state, context, group).await?;

    for member in leaves {
        let key = member.repo_key.clone();
        // Un miembro **proxy** no tiene filas en el catalogo: lo que tiene
        // lo resuelve su upstream, y lo resuelve con el **nombre del
        // miembro**: el grupo no renombra a su miembro.
        if member.mode == RepositoryMode::Proxy {
            let resolved = match crate::oci::proxy::the_resolved_manifest(
                state, context, &member, &key, reference, headers,
            )
            .await
            {
                Ok(resolved) => resolved,
                Err(failure) => {
                    // Un upstream que dice con certeza que no lo tiene es
                    // un MISS, y se sigue. Un upstream caido es un ERROR, y
                    // la resolucion se detiene: no se salta al siguiente
                    // miembro, porque el fallo cambiaria en silencio lo que
                    // se descarga.
                    if is_a_miss(&failure) {
                        trail.push((key, Step::Miss));
                        continue;
                    }
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
            // La compuerta de confianza se aplica al digest del **miembro
            // que gano**: si lo oculta, el resultado es un "no existe" y el
            // grupo **no** pasa al siguiente miembro.
            if let Err(admission) =
                crate::trust::admit(state, account, &member, &resolved.asset.digest).await
            {
                trail.push((key, Step::Error));
                return Err(match admission {
                    crate::trust::Admission::Hidden => OciFailure::manifest_unknown(format!(
                        "the manifest {reference:?} is unknown"
                    )),
                    crate::trust::Admission::Unavailable => {
                        OciFailure::unavailable("the trust ledger is not available")
                    }
                });
            }
            // Los bytes se leen **despues** de la compuerta: lo que no se
            // puede servir no se materializa.
            let bytes = match crate::oci::proxy::the_bytes(state, &resolved.asset).await {
                Ok(bytes) => bytes,
                Err(failure) => {
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
            trail.push((key.clone(), Step::Hit));
            return Ok(Won {
                member,
                digest: resolved.asset.digest,
                bytes,
            });
        }
        // El digest se resuelve **dentro** del namespace del miembro: cada
        // miembro tiene sus propias etiquetas y sus propios digest.
        let digest = match the_digest_of(reader.as_ref(), account, &key, reference).await {
            Ok(digest) => digest,
            Err(failure) => {
                // El error de la etiqueta puede ser un MISS (el miembro no
                // tiene esa etiqueta) o un ERROR (el catalogo no responde).
                // El dialecto lo dice: `manifest_unknown` es un MISS, y
                // cualquier otra cosa detiene la resolucion.
                if failure.the_code() == "MANIFEST_UNKNOWN" {
                    trail.push((key, Step::Miss));
                    continue;
                }
                trail.push((key, Step::Error));
                return Err(failure);
            }
        };
        let asset = match the_asset(
            reader.as_ref(),
            account,
            &key,
            &format!("manifests/{digest}"),
        )
        .await
        {
            Ok(Some(asset)) => the_asset_of(asset, &digest),
            Ok(None) => None,
            Err(failure) => {
                trail.push((key, Step::Error));
                return Err(failure);
            }
        };
        let Some(asset) = asset else {
            trail.push((key, Step::Miss));
            continue;
        };
        // La compuerta de confianza se aplica al digest del **miembro que
        // gano**: si lo oculta, no se busca en el siguiente miembro, porque
        // entonces la revocacion de un digest haria aparecer otro contenido
        // en su lugar.
        if let Err(admission) = crate::trust::admit(state, account, &member, &asset.digest).await {
            trail.push((key, Step::Error));
            return Err(match admission {
                crate::trust::Admission::Hidden => {
                    OciFailure::manifest_unknown(format!("the manifest {reference:?} is unknown"))
                }
                crate::trust::Admission::Unavailable => {
                    OciFailure::unavailable("the trust ledger is not available")
                }
            });
        }
        let bytes = match the_bytes(state, &asset).await {
            Ok(bytes) => bytes,
            Err(failure) => {
                trail.push((key, Step::Error));
                return Err(failure);
            }
        };
        trail.push((key.clone(), Step::Hit));
        return Ok(Won {
            member,
            digest: ContentDigest::sha256(&digest),
            bytes,
        });
    }
    Err(OciFailure::manifest_unknown(format!(
        "the manifest {reference:?} is unknown"
    )))
}

/// Un blob del grupo, por su digest.
///
/// El blob va direccionado por digest: se sirve solo tras comprobar que los
/// bytes **son** el digest pedido.
pub async fn the_blob(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    digest: &str,
    headers: &HeaderMap,
) -> Resolution {
    let mut trail: Vec<(String, Step)> = Vec::new();
    let outcome = resolve_blob(state, context, group, digest, headers, &mut trail).await;
    Resolution { outcome, trail }
}

async fn resolve_blob(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
    digest: &str,
    headers: &HeaderMap,
    trail: &mut Vec<(String, Step)>,
) -> Result<Won, OciFailure> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let account = context.account().as_str();

    let (leaves, _denied) = the_governed_leaves(state, context, group).await?;

    for member in leaves {
        let key = member.repo_key.clone();
        // Un miembro **proxy** resuelve el blob en su upstream, con el
        // nombre del miembro y no con el del grupo.
        if member.mode == RepositoryMode::Proxy {
            let resolved = match crate::oci::proxy::the_resolved_blob(
                state, context, &member, &key, digest, headers,
            )
            .await
            {
                Ok(resolved) => resolved,
                Err(failure) => {
                    if is_a_miss(&failure) {
                        trail.push((key, Step::Miss));
                        continue;
                    }
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
            if let Err(admission) =
                crate::trust::admit(state, account, &member, &resolved.asset.digest).await
            {
                trail.push((key, Step::Error));
                return Err(match admission {
                    crate::trust::Admission::Hidden => OciFailure::blob_unknown(format!(
                        "the repository does not reference {digest}"
                    )),
                    crate::trust::Admission::Unavailable => {
                        OciFailure::unavailable("the trust ledger is not available")
                    }
                });
            }
            // Los bytes se leen **despues** de la compuerta: lo que no se
            // puede servir no se materializa.
            let bytes = match crate::oci::proxy::the_bytes(state, &resolved.asset).await {
                Ok(bytes) => bytes,
                Err(failure) => {
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
            trail.push((key.clone(), Step::Hit));
            return Ok(Won {
                member,
                digest: resolved.asset.digest,
                bytes,
            });
        }
        let asset =
            match the_asset(reader.as_ref(), account, &key, &format!("blobs/{digest}")).await {
                Ok(Some(asset)) => the_asset_of(asset, digest),
                Ok(None) => None,
                Err(failure) => {
                    trail.push((key, Step::Error));
                    return Err(failure);
                }
            };
        let Some(asset) = asset else {
            trail.push((key, Step::Miss));
            continue;
        };
        if let Err(admission) = crate::trust::admit(state, account, &member, &asset.digest).await {
            trail.push((key, Step::Error));
            return Err(match admission {
                crate::trust::Admission::Hidden => {
                    OciFailure::blob_unknown(format!("the repository does not reference {digest}"))
                }
                crate::trust::Admission::Unavailable => {
                    OciFailure::unavailable("the trust ledger is not available")
                }
            });
        }
        let bytes = match the_bytes(state, &asset).await {
            Ok(bytes) => bytes,
            Err(failure) => {
                trail.push((key, Step::Error));
                return Err(failure);
            }
        };
        trail.push((key.clone(), Step::Hit));
        return Ok(Won {
            member,
            digest: ContentDigest::sha256(digest),
            bytes,
        });
    }
    Err(OciFailure::blob_unknown(format!(
        "the repository does not reference {digest}"
    )))
}

/// Las etiquetas del grupo: la **union** de las de sus miembros.
///
/// Una entrada por nombre: si dos miembros tienen la misma etiqueta, la
/// del primero en el orden de precedencia es la que cuenta, y la que se
/// resuelve al descargar. Y el listado del grupo **no se sirve parcial**: si
/// la lista de un miembro no se puede obtener, el listado entero es un
/// error, porque una lista a medias parece completa y no lo es.
pub async fn the_tags(
    state: &AppState,
    context: &VerifiedContext,
    group: &RepositoryDescriptor,
) -> Result<Vec<(String, String)>, OciFailure> {
    let reader = state
        .catalog_reader
        .as_ref()
        .ok_or_else(|| OciFailure::unavailable("the catalog is not wired"))?;
    let account = context.account().as_str();
    let (leaves, _denied) = the_governed_leaves(state, context, group).await?;
    let mut names: Vec<(String, String)> = Vec::new();
    for member in leaves {
        let tags = reader
            .tags(account, &member.repo_key)
            .await
            .map_err(|_| OciFailure::unavailable("the catalog is not available"))?;
        // El filtro de confianza es por miembro: una etiqueta hacia un
        // digest no concedido delataria que existe.
        let servable = crate::trust::servable(state, account, &member)
            .await
            .map_err(|_| OciFailure::unavailable("the trust ledger is not available"))?;
        for tag in tags {
            if let Some(digests) = &servable
                && !digests.contains(&tag.coordinates.package_version)
            {
                continue;
            }
            // Una entrada por nombre: el primer miembro que tiene la
            // etiqueta es el que gana.
            if !names.iter().any(|(name, _)| *name == tag.tag_name) {
                names.push((tag.tag_name, member.repo_key.clone()));
            }
        }
    }
    Ok(names)
}
