//! El libro de la recoleccion coordinada sobre el grafo nativo (HSP-192).
//!
//! El claim del GC **vive en la autoridad**, no en la memoria del
//! worker. Dos replicas sobre el mismo scope se excluyen por el token
//! de la generacion vigente; un zombie con la generacion vencida no
//! puede marcar, ni confirmar, ni cerrar, ni renovar.
//!
//! Las reglas que porta este modulo:
//!
//! 1. **El claim es un CAS durable.** La fila-lease del scope lleva
//!    `generation`, `token`, `owner` y `lease_until`. La guarda va en
//!    el `@filter` del upsert sobre el (token, lease) que se acaba de
//!    leer: el concurrente que leyo lo mismo ve su bloque vacio. Un
//!    lease vencido se take-over con generacion y token NUEVOS. El
//!    veredicto del claim sale de la **relectura post-commit**
//!    (HSP-205), no del hecho de que el commit saliera bien.
//! 2. **La marca es transaccional con guarda** (patron frontier,
//!    HSP-199): la guarda del claim vigente va en el `@filter` del
//!    query del upsert y la marca se escribe condicionada a esa guarda,
//!    en la MISMA transaccion. La intencion de borrado lleva ADEMAS la
//!    guarda de referencias en esa misma transaccion: tres variables
//!    por familia del plano de datos cuentan las filas que nombran el
//!    digest y, si alguna aparece entre la revalidacion y el commit,
//!    la intencion no se escribe. Lo que se barra es lo que la
//!    autoridad devolvio leido despues del commit.
//! 3. **El fence es el token vigente, no el stamp.** Toda operacion de
//!    pasada (marcar, reconfirmar, confirmar, cuarentenar, renovar,
//!    cerrar) comprueba el claim antes de escribir: si no es el suyo,
//!    `PreconditionFailed` y la pasada deja de borrar (AC1). Un
//!    take-over de un lease muerto SI puede reanudar la marca durable
//!    de la generacion anterior — eso es la durabilidad — pero solo
//!    presentando el token vivo.
//! 4. **El censo va por paginas** (AC3): `root_page` recorre las tres
//!    familias del plano de datos del owner con `first(limit+1)`
//!    ordenadas por digest y cursor estricto; los empates del corte se
//!    completan para que paginas sucesivas sean disjuntas y
//!    convergentes. Nunca se materializa el grafo completo, y no hay
//!    censo global: el censo por membresia del legacy se declara
//!    indisponible y obliga al paginado por scope.
//! 5. **Las referencias vivas se cuentan frescas** (revalidacion
//!    post-censo): `live_reference_count` pregunta por cada familia sin
//!    scan global; un candidato con nombre vivo no se marca ni se barre.

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use hodei_catalog::{
    CatalogError, ContentHold, ContentLedger, ContentRoot, DeletionState, GcDeletion, GcLease,
    GcRun, HoldScope, Quarantined, RootKind, RootPage, RunCandidate, RunState,
};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_graph_client::{
    GraphClientError, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction, dql_string,
};
use kernel::Hrn;

use super::{
    DgraphCatalogStore, key_part, node_digest, node_option_text, node_text, node_u64, single_row,
};

/// El tipo de grafo del lease de una pasada coordinada.
pub const GC_LEASE_TYPE: &str = "GcLease";
/// El tipo de grafo de la marca durable de una pasada.
pub const GC_RUN_TYPE: &str = "GcRun";
/// El tipo de grafo del rastro de un borrado.
pub const GC_TOMBSTONE_TYPE: &str = "GcTombstone";
/// El tipo de grafo de una cuarentena versionada.
pub const GC_QUARANTINE_TYPE: &str = "GcQuarantine";
/// El tipo de grafo de una retencion.
pub const GC_HOLD_TYPE: &str = "GcHold";

const LEASE_FIELDS: &str = "hodei.gc.lease.scope hodei.gc.lease.generation \
     hodei.gc.lease.token hodei.gc.lease.owner hodei.gc.lease.lease_until";

/// El DDL aditivo del libro de la recoleccion sobre Dgraph. Cada clave
/// es indice exacto y `@upsert`: el servidor —no una pre-lectura—
/// arbitra dos escritores de la misma clave dentro y fuera de la
/// transaccion.
pub fn gc_schema_dql() -> String {
    let mut ddl = String::new();
    for (predicate, kind, index) in [
        // El lease del scope: la cota de fencing.
        ("hodei.gc.lease.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.lease.scope", "string", "@index(exact)"),
        ("hodei.gc.lease.generation", "int", ""),
        ("hodei.gc.lease.token", "string", ""),
        ("hodei.gc.lease.owner", "string", ""),
        ("hodei.gc.lease.lease_until", "datetime", ""),
        ("hodei.gc.lease.renewed_at", "datetime", ""),
        // La marca durable de la pasada.
        ("hodei.gc.run.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.run.run_id", "string", "@index(exact)"),
        ("hodei.gc.run.scope", "string", "@index(exact)"),
        ("hodei.gc.run.generation", "int", ""),
        ("hodei.gc.run.state", "string", "@index(exact)"),
        ("hodei.gc.run.actor", "string", ""),
        ("hodei.gc.run.grace_secs", "int", ""),
        ("hodei.gc.run.deleted", "int", ""),
        ("hodei.gc.run.bytes_freed", "int", ""),
        ("hodei.gc.run.marked_at", "datetime", ""),
        ("hodei.gc.run.swept_at", "datetime", ""),
        // La marca puede tener N candidatos: el predicado es una LISTA
        // de uid. Declararlo escalar haria que cada arista nueva pise a
        // la anterior y la relectura post-commit no vea el rastro.
        ("hodei.gc.run.candidate", "[uid]", ""),
        // Los candidatos de la marca. La clave compuesta
        // `digest_key|run_id` hace que dos pasadas no se pisen un
        // candidato y que la relectura los ordene de forma estable.
        ("hodei.gc.candidate.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.candidate.digest_of", "string", "@index(exact)"),
        ("hodei.gc.candidate.run_id", "string", "@index(exact)"),
        ("hodei.gc.candidate.length", "int", ""),
        // El rastro del borrado, por digests.
        ("hodei.gc.tombstone.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.tombstone.digest_of", "string", "@index(exact)"),
        ("hodei.gc.tombstone.algorithm", "string", ""),
        ("hodei.gc.tombstone.digest", "string", ""),
        ("hodei.gc.tombstone.run_id", "string", "@index(exact)"),
        ("hodei.gc.tombstone.length", "int", ""),
        ("hodei.gc.tombstone.actor", "string", ""),
        ("hodei.gc.tombstone.account", "string", ""),
        ("hodei.gc.tombstone.state", "string", ""),
        ("hodei.gc.tombstone.marker", "string", ""),
        ("hodei.gc.tombstone.at", "datetime", ""),
        // La cuarentena versionada, por pasada (AC2).
        ("hodei.gc.quarantine.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.quarantine.run_id", "string", "@index(exact)"),
        ("hodei.gc.quarantine.digest_of", "string", "@index(exact)"),
        ("hodei.gc.quarantine.length", "int", ""),
        ("hodei.gc.quarantine.quarantine_key", "string", ""),
        ("hodei.gc.quarantine.marker", "string", ""),
        ("hodei.gc.quarantine.at", "datetime", ""),
        // Las retenciones del censo.
        ("hodei.gc.hold.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.hold.hold_id", "string", "@index(exact)"),
        ("hodei.gc.hold.scope_kind", "string", "@index(exact)"),
        ("hodei.gc.hold.account", "string", "@index(exact)"),
        ("hodei.gc.hold.repository", "string", ""),
        ("hodei.gc.hold.digest_of", "string", "@index(exact)"),
        ("hodei.gc.hold.reason", "string", ""),
        ("hodei.gc.hold.actor", "string", ""),
        ("hodei.gc.hold.created_at", "datetime", ""),
        ("hodei.gc.hold.expires_at", "datetime", ""),
    ] {
        ddl.push_str(predicate);
        ddl.push_str(": ");
        ddl.push_str(kind);
        if !index.is_empty() {
            ddl.push(' ');
            ddl.push_str(index);
        }
        ddl.push_str(" .\n");
    }
    for (name, family, fields) in [
        (
            GC_LEASE_TYPE,
            "lease",
            "key scope generation token owner lease_until renewed_at",
        ),
        (
            GC_RUN_TYPE,
            "run",
            "key run_id scope generation state actor grace_secs deleted bytes_freed marked_at \
             swept_at candidate",
        ),
        (
            GC_TOMBSTONE_TYPE,
            "tombstone",
            "key digest_of algorithm digest run_id length actor account state marker at",
        ),
        (
            GC_QUARANTINE_TYPE,
            "quarantine",
            "key run_id digest_of length quarantine_key marker at",
        ),
        (
            GC_HOLD_TYPE,
            "hold",
            "key hold_id scope_kind account repository digest_of reason actor created_at \
             expires_at",
        ),
    ] {
        ddl.push_str(&format!("type {name} {{\n"));
        for field in fields.split_whitespace() {
            ddl.push_str(&format!("hodei.gc.{family}.{field}\n"));
        }
        ddl.push_str("}\n");
    }
    ddl
}

/// La clave del lease de un scope: unica por scope.
pub fn gc_lease_key(scope: &str) -> String {
    format!("lease|{}", key_part(scope))
}

/// La clave del run.
fn gc_run_key(run_id: &str) -> String {
    format!("run|{}", key_part(run_id))
}

/// La parte de digests de una clave compuesta: `algoritmo|valor`.
fn digest_key(digest: &ContentDigest) -> String {
    format!(
        "{}|{}",
        key_part(&digest.algorithm.to_string()),
        key_part(&digest.value)
    )
}

/// La clave del candidato de una pasada.
fn gc_candidate_key(digest: &ContentDigest, run_id: &str) -> String {
    format!("{}|{}", digest_key(digest), key_part(run_id))
}

/// Desmonta una clave de digests: `algoritmo|valor`.
fn split_digest_key(digest_of: &str) -> Option<ContentDigest> {
    let (algorithm, value) = digest_of.split_once('|')?;
    let algorithm = match algorithm {
        "sha256" => DigestAlgorithm::Sha256,
        "sha512" => DigestAlgorithm::Sha512,
        _ => return None,
    };
    Some(ContentDigest {
        algorithm,
        value: value.to_owned(),
    })
}

fn stamp(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_stamp(field: &str, text: &str) -> Result<DateTime<Utc>, CatalogError> {
    DateTime::parse_from_rfc3339(text)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| CatalogError::corrupt("gc ledger", field, format!("'{text}' is not a time")))
}

/// El error del grafo dicho como error del libro: la perdida de la
/// carrera `@upsert` es un conflicto; lo demas, indisponibilidad.
fn graph_error(error: GraphClientError) -> CatalogError {
    match error {
        GraphClientError::UpsertConflict { detail } => {
            CatalogError::conflict("gc ledger", "key", detail)
        }
        other => CatalogError::unavailable(other.to_string()),
    }
}

/// El claim vigente como lo porta el grafo.
fn lease_from(node: &serde_json::Value) -> Result<GcLease, CatalogError> {
    Ok(GcLease {
        generation: node_u64(node, "hodei.gc.lease.generation")?,
        token: node_text(node, "hodei.gc.lease.token")?,
        lease_until: parse_stamp(
            "hodei.gc.lease.lease_until",
            &node_text(node, "hodei.gc.lease.lease_until")?,
        )?,
    })
}

/// El blank node estable de un candidato.
fn blank_of(key: &str) -> String {
    format!(
        "_:c_{}",
        key.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    )
}

/// El cursor del paginado es la impresion del digests: `algoritmo:valor`.
fn cursor_of_digest(cursor: &str) -> Result<ContentDigest, CatalogError> {
    let (algorithm, value) = cursor
        .split_once(':')
        .ok_or_else(|| CatalogError::invalid("the census cursor is not a digest"))?;
    let algorithm = match algorithm {
        "sha256" => DigestAlgorithm::Sha256,
        "sha512" => DigestAlgorithm::Sha512,
        other => {
            return Err(CatalogError::invalid(format!(
                "'{other}' is not a digest algorithm"
            )));
        }
    };
    Ok(ContentDigest {
        algorithm,
        value: value.to_owned(),
    })
}

/// El owner del censo: si el scope es una HRN, su cuenta; si no, el
/// propio scope. La cuenta es lo que particiona las filas del plano de
/// datos — el censo JAMAS es global.
fn scope_owner(scope: &str) -> String {
    Hrn::from_string(scope)
        .map(|hrn| hrn.account_id().to_owned())
        .unwrap_or_else(|| scope.to_owned())
}

/// Las familias del plano de datos que mantienen vivo un digest.
#[derive(Clone, Copy)]
enum Family {
    Asset,
    Version,
    Reference,
    AptSnapshot,
}

impl Family {
    fn prefix(self) -> &'static str {
        match self {
            Self::Asset => "hodei.catalog.asset",
            Self::Version => "hodei.catalog.version",
            Self::Reference => "hodei.catalog.reference",
            Self::AptSnapshot => "hodei.catalog.apt_snapshot_root",
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Self::Asset => "hodei.catalog.asset.asset_path",
            Self::Version => {
                "hodei.catalog.version.package_name hodei.catalog.version.package_version"
            }
            Self::Reference => "hodei.catalog.reference.asset_path",
            Self::AptSnapshot => "hodei.catalog.apt_snapshot_root.subject",
        }
    }

    fn kind(self) -> RootKind {
        match self {
            Self::Asset | Self::AptSnapshot => RootKind::Asset,
            Self::Version => RootKind::Version,
            Self::Reference => RootKind::Reference,
        }
    }
}

/// Una fila del censo leida del grafo, ordenable por (digest, clave).
struct CensusRow {
    row_key: String,
    root: ContentRoot,
    digest: ContentDigest,
}

impl CensusRow {
    fn from_node(family: Family, node: &serde_json::Value) -> Result<Self, CatalogError> {
        let prefix = family.prefix();
        let digest = node_digest(
            node,
            &format!("{prefix}.digest_algorithm"),
            &format!("{prefix}.digest"),
        )?;
        let account = node_text(node, &format!("{prefix}.account"))?;
        let repository = node_text(node, &format!("{prefix}.repository"))?;
        let subject = match family {
            Family::Asset => node_text(node, "hodei.catalog.asset.asset_path")?,
            Family::Version => format!(
                "{}@{}",
                node_text(node, "hodei.catalog.version.package_name")?,
                node_text(node, "hodei.catalog.version.package_version")?
            ),
            Family::Reference => node_text(node, "hodei.catalog.reference.asset_path")?,
            Family::AptSnapshot => node_text(node, "hodei.catalog.apt_snapshot_root.subject")?,
        };
        let row_key = node_text(node, &format!("{prefix}.key"))?;
        Ok(Self {
            digest: digest.clone(),
            row_key,
            root: ContentRoot {
                kind: family.kind(),
                account,
                repository,
                subject,
                digest,
            },
        })
    }
}

impl DgraphCatalogStore {
    // -----------------------------------------------------------------------
    // Claim fenced (AC1).
    // -----------------------------------------------------------------------

    /// Toma o renueva el lease durable del scope.
    ///
    /// Lee el claim vigente y el upsert lleva la guarda CAS en el
    /// `@filter` sobre lo que acabo de ver: un claim ajeno vigente se
    /// responde ocupado sin escribir; el propio holder renueva con el
    /// MISMO token; un lease vencido (o sin fila) se take-over con
    /// generacion nueva y token NUEVO. El veredicto sale de la
    /// relectura: si el token vigente no es el que este upsert dejo,
    /// otro gano la carrera y no soy el holder.
    pub async fn claim_gc_pass_here(
        &self,
        scope: &str,
        owner: &str,
        lease_for: std::time::Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        if scope.is_empty() || owner.is_empty() {
            return Err(CatalogError::invalid(
                "the claim needs a scope and an owner",
            ));
        }
        let lease_for = chrono::Duration::from_std(lease_for)
            .map_err(|error| CatalogError::invalid(format!("the lease is not valid: {error}")))?;
        let until = now + lease_for;
        let key = gc_lease_key(scope);
        let seen = self.lease_row(&key).await?;
        // El veredicto del claim sale de la relectura post-commit
        // (HSP-205); la escritura va guardada por la fila que este
        // claim leyo: el query liga la (clave, token) vistos y, en el
        // take-over, solo si el lease ya vencia a `now`. Si otro gano
        // la carrera entre la lectura y el commit, la guarda no liga
        // nada y el bloque queda inerte.
        let (query, set, cond);
        let expected_token;
        let expected_generation;
        match &seen {
            None => {
                // Sin fila: la primera generacion, con su propio token.
                // La creacion se arbitra con la ausencia de fila y el
                // `@upsert` de la clave: dos escritores de la primera
                // generacion pierden o ganan por el indice, no por el
                // orden de lectura.
                let token = format!("{}-{}", self.identity.new_id(), stamp(now));
                expected_token = token.clone();
                expected_generation = 1;
                query = format!(
                    "{{ nc as var(func: eq(hodei.gc.lease.key, {})) }}",
                    dql_string(&key),
                );
                cond = "eq(len(nc), 0)".to_owned();
                set = format!(
                    "_:ls <dgraph.type> \"{GC_LEASE_TYPE}\" .\n\
                     _:ls <hodei.gc.lease.key> {key_q} .\n\
                     _:ls <hodei.gc.lease.scope> {scope_q} .\n\
                     _:ls <hodei.gc.lease.generation> \"1\" .\n\
                     _:ls <hodei.gc.lease.token> {token_q} .\n\
                     _:ls <hodei.gc.lease.owner> {owner_q} .\n\
                     _:ls <hodei.gc.lease.lease_until> \"{until}\" .\n\
                     _:ls <hodei.gc.lease.renewed_at> \"{at}\" .\n",
                    key_q = dql_string(&key),
                    scope_q = dql_string(scope),
                    token_q = dql_string(&token),
                    owner_q = dql_string(owner),
                    until = stamp(until),
                    at = stamp(now),
                );
            }
            Some(row) => {
                let generation = node_u64(row, "hodei.gc.lease.generation")?;
                let seen_token = node_text(row, "hodei.gc.lease.token")?;
                let seen_owner = node_text(row, "hodei.gc.lease.owner")?;
                let seen_until = parse_stamp(
                    "hodei.gc.lease.lease_until",
                    &node_text(row, "hodei.gc.lease.lease_until")?,
                )?;
                if seen_until > now && seen_owner != owner {
                    return Err(CatalogError::conflict(
                        "gc lease",
                        scope,
                        format!(
                            "the scope is held by owner '{}' until {}",
                            seen_owner,
                            stamp(seen_until)
                        ),
                    ));
                }
                // La guarda del claim visto se liga en el query del
                // upsert con el token Y el estado de lease que este
                // claim leyo: una renovacion que prolonga un lease
                // vivo mantiene el token (el fence no la detectaria),
                // y un take-over que cambia el token la borra. Con
                // ambos, la carrera queda arbitrada por la autoridad
                // en el snapshot de la transaccion; la relectura
                // post-commit dice quien gano.
                if seen_until > now {
                    // El propio holder renueva: mismo token, lease mas
                    // largo.
                    query = format!(
                        "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                         @filter(eq(hodei.gc.lease.token, {}) AND \
                         gt(hodei.gc.lease.lease_until, \"{}\")) }}",
                        dql_string(&key),
                        dql_string(&seen_token),
                        stamp(now),
                    );
                    expected_token = seen_token.clone();
                    expected_generation = generation;
                    cond = "eq(len(gl), 1)".to_owned();
                    set = format!(
                        "uid(gl) <hodei.gc.lease.lease_until> \"{until}\" .\n\
                         uid(gl) <hodei.gc.lease.renewed_at> \"{at}\" .\n",
                        until = stamp(until),
                        at = stamp(now),
                    );
                } else {
                    // Take-over: generacion nueva y token NUEVO.
                    query = format!(
                        "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                         @filter(eq(hodei.gc.lease.token, {}) AND \
                         le(hodei.gc.lease.lease_until, \"{}\")) }}",
                        dql_string(&key),
                        dql_string(&seen_token),
                        stamp(now),
                    );
                    let token = format!("{}-{}", self.identity.new_id(), stamp(now));
                    expected_token = token.clone();
                    expected_generation = generation + 1;
                    cond = "eq(len(gl), 1)".to_owned();
                    set = format!(
                        "uid(gl) <hodei.gc.lease.generation> \"{next}\" .\n\
                         uid(gl) <hodei.gc.lease.token> {token_q} .\n\
                         uid(gl) <hodei.gc.lease.owner> {owner_q} .\n\
                         uid(gl) <hodei.gc.lease.lease_until> \"{until}\" .\n\
                         uid(gl) <hodei.gc.lease.renewed_at> \"{at}\" .\n",
                        next = generation + 1,
                        token_q = dql_string(&token),
                        owner_q = dql_string(owner),
                        until = stamp(until),
                        at = stamp(now),
                    );
                }
            }
        }
        let transaction = GraphUpsertTransaction {
            query,
            blocks: vec![GraphMutationBlock::set_if(cond, set)],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        // Relectura post-commit: el veredicto del claim lo dice la
        // autoridad despues del commit, no el hecho de que el commit
        // saliera bien (HSP-205). Si el claim vigente no es el que se
        // pretendio dejar, otro gano la carrera: ocupado.
        let back = self
            .current_gc_lease_here(scope, Utc::now())
            .await?
            .ok_or_else(|| CatalogError::unavailable("the claim vanished after commit"))?;
        if back.token != expected_token || back.generation != expected_generation {
            return Err(CatalogError::conflict(
                "gc lease",
                scope,
                "another claim won the race for this scope",
            ));
        }
        Ok(back)
    }

    /// El claim vigente del scope, leido. `None` si no hay o vencio.
    pub async fn current_gc_lease_here(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcLease>, CatalogError> {
        let row = self.lease_row(&gc_lease_key(scope)).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let lease = lease_from(&row)?;
        Ok((lease.lease_until > now).then_some(lease))
    }

    async fn lease_row(&self, key: &str) -> Result<Option<serde_json::Value>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.gc.lease.key, {})) {{ {LEASE_FIELDS} }} }}",
            dql_string(key),
        );
        let json = self.execute_dql_row(&query).await?;
        single_row(json.get("q"), "gc lease", key.to_owned())
    }

    /// El fence de la autoridad: el token presentado es el claim
    /// vigente del scope. Si no, `PreconditionFailed` (AC1).
    async fn the_fence(
        &self,
        scope: &str,
        lease: &GcLease,
        now: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        let row = self.lease_row(&gc_lease_key(scope)).await?;
        match &row {
            Some(node) => {
                let live = lease_from(node)?;
                if live.lease_until <= now {
                    return Err(CatalogError::precondition_failed(
                        "gc lease",
                        scope,
                        "the lease expired; its generation no longer rules",
                    ));
                }
                if live.token != lease.token || live.generation != lease.generation {
                    return Err(CatalogError::precondition_failed(
                        "gc lease",
                        scope,
                        format!(
                            "token presented for generation {} but the scope is held by generation {}",
                            lease.generation, live.generation
                        ),
                    ));
                }
                Ok(())
            }
            None => Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                "the scope has no live lease",
            )),
        }
    }

    async fn execute_dql_row(&self, query: &str) -> Result<serde_json::Value, CatalogError> {
        self.client
            .execute_dql(query)
            .await
            .map(|response| response.json)
            .map_err(|error| CatalogError::unavailable(error.to_string()))
    }

    // -----------------------------------------------------------------------
    // Marca fenced (HSP-199) y reanudacion.
    // -----------------------------------------------------------------------

    /// Escribe la marca de la pasada en UNA transaccion guardada.
    ///
    /// La guarda del claim va en el `@filter` del query del upsert: si
    /// el token presentado no es el vigente, `gl` queda vacio y ningun
    /// n-quad se aplica. Tras el commit la marca se relee: lo que se
    /// barra es lo que la autoridad confirmo, y una marca que no es de
    /// esta generacion es `PreconditionFailed`.
    pub async fn mark_run_fenced_here(
        &self,
        scope: &str,
        lease: &GcLease,
        run: &GcRun,
    ) -> Result<Vec<RunCandidate>, CatalogError> {
        let now = Utc::now();
        let (set, _) = run_mark_nquads(scope, lease, run);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {}) AND eq(hodei.gc.lease.generation, {}) \
                 AND gt(hodei.gc.lease.lease_until, \"{}\")) }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&lease.token),
                lease.generation,
                stamp(now),
            ),
            blocks: vec![GraphMutationBlock::set_if("eq(len(gl), 1)", set)],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        let marked = self.the_run_row(&run.id).await?;
        match marked {
            Some(row)
                if row.scope.as_deref() == Some(scope)
                    && row.generation == Some(lease.generation) =>
            {
                Ok(row.candidates)
            }
            Some(_) => Err(CatalogError::precondition_failed(
                "gc mark",
                &run.id,
                "the mark committed under another scope or generation",
            )),
            None => Err(CatalogError::precondition_failed(
                "gc mark",
                &run.id,
                "the mark did not land: the presented token is not the live claim",
            )),
        }
    }

    /// La marca de `run_id` reconfirmada bajo el claim vigente.
    ///
    /// El fence es por token vigente, no por el stamp de la marca: un
    /// take-over de un lease muerto puede reanudar la marca durable de
    /// la generacion anterior (eso es la durabilidad), pero solo
    /// presentando el token vivo. Un zombie recibe aqui
    /// `PreconditionFailed` y no puede barrer.
    pub async fn mark_of_run_here(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
    ) -> Result<Option<GcRun>, CatalogError> {
        self.the_fence(scope, lease, Utc::now()).await?;
        let run = self.the_run_row(run_id).await?;
        Ok(run.filter(|run| run.scope.as_deref() == Some(scope)))
    }

    /// La pasada marcada y no completada del scope, leida del grafo.
    pub async fn pending_scoped_run_here(
        &self,
        scope: &str,
        _now: DateTime<Utc>,
    ) -> Result<Option<GcRun>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.gc.run.scope, {})) \
             @filter(eq(hodei.gc.run.state, \"marked\")) {{ hodei.gc.run.run_id }} }}",
            dql_string(scope),
        );
        let json = self.execute_dql_row(&query).await?;
        let run_id = json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|node| node.get("hodei.gc.run.run_id").and_then(|v| v.as_str()))
            .min()
            .map(str::to_owned);
        match run_id {
            Some(run_id) => self.the_run_row(&run_id).await,
            None => Ok(None),
        }
    }

    /// Renueva el lease de la generacion vigente para su propio owner.
    ///
    /// El fence primero; el upsert esta guardado por el token. Tras el
    /// commit se relee: si el scope paso a otra generacion entre el
    /// fence y la relectura, esto es `PreconditionFailed` y la pasada
    /// deja de barrer — un zombie no continua con el token nuevo.
    pub async fn renew_gc_lease_here(
        &self,
        scope: &str,
        lease: &GcLease,
        owner: &str,
        lease_for: std::time::Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        self.the_fence(scope, lease, now).await?;
        let lease_for = chrono::Duration::from_std(lease_for)
            .map_err(|error| CatalogError::invalid(format!("the lease is not valid: {error}")))?;
        let until = now + lease_for;
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {}) AND eq(hodei.gc.lease.owner, {})) }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&lease.token),
                dql_string(owner),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(gl), 1)",
                format!(
                    "uid(gl) <hodei.gc.lease.lease_until> \"{until}\" .\n\
                     uid(gl) <hodei.gc.lease.renewed_at> \"{at}\" .\n",
                    until = stamp(until),
                    at = stamp(now),
                ),
            )],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        let back = self
            .current_gc_lease_here(scope, Utc::now())
            .await?
            .ok_or_else(|| CatalogError::unavailable("the lease vanished after renew"))?;
        if back.token != lease.token || back.generation != lease.generation {
            return Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                "the scope moved to a new generation mid-sweep",
            ));
        }
        Ok(back)
    }

    /// Cierra la pasada bajo el claim vigente.
    pub async fn complete_scoped_run_here(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        self.the_fence(scope, lease, at).await?;
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ ru as var(func: eq(hodei.gc.run.run_id, {})) \
                 @filter(eq(hodei.gc.run.scope, {})) }}",
                dql_string(run_id),
                dql_string(scope),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(ru), 1)",
                format!(
                    "uid(ru) <hodei.gc.run.state> \"swept\" .\n\
                     uid(ru) <hodei.gc.run.deleted> \"{deleted}\" .\n\
                     uid(ru) <hodei.gc.run.bytes_freed> \"{bytes}\" .\n\
                     uid(ru) <hodei.gc.run.swept_at> \"{at}\" .\n",
                    bytes = bytes_freed,
                    at = stamp(at),
                ),
            )],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        let back = self.the_run_row(run_id).await?;
        match back {
            Some(row) if row.state == RunState::Swept => Ok(()),
            _ => Err(CatalogError::precondition_failed(
                "gc run",
                run_id,
                "the close did not land under the presented token",
            )),
        }
    }

    // -----------------------------------------------------------------------
    // Rastro y cuarentena fenced.
    // -----------------------------------------------------------------------

    /// El rastro fenced de un borrado. El fence primero; la escritura
    /// va guardada por el token y por la guarda de referencias en el
    /// MISMO upsert (patron frontier, HSP-199): tres variables ligas
    /// por igualdad de digest sobre las tres familias del plano de
    /// datos, contadas en el snapshot de la transaccion. Un publisher
    /// concurrente que nombro el blob entre la revalidacion del
    /// llamante y este commit deja la intencion sin escribir — el
    /// veredicto sale de la relectura post-commit (HSP-205). Un
    /// zombie con lease vencido no puede ni anotar que borro.
    pub async fn record_deletion_fenced_here(
        &self,
        scope: &str,
        lease: &GcLease,
        deletion: &GcDeletion,
    ) -> Result<(), CatalogError> {
        self.the_fence(scope, lease, Utc::now()).await?;
        let tkey = format!("tomb|{}", digest_key(&deletion.digest));
        let create = tombstone_nquads("_:td", &tkey, deletion, true);
        let update = tombstone_nquads("uid(td)", &tkey, deletion, false);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {})) \n\
                 td as var(func: eq(hodei.gc.tombstone.key, {})) \n\
                 {} }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&lease.token),
                dql_string(&tkey),
                reference_guard_lines(&deletion.digest),
            ),
            blocks: vec![
                GraphMutationBlock::set_if(
                    format!(
                        "eq(len(gl), 1) AND {} AND eq(len(td), 0)",
                        no_live_reference()
                    ),
                    create,
                ),
                GraphMutationBlock::set_if(
                    format!(
                        "eq(len(gl), 1) AND {} AND eq(len(td), 1)",
                        no_live_reference()
                    ),
                    update,
                ),
            ],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        self.verdict_tombstone(deletion).await
    }

    /// El rastro de un digests, leido.
    pub async fn deletion_of_here(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<GcDeletion>, CatalogError> {
        let tkey = format!("tomb|{}", digest_key(digest));
        let query = format!(
            "{{ q(func: eq(hodei.gc.tombstone.key, {})) {{ \
             hodei.gc.tombstone.algorithm hodei.gc.tombstone.digest \
             hodei.gc.tombstone.run_id hodei.gc.tombstone.length \
             hodei.gc.tombstone.actor hodei.gc.tombstone.account \
             hodei.gc.tombstone.state hodei.gc.tombstone.at \
             hodei.gc.tombstone.marker }} }}",
            dql_string(&tkey),
        );
        let json = self.execute_dql_row(&query).await?;
        let node = single_row(json.get("q"), "gc tombstone", tkey)?;
        let Some(node) = node else {
            return Ok(None);
        };
        let state = match node_text(&node, "hodei.gc.tombstone.state")?.as_str() {
            "intended" => DeletionState::Intended,
            "confirmed" => DeletionState::Confirmed,
            other => {
                return Err(CatalogError::corrupt(
                    "gc tombstone",
                    other.to_owned(),
                    "not a deletion state",
                ));
            }
        };
        Ok(Some(GcDeletion {
            digest: node_digest(
                &node,
                "hodei.gc.tombstone.algorithm",
                "hodei.gc.tombstone.digest",
            )?,
            length: node_u64(&node, "hodei.gc.tombstone.length")?,
            run_id: node_text(&node, "hodei.gc.tombstone.run_id")?,
            actor: node_text(&node, "hodei.gc.tombstone.actor")?,
            account: node_text(&node, "hodei.gc.tombstone.account")?,
            state,
            at: parse_stamp(
                "hodei.gc.tombstone.at",
                &node_text(&node, "hodei.gc.tombstone.at")?,
            )?,
            marker: node_option_text(&node, "hodei.gc.tombstone.marker"),
        }))
    }

    async fn verdict_tombstone(&self, deletion: &GcDeletion) -> Result<(), CatalogError> {
        let back = self.deletion_of_here(&deletion.digest).await?;
        match back {
            Some(row) if row.state == deletion.state && row.run_id == deletion.run_id => Ok(()),
            Some(_) => Err(CatalogError::precondition_failed(
                "gc tombstone",
                deletion.digest.to_string(),
                "the trace did not land under the presented token",
            )),
            None => Err(CatalogError::unavailable(
                "the trace is not readable after commit",
            )),
        }
    }

    /// La cuarentena fenced (AC2): los bytes marcados se copiaron a un
    /// nombre NUEVO y la clave original queda reservada en el libro
    /// hasta reconciliar — la reserva es el rastro de cuarentena.
    pub async fn record_quarantine_here(
        &self,
        scope: &str,
        lease: &GcLease,
        quarantine: &Quarantined,
    ) -> Result<(), CatalogError> {
        self.the_fence(scope, lease, Utc::now()).await?;
        let qkey = format!(
            "quar|{}|{}",
            key_part(&quarantine.run_id),
            digest_key(&quarantine.digest)
        );
        let create = quarantine_nquads("_:qz", &qkey, quarantine, true);
        let update = quarantine_nquads("uid(qz)", &qkey, quarantine, false);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {})) \
                 qz as var(func: eq(hodei.gc.quarantine.key, {})) }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&lease.token),
                dql_string(&qkey),
            ),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(gl), 1) AND eq(len(qz), 0)", create),
                GraphMutationBlock::set_if("eq(len(gl), 1) AND eq(len(qz), 1)", update),
            ],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        let back = self
            .quarantines_of_run_here(&quarantine.run_id)
            .await?
            .into_iter()
            .any(|item| item.digest == quarantine.digest);
        if back {
            Ok(())
        } else {
            Err(CatalogError::precondition_failed(
                "gc quarantine",
                quarantine.digest.to_string(),
                "the quarantine did not land under the presented token",
            ))
        }
    }

    /// Las cuarentenas de una pasada (reserva de clave, reconciliacion).
    pub async fn quarantines_of_run_here(
        &self,
        run_id: &str,
    ) -> Result<Vec<Quarantined>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.gc.quarantine.run_id, {})) {{ \
             hodei.gc.quarantine.digest_of hodei.gc.quarantine.length \
             hodei.gc.quarantine.quarantine_key hodei.gc.quarantine.marker \
             hodei.gc.quarantine.at }} }}",
            dql_string(run_id),
        );
        let json = self.execute_dql_row(&query).await?;
        let nodes = json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut items = Vec::new();
        for node in &nodes {
            let digest_of = node_text(node, "hodei.gc.quarantine.digest_of")?;
            let digest = split_digest_key(&digest_of).ok_or_else(|| {
                CatalogError::corrupt("gc quarantine", digest_of, "the key is not a digest")
            })?;
            items.push(Quarantined {
                digest,
                length: node_u64(node, "hodei.gc.quarantine.length")?,
                run_id: run_id.to_owned(),
                quarantine_key: node_text(node, "hodei.gc.quarantine.quarantine_key")?,
                marker: node_text(node, "hodei.gc.quarantine.marker")?,
                at: parse_stamp(
                    "hodei.gc.quarantine.at",
                    &node_text(node, "hodei.gc.quarantine.at")?,
                )?,
            });
        }
        Ok(items)
    }

    // -----------------------------------------------------------------------
    // Censo paginado (AC3).
    // -----------------------------------------------------------------------

    /// Una pagina acotada de raices del scope, ordenada por digests.
    ///
    /// Cada familia del plano de datos del owner se lee con
    /// `first(limit+1)`, `order: digest` y cursor estricto `gt`; las
    /// paginas son disjuntas y convergentes. El corte puede empatar el
    /// ultimo digests de la pagina con filas de otra familia: esas
    /// filas se completan en la MISMA pagina y el cursor las deja
    /// atras, o `has_more` seguiria prometiendo una pagina infinita.
    /// El cursor de salida es la impresion del ultimo digests de la
    /// pagina; `has_more` solo es cierto si queda algo despues del
    /// limite.
    pub async fn root_page_here(
        &self,
        scope: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<RootPage, CatalogError> {
        let limit = limit.max(1);
        let account = scope_owner(scope);
        let cursor = after.map(cursor_of_digest).transpose()?;
        let mut fetched: Vec<CensusRow> = Vec::new();
        for family in [
            Family::Asset,
            Family::Version,
            Family::Reference,
            Family::AptSnapshot,
        ] {
            fetched.extend(
                self.census_window(&account, family, cursor.as_ref(), limit + 1)
                    .await?,
            );
        }
        fetched.sort_by(|a, b| {
            a.digest
                .value
                .cmp(&b.digest.value)
                .then_with(|| a.row_key.cmp(&b.row_key))
        });
        if fetched.len() <= limit {
            // La ventana entera cabe: son todas las raices que hay.
            return Ok(RootPage {
                roots: fetched.into_iter().map(|row| row.root).collect(),
                has_more: false,
            });
        }
        fetched.truncate(limit);
        let boundary = fetched[limit - 1].digest.value.clone();
        // Empates del limite: las filas de otra familia con el mismo
        // digests del corte se completan en la MISMA pagina — el
        // cursor de salida las deja atras y si no se completaran,
        // `has_more` prometeria una pagina que siempre devuelve el
        // mismo digests. Se leen de la autoridad por igualdad exacta,
        // no de la ventana truncada.
        for family in [
            Family::Asset,
            Family::Version,
            Family::Reference,
            Family::AptSnapshot,
        ] {
            for row in self.census_equal(&account, family, &boundary).await? {
                if !fetched.iter().any(|seen| seen.row_key == row.row_key) {
                    fetched.push(row);
                }
            }
        }
        fetched.sort_by(|a, b| {
            a.digest
                .value
                .cmp(&b.digest.value)
                .then_with(|| a.row_key.cmp(&b.row_key))
        });
        // Hay mas si algo, en cualquier familia, queda estrictamente
        // despues del digests limite.
        let mut has_more = false;
        for family in [
            Family::Asset,
            Family::Version,
            Family::Reference,
            Family::AptSnapshot,
        ] {
            if self.census_any_gt(&account, family, &boundary).await? {
                has_more = true;
                break;
            }
        }
        Ok(RootPage {
            roots: fetched.into_iter().map(|row| row.root).collect(),
            has_more,
        })
    }

    /// La ventana de una familia estrictamente despues del cursor:
    /// `first: {window}` + `orderasc: digest` + `@filter(gt(digest,
    /// cursor))` — el idiom de paginado del repo, nunca un scan sin
    /// cota. `after:` no vale aqui: en DQL corta por UID, no por el
    /// valor del orden. El `has` y el `gt` comparten UN solo
    /// `@filter` unido por AND: dos filtros en la raiz son "Repeated
    /// filter at root".
    async fn census_window(
        &self,
        account: &str,
        family: Family,
        cursor: Option<&ContentDigest>,
        window: usize,
    ) -> Result<Vec<CensusRow>, CatalogError> {
        let filter = match cursor {
            Some(digest) => format!(
                "has({}.digest) AND gt({}.digest, {})",
                family.prefix(),
                family.prefix(),
                dql_string(&digest.value)
            ),
            None => format!("has({}.digest)", family.prefix()),
        };
        let query = format!(
            "{{ q(func: eq({}.account, {}), orderasc: {}.digest, first: {}) \
             @filter({}) {{ \
             {}.key {}.account {}.repository {}.digest_algorithm {}.digest {} }} }}",
            family.prefix(),
            dql_string(account),
            family.prefix(),
            window,
            filter,
            family.prefix(),
            family.prefix(),
            family.prefix(),
            family.prefix(),
            family.prefix(),
            family.detail(),
        );
        let json = self.execute_dql_row(&query).await?;
        json.get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|node| CensusRow::from_node(family, node))
            .collect()
    }

    /// Las filas de una familia con el digests del corte exacto.
    async fn census_equal(
        &self,
        account: &str,
        family: Family,
        value: &str,
    ) -> Result<Vec<CensusRow>, CatalogError> {
        let query = format!(
            "{{ q(func: eq({}.account, {})) @filter(eq({}.digest, {})) {{ \
             {}.key {}.account {}.repository {}.digest_algorithm {}.digest {} }} }}",
            family.prefix(),
            dql_string(account),
            family.prefix(),
            dql_string(value),
            family.prefix(),
            family.prefix(),
            family.prefix(),
            family.prefix(),
            family.prefix(),
            family.detail(),
        );
        let json = self.execute_dql_row(&query).await?;
        json.get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|node| CensusRow::from_node(family, node))
            .collect()
    }

    /// Queda algo estrictamente despues de este digests en la familia.
    async fn census_any_gt(
        &self,
        account: &str,
        family: Family,
        value: &str,
    ) -> Result<bool, CatalogError> {
        let query = format!(
            "{{ q(func: eq({}.account, {}), first: 1) @filter(gt({}.digest, {})) {{ uid }} }}",
            family.prefix(),
            dql_string(account),
            family.prefix(),
            dql_string(value),
        );
        let json = self.execute_dql_row(&query).await?;
        Ok(json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|array| !array.is_empty()))
    }

    /// Cuantas filas del plano de datos nombran este digests, leido
    /// fresco por membresia — nunca por scan global (revalidacion
    /// post-censo, guarda de referencias). El indice `exact` de
    /// `{family}.digest` resuelve la consulta por clave; no se
    /// materializa nada: solo se cuenta la respuesta.
    pub async fn live_reference_count_here(
        &self,
        digest: &ContentDigest,
    ) -> Result<u64, CatalogError> {
        let mut total = 0u64;
        for family in [
            Family::Asset,
            Family::Version,
            Family::Reference,
            Family::AptSnapshot,
        ] {
            let query = format!(
                "{{ q(func: eq({}.digest, {})) {{ uid }} }}",
                family.prefix(),
                dql_string(&digest.value),
            );
            let json = self.execute_dql_row(&query).await?;
            total += json
                .get("q")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default()
                .len() as u64;
        }
        Ok(total)
    }

    // -----------------------------------------------------------------------
    // Lecturas internas compartidas.
    // -----------------------------------------------------------------------

    async fn the_run_row(&self, run_id: &str) -> Result<Option<GcRun>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.gc.run.run_id, {})) {{ \
             hodei.gc.run.scope hodei.gc.run.generation hodei.gc.run.state \
             hodei.gc.run.actor hodei.gc.run.grace_secs hodei.gc.run.deleted \
             hodei.gc.run.bytes_freed hodei.gc.run.marked_at hodei.gc.run.swept_at \
             hodei.gc.run.candidate @filter(has(hodei.gc.candidate.key)) {{ \
             hodei.gc.candidate.digest_of hodei.gc.candidate.length }} }} }}",
            dql_string(run_id),
        );
        let json = self.execute_dql_row(&query).await?;
        let node = single_row(json.get("q"), "gc run", run_id.to_owned())?;
        let Some(node) = node else {
            return Ok(None);
        };
        let state = match node_text(&node, "hodei.gc.run.state")?.as_str() {
            "marked" => RunState::Marked,
            "swept" => RunState::Swept,
            other => {
                return Err(CatalogError::corrupt(
                    "gc run",
                    other.to_owned(),
                    "not a run state",
                ));
            }
        };
        let mut candidates = Vec::new();
        for item in node
            .get("hodei.gc.run.candidate")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let digest_of = node_text(&item, "hodei.gc.candidate.digest_of")?;
            let digest = split_digest_key(&digest_of).ok_or_else(|| {
                CatalogError::corrupt("gc candidate", digest_of, "the digest key is corrupt")
            })?;
            candidates.push(RunCandidate {
                digest,
                length: node_u64(&item, "hodei.gc.candidate.length")?,
            });
        }
        Ok(Some(GcRun {
            id: run_id.to_owned(),
            state,
            grace_secs: node_u64(&node, "hodei.gc.run.grace_secs")?,
            actor: node_text(&node, "hodei.gc.run.actor")?,
            candidates,
            marked_at: parse_stamp(
                "hodei.gc.run.marked_at",
                &node_text(&node, "hodei.gc.run.marked_at")?,
            )?,
            deleted: node_u64(&node, "hodei.gc.run.deleted")?,
            bytes_freed: node_u64(&node, "hodei.gc.run.bytes_freed")?,
            swept_at: node_option_text(&node, "hodei.gc.run.swept_at")
                .map(|text| parse_stamp("hodei.gc.run.swept_at", &text))
                .transpose()?,
            scope: node_option_text(&node, "hodei.gc.run.scope"),
            generation: Some(node_u64(&node, "hodei.gc.run.generation")?),
        }))
    }
}

/// Las lineas de query de la guarda de referencias de un digest: una
/// variable por familia del plano de datos, ligada por igualdad exacta
/// de digest. Cuentan en el snapshot de la transaccion; el bloque las
/// nombra en su condicion para que DQL no las descarte como muertas.
fn reference_guard_lines(digest: &ContentDigest) -> String {
    [
        Family::Asset,
        Family::Version,
        Family::Reference,
        Family::AptSnapshot,
    ]
    .iter()
    .enumerate()
    .map(|(index, family)| {
        format!(
            "rv{} as var(func: eq({}.digest, {}))",
            index,
            family.prefix(),
            dql_string(&digest.value)
        )
    })
    .collect::<Vec<_>>()
    .join(" \n")
}

/// La condición de la guarda: ninguna fila nombra el digest.
fn no_live_reference() -> String {
    "eq(len(rv0), 0) AND eq(len(rv1), 0) AND eq(len(rv2), 0) AND eq(len(rv3), 0)".to_owned()
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{no_live_reference, reference_guard_lines};
    use hodei_evidence::{ContentDigest, DigestAlgorithm};

    #[test]
    fn deletion_guard_checks_every_live_root_family() {
        let digest = ContentDigest {
            algorithm: DigestAlgorithm::Sha256,
            value: "a".repeat(64),
        };
        let guards = reference_guard_lines(&digest);
        let condition = no_live_reference();

        for index in 0..4 {
            assert!(guards.contains(&format!("rv{index} as var(")));
            assert!(condition.contains(&format!("eq(len(rv{index}), 0)")));
        }
        assert!(guards.contains("hodei.catalog.apt_snapshot_root.digest"));
    }
}

/// Los n-quads de la marca de una pasada: el run, sus candidatos y las
/// aristas run→candidato. Devuelve el conjunto y la clave del run.
fn run_mark_nquads(scope: &str, lease: &GcLease, run: &GcRun) -> (String, String) {
    let key = gc_run_key(&run.id);
    let mut set = format!(
        "_:ru <dgraph.type> \"{GC_RUN_TYPE}\" .\n\
         _:ru <hodei.gc.run.key> {key_q} .\n\
         _:ru <hodei.gc.run.run_id> {id_q} .\n\
         _:ru <hodei.gc.run.scope> {scope_q} .\n\
         _:ru <hodei.gc.run.generation> \"{generation}\" .\n\
         _:ru <hodei.gc.run.state> \"marked\" .\n\
         _:ru <hodei.gc.run.actor> {actor_q} .\n\
         _:ru <hodei.gc.run.grace_secs> \"{grace}\" .\n\
         _:ru <hodei.gc.run.deleted> \"0\" .\n\
         _:ru <hodei.gc.run.bytes_freed> \"0\" .\n\
         _:ru <hodei.gc.run.marked_at> \"{marked}\" .\n",
        key_q = dql_string(&key),
        id_q = dql_string(&run.id),
        scope_q = dql_string(scope),
        generation = lease.generation,
        actor_q = dql_string(&run.actor),
        grace = run.grace_secs,
        marked = stamp(run.marked_at),
    );
    for candidate in &run.candidates {
        let dkey = gc_candidate_key(&candidate.digest, &run.id);
        let blank = blank_of(&dkey);
        set.push_str(&format!(
            "{blank} <hodei.gc.candidate.key> {dkey_q} .\n\
             {blank} <hodei.gc.candidate.digest_of> {digest_q} .\n\
             {blank} <hodei.gc.candidate.run_id> {id_q} .\n\
             {blank} <hodei.gc.candidate.length> \"{length}\" .\n\
             _:ru <hodei.gc.run.candidate> {blank} .\n",
            dkey_q = dql_string(&dkey),
            digest_q = dql_string(&digest_key(&candidate.digest)),
            id_q = dql_string(&run.id),
            length = candidate.length,
        ));
    }
    (set, key)
}

/// Los n-quads del rastro de un borrado.
fn tombstone_nquads(subject: &str, tkey: &str, deletion: &GcDeletion, create: bool) -> String {
    let type_line = if create {
        format!("{subject} <dgraph.type> \"{GC_TOMBSTONE_TYPE}\" .\n")
    } else {
        String::new()
    };
    let key_line = if create {
        format!(
            "{subject} <hodei.gc.tombstone.key> {} .\n",
            dql_string(tkey)
        )
    } else {
        String::new()
    };
    let marker = deletion
        .marker
        .as_ref()
        .map(|value| {
            format!(
                "{subject} <hodei.gc.tombstone.marker> {} .\n",
                dql_string(value)
            )
        })
        .unwrap_or_default();
    format!(
        "{type_line}{key_line}\
         {subject} <hodei.gc.tombstone.digest_of> {digest_q} .\n\
         {subject} <hodei.gc.tombstone.algorithm> {alg_q} .\n\
         {subject} <hodei.gc.tombstone.digest> {value_q} .\n\
         {subject} <hodei.gc.tombstone.run_id> {run_q} .\n\
         {subject} <hodei.gc.tombstone.length> \"{length}\" .\n\
         {subject} <hodei.gc.tombstone.actor> {actor_q} .\n\
         {subject} <hodei.gc.tombstone.account> {account_q} .\n\
         {subject} <hodei.gc.tombstone.state> {state_q} .\n\
         {subject} <hodei.gc.tombstone.at> \"{at}\" .\n\
         {marker}",
        digest_q = dql_string(&digest_key(&deletion.digest)),
        alg_q = dql_string(&deletion.digest.algorithm.to_string()),
        value_q = dql_string(&deletion.digest.value),
        run_q = dql_string(&deletion.run_id),
        length = deletion.length,
        actor_q = dql_string(&deletion.actor),
        account_q = dql_string(&deletion.account),
        state_q = dql_string(deletion.state.as_str()),
        at = stamp(deletion.at),
    )
}

/// Los n-quads de una cuarentena versionada.
fn quarantine_nquads(subject: &str, qkey: &str, quarantine: &Quarantined, create: bool) -> String {
    let type_line = if create {
        format!("{subject} <dgraph.type> \"{GC_QUARANTINE_TYPE}\" .\n")
    } else {
        String::new()
    };
    let key_line = if create {
        format!(
            "{subject} <hodei.gc.quarantine.key> {} .\n",
            dql_string(qkey)
        )
    } else {
        String::new()
    };
    format!(
        "{type_line}{key_line}\
         {subject} <hodei.gc.quarantine.run_id> {run_q} .\n\
         {subject} <hodei.gc.quarantine.digest_of> {digest_q} .\n\
         {subject} <hodei.gc.quarantine.length> \"{length}\" .\n\
         {subject} <hodei.gc.quarantine.quarantine_key> {qk_q} .\n\
         {subject} <hodei.gc.quarantine.marker> {marker_q} .\n\
         {subject} <hodei.gc.quarantine.at> \"{at}\" .\n",
        run_q = dql_string(&quarantine.run_id),
        digest_q = dql_string(&digest_key(&quarantine.digest)),
        length = quarantine.length,
        qk_q = dql_string(&quarantine.quarantine_key),
        marker_q = dql_string(&quarantine.marker),
        at = stamp(quarantine.at),
    )
}

// ---------------------------------------------------------------------------
// El contrato del libro sobre el grafo.
// ---------------------------------------------------------------------------

#[async_trait]
impl ContentLedger for DgraphCatalogStore {
    fn supports_fenced_pass(&self) -> bool {
        true
    }

    /// El censo global materializado NO existe en esta autoridad:
    /// seria el scan prohibido por HSP-220. La pasada coordinada usa
    /// `root_page`, que recorre por paginas acotadas por scope.
    async fn content_roots(&self) -> Result<Vec<ContentRoot>, CatalogError> {
        Err(CatalogError::unavailable(
            "content_roots materializes the whole census: use the paged root_page by scope instead (HSP-220)",
        ))
    }

    async fn holds(&self) -> Result<Vec<ContentHold>, CatalogError> {
        let query = "{ q(func: has(hodei.gc.hold.hold_id), orderasc: hodei.gc.hold.hold_id) { \
             hodei.gc.hold.hold_id hodei.gc.hold.scope_kind hodei.gc.hold.account \
             hodei.gc.hold.repository hodei.gc.hold.digest_of hodei.gc.hold.reason \
             hodei.gc.hold.actor hodei.gc.hold.created_at hodei.gc.hold.expires_at } }";
        let json = self.execute_dql_row(query).await?;
        let nodes = json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut holds = Vec::new();
        for node in &nodes {
            let id = node_text(node, "hodei.gc.hold.hold_id")?;
            let scope = match node_text(node, "hodei.gc.hold.scope_kind")?.as_str() {
                "digest" => {
                    let digest_of = node_text(node, "hodei.gc.hold.digest_of")?;
                    let digest = split_digest_key(&digest_of).ok_or_else(|| {
                        CatalogError::corrupt("gc hold", digest_of, "the digest key is corrupt")
                    })?;
                    HoldScope::Digest { digest }
                }
                "repository" => HoldScope::Repository {
                    account: node_text(node, "hodei.gc.hold.account")?,
                    repository: node_text(node, "hodei.gc.hold.repository")?,
                },
                "account" => HoldScope::Account {
                    account: node_text(node, "hodei.gc.hold.account")?,
                },
                other => {
                    return Err(CatalogError::corrupt(
                        "gc hold",
                        id,
                        format!("'{other}' is not a hold scope"),
                    ));
                }
            };
            holds.push(ContentHold {
                id,
                scope,
                reason: node_text(node, "hodei.gc.hold.reason")?,
                actor: node_text(node, "hodei.gc.hold.actor")?,
                created_at: parse_stamp(
                    "hodei.gc.hold.created_at",
                    &node_text(node, "hodei.gc.hold.created_at")?,
                )?,
                expires_at: node_option_text(node, "hodei.gc.hold.expires_at")
                    .map(|text| parse_stamp("hodei.gc.hold.expires_at", &text))
                    .transpose()?,
            });
        }
        Ok(holds)
    }

    async fn place_hold(&self, hold: &ContentHold) -> Result<(), CatalogError> {
        let (scope_kind, account, repository, digest_of) = match &hold.scope {
            HoldScope::Digest { digest } => ("digest", None, None, Some(digest_key(digest))),
            HoldScope::Repository {
                account,
                repository,
            } => (
                "repository",
                Some(account.clone()),
                Some(repository.clone()),
                None,
            ),
            HoldScope::Account { account } => ("account", Some(account.clone()), None, None),
        };
        let key = format!("hold|{}", key_part(&hold.id));
        // El cuerpo va sujeto a un blank node en la creacion y al uid
        // ligado en la actualizacion: la misma fila escrita dos veces
        // por dos escritores no deja campos viejos del scope anterior.
        let body = |subject: &str| -> String {
            let mut set = format!(
                "{subject} <hodei.gc.hold.hold_id> {id_q} .\n\
                 {subject} <hodei.gc.hold.scope_kind> {scope_q} .\n\
                 {subject} <hodei.gc.hold.reason> {reason_q} .\n\
                 {subject} <hodei.gc.hold.actor> {actor_q} .\n\
                 {subject} <hodei.gc.hold.created_at> \"{created}\" .\n",
                id_q = dql_string(&hold.id),
                scope_q = dql_string(scope_kind),
                reason_q = dql_string(&hold.reason),
                actor_q = dql_string(&hold.actor),
                created = stamp(hold.created_at),
            );
            for (predicate, value) in [
                ("hodei.gc.hold.account", account.clone()),
                ("hodei.gc.hold.repository", repository.clone()),
                ("hodei.gc.hold.digest_of", digest_of.clone()),
            ] {
                if let Some(value) = value {
                    set.push_str(&format!(
                        "{subject} <{predicate}> {} .\n",
                        dql_string(&value)
                    ));
                }
            }
            if let Some(expires) = hold.expires_at {
                set.push_str(&format!(
                    "{subject} <hodei.gc.hold.expires_at> \"{}\" .\n",
                    stamp(expires)
                ));
            }
            set
        };
        let delete = "uid(hl) <hodei.gc.hold.account> * .\n\
             uid(hl) <hodei.gc.hold.repository> * .\n\
             uid(hl) <hodei.gc.hold.digest_of> * .\n\
             uid(hl) <hodei.gc.hold.expires_at> * .\n";
        let create = format!(
            "_:hl <dgraph.type> \"{GC_HOLD_TYPE}\" .\n\
             _:hl <hodei.gc.hold.key> {key_q} .\n{}",
            body("_:hl"),
            key_q = dql_string(&key),
        );
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ hl as var(func: eq(hodei.gc.hold.key, {})) }}",
                dql_string(&key),
            ),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(hl), 0)", create),
                GraphMutationBlock {
                    cond: Some("eq(len(hl), 1)".to_owned()),
                    set_nquads: body("uid(hl)"),
                    delete_nquads: delete.to_owned(),
                },
            ],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        Ok(())
    }

    async fn release_hold(&self, id: &str) -> Result<bool, CatalogError> {
        let key = format!("hold|{}", key_part(id));
        let exists = {
            let json = self
                .execute_dql_row(&format!(
                    "{{ q(func: eq(hodei.gc.hold.key, {})) {{ uid }} }}",
                    dql_string(&key),
                ))
                .await?;
            json.get("q")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|array| !array.is_empty())
        };
        if !exists {
            return Ok(false);
        }
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ hl as var(func: eq(hodei.gc.hold.key, {})) }}",
                dql_string(&key),
            ),
            blocks: vec![GraphMutationBlock {
                cond: Some("eq(len(hl), 1)".to_owned()),
                set_nquads: String::new(),
                delete_nquads: "uid(hl) * .\n".to_owned(),
            }],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        Ok(true)
    }

    /// La marca pendiente global NO existe en esta autoridad: la
    /// pasada legacy sin scope no tiene donde apoyarse aqui. El modo
    /// coordinado usa `pending_scoped_run`.
    async fn pending_run(&self) -> Result<Option<GcRun>, CatalogError> {
        Err(CatalogError::unavailable(
            "pending_run is a global scan: the Dgraph ledger only knows scoped pending runs (pending_scoped_run)",
        ))
    }

    async fn last_swept_run(&self) -> Result<Option<GcRun>, CatalogError> {
        let query = "{ q(func: eq(hodei.gc.run.state, \"swept\"), orderasc: hodei.gc.run.swept_at) { \
             hodei.gc.run.run_id } }";
        let json = self.execute_dql_row(query).await?;
        let nodes = json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let run_id = nodes
            .iter()
            .filter_map(|node| node.get("hodei.gc.run.run_id").and_then(|v| v.as_str()))
            .max()
            .map(str::to_owned);
        match run_id {
            Some(run_id) => self.the_run_row(&run_id).await,
            None => Ok(None),
        }
    }

    /// Abrir una pasada sin claim solo vale si ya existe un claim
    /// vigente para su generacion — si no, el modo legacy no tiene
    /// donde apoyarse en esta autoridad.
    async fn open_run(&self, run: &GcRun) -> Result<(), CatalogError> {
        let Some(scope) = run.scope.as_deref() else {
            return Err(CatalogError::unavailable(
                "open_run without a scope needs a coordinated claim (HSP-192)",
            ));
        };
        // La marca solo se abre bajo el claim VIGENTE del scope: sin
        // lease vivo no hay generacion que abrir, y un lease vencido
        // es la sena de que otra replica manda ahora.
        let now = Utc::now();
        let row = self
            .lease_row(&gc_lease_key(scope))
            .await?
            .ok_or_else(|| CatalogError::precondition_failed("gc lease", scope, "no live lease"))?;
        let live = lease_from(&row)?;
        if live.lease_until <= now {
            return Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                "the lease expired; its generation no longer rules",
            ));
        }
        let generation = run.generation.unwrap_or(live.generation);
        if generation != live.generation {
            return Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                format!(
                    "the run names generation {generation} but the scope lives in generation {}",
                    live.generation
                ),
            ));
        }
        let (set, _) = run_mark_nquads(scope, &live, run);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {}) AND gt(hodei.gc.lease.lease_until, \"{}\")) }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&live.token),
                stamp(now),
            ),
            blocks: vec![GraphMutationBlock::set_if("eq(len(gl), 1)", set)],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        Ok(())
    }

    async fn complete_run(
        &self,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ ru as var(func: eq(hodei.gc.run.run_id, {})) }}",
                dql_string(run_id),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(ru), 1)",
                format!(
                    "uid(ru) <hodei.gc.run.state> \"swept\" .\n\
                     uid(ru) <hodei.gc.run.deleted> \"{deleted}\" .\n\
                     uid(ru) <hodei.gc.run.bytes_freed> \"{bytes}\" .\n\
                     uid(ru) <hodei.gc.run.swept_at> \"{at}\" .\n",
                    bytes = bytes_freed,
                    at = stamp(at),
                ),
            )],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        Ok(())
    }

    /// El rastro legacy, sin claim: igual que el coordinado pero sin
    /// la guarda del lease. La revalidacion es del llamante.
    async fn record_deletion(&self, deletion: &GcDeletion) -> Result<(), CatalogError> {
        let tkey = format!("tomb|{}", digest_key(&deletion.digest));
        let create = tombstone_nquads("_:td", &tkey, deletion, true);
        let update = tombstone_nquads("uid(td)", &tkey, deletion, false);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ td as var(func: eq(hodei.gc.tombstone.key, {})) }}",
                dql_string(&tkey),
            ),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(td), 0)", create),
                GraphMutationBlock::set_if("eq(len(td), 1)", update),
            ],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        self.verdict_tombstone(deletion).await
    }

    async fn deletion_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<GcDeletion>, CatalogError> {
        self.deletion_of_here(digest).await
    }

    async fn claim_gc_pass(
        &self,
        scope: &str,
        owner: &str,
        lease: std::time::Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        self.claim_gc_pass_here(scope, owner, lease, now).await
    }

    async fn current_gc_lease(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcLease>, CatalogError> {
        self.current_gc_lease_here(scope, now).await
    }

    async fn mark_run_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        run: &GcRun,
        candidate_digests: &[ContentDigest],
    ) -> Result<Vec<RunCandidate>, CatalogError> {
        // La guarda de referencias vive EN la marca: se preguntan
        // antes del commit por cada digests que el plan propuso.
        for digest in candidate_digests {
            let live = self.live_reference_count_here(digest).await?;
            if live > 0 {
                return Err(CatalogError::precondition_failed(
                    "gc mark",
                    digest.to_string(),
                    "a candidate gained a live reference while marking",
                ));
            }
        }
        self.mark_run_fenced_here(scope, lease, run).await
    }

    async fn mark_of_run(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
    ) -> Result<Option<GcRun>, CatalogError> {
        self.mark_of_run_here(scope, lease, run_id).await
    }

    async fn record_deletion_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        deletion: &GcDeletion,
    ) -> Result<(), CatalogError> {
        self.record_deletion_fenced_here(scope, lease, deletion)
            .await
    }

    async fn record_quarantine(
        &self,
        scope: &str,
        lease: &GcLease,
        quarantine: &Quarantined,
    ) -> Result<(), CatalogError> {
        self.record_quarantine_here(scope, lease, quarantine).await
    }

    async fn root_page(
        &self,
        scope: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<RootPage, CatalogError> {
        self.root_page_here(scope, after, limit).await
    }

    async fn quarantines_of_run(&self, run_id: &str) -> Result<Vec<Quarantined>, CatalogError> {
        self.quarantines_of_run_here(run_id).await
    }

    async fn pending_scoped_run(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcRun>, CatalogError> {
        self.pending_scoped_run_here(scope, now).await
    }

    async fn renew_gc_lease(
        &self,
        scope: &str,
        lease: &GcLease,
        owner: &str,
        lease_for: std::time::Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        self.renew_gc_lease_here(scope, lease, owner, lease_for, now)
            .await
    }

    async fn live_reference_count(&self, digest: &ContentDigest) -> Result<u64, CatalogError> {
        self.live_reference_count_here(digest).await
    }

    async fn complete_scoped_run(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        _quarantined: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        self.complete_scoped_run_here(scope, lease, run_id, deleted, bytes_freed, at)
            .await
    }
}
