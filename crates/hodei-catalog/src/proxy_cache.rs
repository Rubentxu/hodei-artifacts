//! La politica de cache de un repositorio proxy y la decision de frescura.
//!
//! Este modulo es **puro**: no toca la red, el reloj ni el almacen. Recibe
//! la politica, lo que el libro de procedencia sabe de una ruta y la hora, y
//! contesta una pregunta: que hay que hacer con esta peticion. Quien ejecuta
//! la decision (traer, revalidar, servir, rechazar) es el servicio.
//!
//! # La configuracion
//!
//! Vive en `config_settings.proxy.cache`, junto a `proxy.upstream`:
//!
//! ```json
//! { "proxy": { "cache": {
//!     "ttl": { "content_secs": null, "metadata_secs": 300 },
//!     "metadata_paths": ["maven-metadata.xml", "dists/"],
//!     "negative_secs": 60,
//!     "stale_if_error_secs": 3600,
//!     "offline": false
//! } } }
//! ```
//!
//! - **Dos clases de ruta.** El *contenido* (un jar, una capa) se identifica
//!   por sus bytes y no cambia: su TTL por defecto es "no caduca". La
//!   *metadata* (un indice, un `maven-metadata.xml`) es mutable por
//!   naturaleza: es lo que se revalida. Una entrada de `metadata_paths` sin
//!   `/` final es el **nombre del ultimo segmento**; con `/` final, un
//!   prefijo de ruta.
//! - **TTL.** Ausente = no caduca (nunca se revalida por tiempo); `0` =
//!   se revalida en cada lectura. Un TTL que vence no sirve nada por si
//!   mismo: dispara una revalidacion condicional (`If-None-Match`,
//!   `If-Modified-Since`) y un `304` renueva el recibo sin mover bytes.
//! - **`stale_if_error_secs`.** Cuanto tiempo **mas alla** del TTL se sirve
//!   el contenido cacheado si el upstream falla. `0` (por defecto) = nunca
//!   se sirve contenido caducado.
//! - **`negative_secs`.** Cuanto se recuerda un `404`. `0` (por defecto) =
//!   no se recuerda: un `404` no es una garantia de ausencia permanente.
//! - **`offline`.** El repositorio no sale a la red: sirve solo lo que ya
//!   tiene con procedencia.

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use serde::Deserialize;
use serde_json::Value;

use crate::model::RepositoryFormat;
use crate::proxy::{ChecksumEvidence, ProvenanceReceipt, ReceiptOutcome, UpstreamConfigError};

/// El tope de cualquier duracion de la politica: diez años.
pub const MAX_CACHE_SECS: u64 = 315_360_000;

/// Los nombres de metadata que se reconocen si la politica no declara
/// `metadata_paths`: indices que los formatos habituales republican.
pub const DEFAULT_METADATA_NAMES: &[&str] = &[
    "maven-metadata.xml",
    "repomd.xml",
    "Packages",
    "Packages.gz",
    "Packages.xz",
    "Sources",
    "Sources.gz",
    "Sources.xz",
    "Release",
    "Release.gpg",
    "InRelease",
    "index.json",
];

/// Sufijos de los ficheros que acompañan a una metadata (su checksum o su
/// firma): se republican con ella y se tratan igual.
const SIDECAR_SUFFIXES: &[&str] = &[
    ".md5", ".sha1", ".sha256", ".sha512", ".asc", ".sig", ".gpg",
];

fn invalid(reason: impl Into<String>) -> UpstreamConfigError {
    UpstreamConfigError(reason.into())
}

/// Si la ruta es un fichero de una version `-SNAPSHOT` con el nombre
/// flotante de Maven (`.../1.0-SNAPSHOT/widget-1.0-SNAPSHOT.jar`).
///
/// Ese nombre apunta a la ultima construccion y el upstream lo republica:
/// un cambio es lo esperado, no un incumplimiento. Los ficheros con marca
/// de tiempo (`widget-1.0-20240101.120000-1.jar`) no llevan el nombre de
/// la version y siguen siendo inmutables.
fn is_floating_snapshot(asset_path: &str) -> bool {
    let mut segments = asset_path.rsplit('/');
    let (Some(file), Some(directory), Some(artifact)) =
        (segments.next(), segments.next(), segments.next())
    else {
        return false;
    };
    if !directory.ends_with("-SNAPSHOT") {
        return false;
    }
    // `{artifactId}-{version}` y despues la extension o un clasificador.
    let floating = format!("{artifact}-{directory}");
    file.strip_prefix(floating.as_str())
        .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('-'))
}

/// Que clase de ruta es: de eso depende como se trata un cambio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    /// Bytes que se identifican por su digest y no deben cambiar.
    ///
    /// Un upstream que entrega **otros** bytes para la misma ruta no esta
    /// actualizando: esta incumpliendo, y el contenido se pone en
    /// cuarentena.
    Content,
    /// Un indice que el upstream republica. Un cambio es lo esperado.
    Metadata,
}

/// La politica de cache de un repositorio proxy, ya validada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachePolicy {
    /// Segundos de frescura del contenido. `None` = no caduca.
    pub content_ttl: Option<u64>,
    /// Segundos de frescura de la metadata. `None` = no caduca.
    pub metadata_ttl: Option<u64>,
    /// Nombres de ultimo segmento (sin `/` final) y prefijos (con `/`
    /// final) que son metadata.
    pub metadata_paths: Vec<String>,
    /// Segundos que se recuerda un `404`. `0` = no se recuerda.
    pub negative_secs: u64,
    /// Segundos mas alla del TTL en que se sirve contenido si el upstream
    /// falla. `0` = nunca.
    pub stale_if_error_secs: u64,
    /// Si el repositorio trabaja sin red.
    pub offline: bool,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            content_ttl: None,
            metadata_ttl: None,
            metadata_paths: DEFAULT_METADATA_NAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            negative_secs: 0,
            stale_if_error_secs: 0,
            offline: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCache {
    #[serde(default)]
    ttl: Option<RawTtl>,
    #[serde(default)]
    metadata_paths: Option<Vec<String>>,
    #[serde(default)]
    negative_secs: Option<u64>,
    #[serde(default)]
    stale_if_error_secs: Option<u64>,
    #[serde(default)]
    offline: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTtl {
    #[serde(default)]
    content_secs: Option<u64>,
    #[serde(default)]
    metadata_secs: Option<u64>,
}

impl CachePolicy {
    /// Valida la politica escrita como JSON.
    pub fn parse(value: &Value) -> Result<Self, UpstreamConfigError> {
        let raw: RawCache = serde_json::from_value(value.clone())
            .map_err(|error| invalid(format!("the cache settings cannot be read: {error}")))?;

        let (content_ttl, metadata_ttl) = match raw.ttl {
            Some(ttl) => (ttl.content_secs, ttl.metadata_secs),
            None => (None, None),
        };
        let negative_secs = raw.negative_secs.unwrap_or(0);
        let stale_if_error_secs = raw.stale_if_error_secs.unwrap_or(0);
        for (name, seconds) in [
            ("ttl.content_secs", content_ttl.unwrap_or(0)),
            ("ttl.metadata_secs", metadata_ttl.unwrap_or(0)),
            ("negative_secs", negative_secs),
            ("stale_if_error_secs", stale_if_error_secs),
        ] {
            if seconds > MAX_CACHE_SECS {
                return Err(invalid(format!(
                    "{name} cannot be more than {MAX_CACHE_SECS} seconds"
                )));
            }
        }

        let metadata_paths = match raw.metadata_paths {
            Some(paths) => {
                if paths.len() > 64 {
                    return Err(invalid("metadata_paths cannot have more than 64 entries"));
                }
                for path in &paths {
                    Self::check_metadata_path(path)?;
                }
                paths
            }
            None => Self::default().metadata_paths,
        };

        Ok(Self {
            content_ttl,
            metadata_ttl,
            metadata_paths,
            negative_secs,
            stale_if_error_secs,
            offline: raw.offline,
        })
    }

    fn check_metadata_path(raw: &str) -> Result<(), UpstreamConfigError> {
        let name = raw.strip_suffix('/').unwrap_or(raw);
        let prefix = raw.ends_with('/');
        let bad = name.is_empty()
            || raw.starts_with('/')
            || raw.contains('\\')
            || raw.contains('?')
            || raw.contains('#')
            || raw.chars().any(char::is_control)
            || name.split('/').any(|segment| segment.is_empty() || segment == ".." || segment == ".")
            // Sin barra final es un nombre de segmento: no lleva barras.
            || (!prefix && name.contains('/'));
        if bad {
            return Err(invalid(
                "a metadata path is a file name, or a relative prefix ending in /",
            ));
        }
        Ok(())
    }

    /// La clase de una ruta relativa.
    pub fn kind_of(&self, asset_path: &str) -> CacheKind {
        let last = asset_path.rsplit('/').next().unwrap_or(asset_path);
        // `maven-metadata.xml.sha1` cambia cuando cambia `maven-metadata.xml`:
        // se decide por el nombre al que acompana.
        let named = SIDECAR_SUFFIXES
            .iter()
            .find_map(|suffix| last.strip_suffix(suffix))
            .filter(|base| !base.is_empty())
            .unwrap_or(last);
        let metadata = self.metadata_paths.iter().any(|entry| {
            if entry.ends_with('/') {
                asset_path.starts_with(entry.as_str())
            } else {
                last == entry || named == entry
            }
        });
        if metadata || is_floating_snapshot(asset_path) {
            CacheKind::Metadata
        } else {
            CacheKind::Content
        }
    }

    /// La clase de una ruta segun el formato del repositorio.
    ///
    /// En npm el contenido inmutable son los tarballs (`{nombre}/-/{fichero}.tgz`)
    /// y todo lo demas es un packument, que el upstream republica. El resto de
    /// formatos se rigen por las rutas de metadatos de la politica.
    pub fn kind_in(&self, format: RepositoryFormat, asset_path: &str) -> CacheKind {
        match format {
            RepositoryFormat::Npm => {
                if crate::npm::is_tarball_path(asset_path) {
                    CacheKind::Content
                } else {
                    CacheKind::Metadata
                }
            }
            RepositoryFormat::PyPi => {
                // El documento de un proyecto (`{proyecto}`) es metadata: el
                // upstream publica versiones nuevas. Las distribuciones
                // (`{proyecto}/{fichero}`) son contenido inmutable.
                if asset_path.contains('/') {
                    CacheKind::Content
                } else {
                    CacheKind::Metadata
                }
            }
            _ => self.kind_of(asset_path),
        }
    }

    /// El TTL que le toca a una clase de ruta.
    pub fn ttl_of(&self, kind: CacheKind) -> Option<u64> {
        match kind {
            CacheKind::Content => self.content_ttl,
            CacheKind::Metadata => self.metadata_ttl,
        }
    }
}

/// Un recibo que respalda el contenido que hay en el catalogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    /// El upstream que lo respaldo.
    pub upstream_id: String,
    /// La version de la configuracion con la que se respaldo.
    pub config_version: u64,
    /// Cuando se trajo o se revalido.
    pub at: DateTime<Utc>,
    /// La etiqueta de entidad del upstream.
    pub etag: Option<String>,
    /// El `Last-Modified` del upstream.
    pub last_modified: Option<String>,
    /// Con que se comprobo el contenido entonces.
    pub checksum: ChecksumEvidence,
}

/// Lo que el libro sabe del contenido que hay hoy en una ruta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    /// El digest del contenido que el catalogo sirve para la ruta.
    pub digest: ContentDigest,
    /// Los bytes de ese contenido.
    pub size_bytes: u64,
    /// El recibo mas reciente que respalda **ese** digest, si hay alguno.
    ///
    /// Sin el, el contenido esta en el catalogo pero no tiene procedencia
    /// demostrable: no se sirve offline y se revalida online.
    pub confirmation: Option<Confirmation>,
    /// La version de configuracion con la que el upstream entrego bytes que
    /// no son los esperados, si el ultimo recibo es de cuarentena.
    pub quarantined_at: Option<u64>,
}

impl CacheEntry {
    /// Lo que dicen los recibos de una ruta (el mas reciente primero) sobre
    /// el digest que el catalogo sirve hoy.
    pub fn of(
        asset_digest: &ContentDigest,
        size_bytes: u64,
        receipts: &[ProvenanceReceipt],
    ) -> Self {
        let quarantined_at = receipts
            .first()
            .filter(|receipt| receipt.outcome == ReceiptOutcome::Quarantined)
            .map(|receipt| receipt.config_version);
        let confirmation = receipts
            .iter()
            .find(|receipt| {
                receipt.outcome != ReceiptOutcome::Quarantined && receipt.digest == *asset_digest
            })
            .map(|receipt| Confirmation {
                upstream_id: receipt.upstream_id.clone(),
                config_version: receipt.config_version,
                at: receipt.fetched_at,
                etag: receipt.etag.clone(),
                last_modified: receipt.last_modified.clone(),
                checksum: receipt.checksum.clone(),
            });
        Self {
            digest: asset_digest.clone(),
            size_bytes,
            confirmation,
            quarantined_at,
        }
    }
}

/// Lo que hace falta para revalidar un contenido que ya se tiene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    /// El digest que se tiene.
    pub digest: ContentDigest,
    /// Los bytes de ese contenido.
    pub size_bytes: u64,
    /// Con que se comprobo cuando se respaldo, si hay respaldo vigente: es
    /// lo que hereda un `304`, que no trae cuerpo que comprobar.
    pub checksum: Option<ChecksumEvidence>,
    /// El validador `ETag` que dio el upstream, si lo dio y sigue valiendo.
    pub etag: Option<String>,
    /// El validador `Last-Modified`, igual.
    pub last_modified: Option<String>,
    /// Si el contenido es inmutable: otros bytes para la misma ruta son un
    /// incumplimiento del upstream y no una actualizacion.
    ///
    /// Una revalidacion que se hace porque la configuracion cambio despues
    /// de una cuarentena **no** es inmutable: quien administra cambio la
    /// configuracion sabiendo que habia contenido retenido, y ese cambio es
    /// la decision de aceptar lo que el upstream sirve ahora.
    pub immutable: bool,
}

/// Lo que se hace con una peticion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheDecision {
    /// Se sirve lo que hay, sin tocar el upstream.
    Fresh,
    /// Se pregunta al upstream si lo que hay sigue valiendo.
    Revalidate(Known),
    /// No hay nada: se trae.
    Fetch,
    /// El upstream entrego otros bytes para un contenido inmutable y esa
    /// configuracion no se ha cambiado desde entonces.
    Quarantined,
    /// La ruta ya no cae en ningun scope: el cambio de politica la oculta.
    OutOfScope,
    /// Sin red: se sirve lo que hay, que tiene procedencia.
    OfflineServe,
    /// Sin red y sin contenido servible con procedencia.
    OfflineMiss,
}

/// El contexto de una peticion.
#[derive(Debug, Clone)]
pub struct Context<'a> {
    /// El formato del repositorio.
    pub format: RepositoryFormat,
    /// La politica vigente.
    pub policy: &'a CachePolicy,
    /// La ruta relativa.
    pub asset_path: &'a str,
    /// El upstream vigente.
    pub upstream_id: &'a str,
    /// Si la ruta cae hoy en algun scope del upstream.
    pub in_scope: bool,
    /// La version vigente de la configuracion del repositorio.
    pub config_version: u64,
    /// Si no se puede salir a la red (la politica o el servicio).
    pub offline: bool,
    /// La hora de la decision.
    pub now: DateTime<Utc>,
}

/// La edad de una confirmacion, en segundos (nunca negativa).
fn age_secs(confirmation: &Confirmation, now: DateTime<Utc>) -> u64 {
    now.signed_duration_since(confirmation.at)
        .num_seconds()
        .max(0) as u64
}

/// Decide que hacer con una peticion de una ruta proxy.
pub fn decide(ctx: &Context<'_>, entry: Option<&CacheEntry>) -> CacheDecision {
    if !ctx.in_scope {
        return CacheDecision::OutOfScope;
    }
    let Some(entry) = entry else {
        return if ctx.offline {
            CacheDecision::OfflineMiss
        } else {
            CacheDecision::Fetch
        };
    };

    // Una cuarentena dura mientras no cambie la configuracion: es la
    // unica palanca de quien administra para decir "ahora si".
    let mut forced = false;
    if let Some(version) = entry.quarantined_at {
        if ctx.offline || version == ctx.config_version {
            return CacheDecision::Quarantined;
        }
        forced = true;
    }

    let current = entry
        .confirmation
        .as_ref()
        .filter(|confirmation| confirmation.upstream_id == ctx.upstream_id);

    if ctx.offline {
        return match current {
            Some(_) => CacheDecision::OfflineServe,
            None => CacheDecision::OfflineMiss,
        };
    }

    let kind = ctx.policy.kind_in(ctx.format, ctx.asset_path);
    let known = |with_validators: bool| Known {
        digest: entry.digest.clone(),
        size_bytes: entry.size_bytes,
        checksum: current
            .filter(|_| with_validators)
            .map(|c| c.checksum.clone()),
        etag: current
            .filter(|_| with_validators)
            .and_then(|c| c.etag.clone()),
        last_modified: current
            .filter(|_| with_validators)
            .and_then(|c| c.last_modified.clone()),
        immutable: kind == CacheKind::Content && !forced,
    };

    let Some(confirmation) = current else {
        return CacheDecision::Revalidate(known(false));
    };
    // Una configuracion nueva (otra URL, otro `require_checksum`...) no hereda
    // validadores ni evidencia: un `304` del origen nuevo no puede dar por
    // bueno lo que bajo el viejo, y un `200` se comprueba con la politica de
    // ahora. Cuesta una descarga por ruta y cambio de configuracion.
    if forced || confirmation.config_version != ctx.config_version {
        return CacheDecision::Revalidate(known(false));
    }
    match ctx.policy.ttl_of(kind) {
        Some(ttl) if age_secs(confirmation, ctx.now) >= ttl => {
            CacheDecision::Revalidate(known(true))
        }
        _ => CacheDecision::Fresh,
    }
}

/// Si, tras un fallo del upstream, se puede servir el contenido cacheado.
///
/// Solo si hay procedencia del upstream vigente, la politica lo permite
/// (`stale_if_error_secs > 0`) y el contenido no lleva caducado mas que esa
/// ventana. Un TTL ausente no caduca: la ventana no se agota nunca.
pub fn stale_allowed(ctx: &Context<'_>, entry: &CacheEntry) -> bool {
    if ctx.policy.stale_if_error_secs == 0 || entry.quarantined_at.is_some() || !ctx.in_scope {
        return false;
    }
    let Some(confirmation) = entry
        .confirmation
        .as_ref()
        .filter(|confirmation| confirmation.upstream_id == ctx.upstream_id)
    else {
        return false;
    };
    match ctx
        .policy
        .ttl_of(ctx.policy.kind_in(ctx.format, ctx.asset_path))
    {
        None => true,
        Some(ttl) => {
            age_secs(confirmation, ctx.now) < ttl.saturating_add(ctx.policy.stale_if_error_secs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn digest(byte: char) -> ContentDigest {
        ContentDigest::of_bytes(byte.to_string().as_bytes())
    }

    fn at(offset_secs: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000 + offset_secs, 0).expect("time")
    }

    fn policy(value: Value) -> CachePolicy {
        CachePolicy::parse(&value).expect("valid")
    }

    fn confirmation(version: u64, secs: i64) -> Confirmation {
        Confirmation {
            upstream_id: "central".into(),
            config_version: version,
            at: at(secs),
            etag: Some("\"v1\"".into()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".into()),
            checksum: ChecksumEvidence::ComputedOnly,
        }
    }

    fn entry(version: u64, secs: i64) -> CacheEntry {
        CacheEntry {
            digest: digest('a'),
            size_bytes: 7,
            confirmation: Some(confirmation(version, secs)),
            quarantined_at: None,
        }
    }

    fn ctx<'a>(policy: &'a CachePolicy, path: &'a str, now: i64) -> Context<'a> {
        Context {
            format: RepositoryFormat::Maven2,
            policy,
            asset_path: path,
            upstream_id: "central",
            in_scope: true,
            config_version: 1,
            offline: false,
            now: at(now),
        }
    }

    #[test]
    fn npm_tarballs_are_content_and_packuments_are_metadata() {
        let policy = CachePolicy::default();
        let npm = RepositoryFormat::Npm;
        assert_eq!(policy.kind_in(npm, "lodash"), CacheKind::Metadata);
        assert_eq!(policy.kind_in(npm, "@acme/lib"), CacheKind::Metadata);
        assert_eq!(
            policy.kind_in(npm, "lodash/-/lodash-4.17.21.tgz"),
            CacheKind::Content
        );
        // Los demas formatos no cambian de criterio.
        assert_eq!(
            policy.kind_in(RepositoryFormat::Maven2, "a/b/maven-metadata.xml"),
            CacheKind::Metadata
        );
        assert_eq!(
            policy.kind_in(RepositoryFormat::Raw, "lodash"),
            CacheKind::Content
        );
    }

    #[test]
    fn the_defaults_never_expire_nor_remember_nor_serve_stale() {
        let policy = CachePolicy::default();
        assert_eq!(policy.content_ttl, None);
        assert_eq!(policy.metadata_ttl, None);
        assert_eq!(policy.negative_secs, 0);
        assert_eq!(policy.stale_if_error_secs, 0);
        assert!(!policy.offline);
        assert_eq!(CachePolicy::parse(&json!({})).expect("empty"), policy);
    }

    #[test]
    fn what_cannot_be_read_is_refused() {
        let cases = [
            json!({"ttl": {"content_secs": -1}}),
            json!({"ttl": {"content_secs": MAX_CACHE_SECS + 1}}),
            json!({"negative_secs": MAX_CACHE_SECS + 1}),
            json!({"stale_if_error_secs": "soon"}),
            json!({"ttl": {"metadata": 5}}),
            json!({"unknown": true}),
            json!({"metadata_paths": ["/abs/"]}),
            json!({"metadata_paths": ["a/b"]}),
            json!({"metadata_paths": ["../x/"]}),
            json!({"metadata_paths": [""]}),
            json!({"metadata_paths": ["/"]}),
            json!({"metadata_paths": ["a//b/"]}),
        ];
        for case in cases {
            assert!(
                CachePolicy::parse(&case).is_err(),
                "{case} should be refused"
            );
        }
    }

    #[test]
    fn a_path_is_metadata_by_name_or_by_prefix_and_otherwise_content() {
        let policy = policy(json!({"metadata_paths": ["maven-metadata.xml", "dists/"]}));
        assert_eq!(
            policy.kind_of("acme/lib/maven-metadata.xml"),
            CacheKind::Metadata
        );
        assert_eq!(policy.kind_of("maven-metadata.xml"), CacheKind::Metadata);
        assert_eq!(policy.kind_of("dists/stable/Release"), CacheKind::Metadata);
        assert_eq!(
            policy.kind_of("acme/lib/1.0/lib-1.0.jar"),
            CacheKind::Content
        );
        // El nombre es de segmento entero: no una terminacion.
        assert_eq!(policy.kind_of("x-maven-metadata.xml"), CacheKind::Content);
        // El prefijo es de segmento: `dists-x/` no es `dists/`.
        assert_eq!(policy.kind_of("dists-x/a"), CacheKind::Content);
    }

    #[test]
    fn the_checksum_and_signature_of_a_metadata_follow_its_kind() {
        let policy = policy(json!({}));
        for path in [
            "acme/lib/maven-metadata.xml.sha1",
            "acme/lib/maven-metadata.xml.asc",
            "dists/stable/Release.gpg",
            "dists/stable/InRelease",
            "dists/stable/main/Packages.xz",
        ] {
            assert_eq!(policy.kind_of(path), CacheKind::Metadata, "{path}");
        }
        // El checksum de un artefacto es tan inmutable como el artefacto.
        assert_eq!(
            policy.kind_of("acme/lib/1.0/lib-1.0.jar.sha1"),
            CacheKind::Content
        );
        assert_eq!(policy.kind_of(".sha1"), CacheKind::Content);
    }

    #[test]
    fn without_an_entry_it_fetches_online_and_misses_offline() {
        let policy = CachePolicy::default();
        assert_eq!(decide(&ctx(&policy, "a/b", 0), None), CacheDecision::Fetch);
        let mut offline = ctx(&policy, "a/b", 0);
        offline.offline = true;
        assert_eq!(decide(&offline, None), CacheDecision::OfflineMiss);
    }

    #[test]
    fn a_path_outside_the_scopes_is_hidden_even_if_it_is_cached() {
        let policy = CachePolicy::default();
        let mut out = ctx(&policy, "a/b", 0);
        out.in_scope = false;
        assert_eq!(decide(&out, Some(&entry(1, 0))), CacheDecision::OutOfScope);
        assert_eq!(decide(&out, None), CacheDecision::OutOfScope);
    }

    #[test]
    fn a_fresh_entry_is_served_and_an_expired_one_is_revalidated_with_validators() {
        let policy = policy(json!({"ttl": {"content_secs": 100, "metadata_secs": 10}}));
        let entry = entry(1, 0);

        assert_eq!(
            decide(&ctx(&policy, "a/lib.jar", 99), Some(&entry)),
            CacheDecision::Fresh
        );
        let CacheDecision::Revalidate(known) =
            decide(&ctx(&policy, "a/lib.jar", 100), Some(&entry))
        else {
            panic!("expired content should be revalidated");
        };
        assert_eq!(known.digest, digest('a'));
        assert_eq!(known.etag.as_deref(), Some("\"v1\""));
        assert!(known.last_modified.is_some());
        assert!(known.immutable);

        // La metadata caduca antes y no es inmutable.
        assert_eq!(
            decide(&ctx(&policy, "a/maven-metadata.xml", 9), Some(&entry)),
            CacheDecision::Fresh
        );
        let CacheDecision::Revalidate(known) =
            decide(&ctx(&policy, "a/maven-metadata.xml", 10), Some(&entry))
        else {
            panic!("expired metadata should be revalidated");
        };
        assert!(!known.immutable);
    }

    #[test]
    fn a_zero_ttl_revalidates_every_time_and_no_ttl_never() {
        let always = policy(json!({"ttl": {"content_secs": 0}}));
        assert!(matches!(
            decide(&ctx(&always, "a/b", 0), Some(&entry(1, 0))),
            CacheDecision::Revalidate(_)
        ));
        let never = CachePolicy::default();
        assert_eq!(
            decide(&ctx(&never, "a/b", 10_000_000), Some(&entry(1, 0))),
            CacheDecision::Fresh
        );
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_make_an_entry_expire() {
        let policy = policy(json!({"ttl": {"content_secs": 10}}));
        assert_eq!(
            decide(&ctx(&policy, "a/b", -500), Some(&entry(1, 0))),
            CacheDecision::Fresh
        );
    }

    #[test]
    fn a_new_config_version_revalidates_even_if_fresh() {
        let policy = CachePolicy::default();
        let mut now = ctx(&policy, "a/b", 1);
        now.config_version = 2;
        let CacheDecision::Revalidate(known) = decide(&now, Some(&entry(1, 0))) else {
            panic!("a new configuration should be revalidated");
        };
        // Y sin heredar validadores: otra configuracion puede apuntar a otro
        // origen o exigir otra evidencia.
        assert_eq!(known.etag, None);
    }

    #[test]
    fn content_without_a_receipt_of_the_current_upstream_is_revalidated_without_validators() {
        let policy = CachePolicy::default();
        let no_receipt = CacheEntry {
            digest: digest('a'),
            size_bytes: 7,
            confirmation: None,
            quarantined_at: None,
        };
        let CacheDecision::Revalidate(known) = decide(&ctx(&policy, "a/b", 0), Some(&no_receipt))
        else {
            panic!("no provenance means revalidate");
        };
        assert!(known.etag.is_none() && known.last_modified.is_none());

        let mut other = entry(1, 0);
        other
            .confirmation
            .as_mut()
            .expect("confirmation")
            .upstream_id = "elsewhere".into();
        let CacheDecision::Revalidate(known) = decide(&ctx(&policy, "a/b", 0), Some(&other)) else {
            panic!("another upstream means revalidate");
        };
        // Los validadores de otro upstream no se le ensenan a este.
        assert!(known.etag.is_none());
    }

    #[test]
    fn offline_serves_only_what_has_provenance_from_the_current_upstream() {
        let policy = policy(json!({"ttl": {"content_secs": 1}}));
        let mut offline = ctx(&policy, "a/b", 1_000_000);
        offline.offline = true;
        offline.config_version = 9;

        // El TTL y la deriva de configuracion no importan sin red.
        assert_eq!(
            decide(&offline, Some(&entry(1, 0))),
            CacheDecision::OfflineServe
        );

        let no_receipt = CacheEntry {
            digest: digest('a'),
            size_bytes: 7,
            confirmation: None,
            quarantined_at: None,
        };
        assert_eq!(
            decide(&offline, Some(&no_receipt)),
            CacheDecision::OfflineMiss
        );

        let mut other = entry(1, 0);
        other
            .confirmation
            .as_mut()
            .expect("confirmation")
            .upstream_id = "elsewhere".into();
        assert_eq!(decide(&offline, Some(&other)), CacheDecision::OfflineMiss);
    }

    /// Un recibo de importacion confirma el contenido igual que un fetch:
    /// lo que autoriza a servirlo sin red es la configuracion vigente del
    /// repositorio, no como llegaron los bytes.
    #[test]
    fn an_imported_receipt_of_the_current_upstream_is_served_offline() {
        let policy = policy(json!({"ttl": {"content_secs": 1}}));
        let mut offline = ctx(&policy, "a/b", 1_000_000);
        offline.offline = true;
        offline.config_version = 4;

        let served = digest('a');
        let mut receipt = ProvenanceReceipt {
            id: "prv-import".into(),
            account: "acme".into(),
            repository: "central".into(),
            asset_path: "a/b".into(),
            upstream_id: "central".into(),
            source_url: "https://repo.example.com/a/b".into(),
            config_version: 4,
            digest: served.clone(),
            size_bytes: 1,
            checksum: ChecksumEvidence::ComputedOnly,
            status: 200,
            redirects: 0,
            etag: None,
            last_modified: None,
            outcome: ReceiptOutcome::Imported,
            actor: "u".into(),
            correlation_id: "c".into(),
            fetched_at: at(0),
        };
        let entry = CacheEntry::of(&served, 1, std::slice::from_ref(&receipt));
        assert_eq!(
            decide(&offline, Some(&entry)),
            CacheDecision::OfflineServe
        );

        // Lo que no es el upstream vigente tampoco se sirve sin red, y un
        // importado no abre excepciones.
        receipt.upstream_id = "elsewhere".into();
        let entry = CacheEntry::of(&served, 1, std::slice::from_ref(&receipt));
        assert_eq!(decide(&offline, Some(&entry)), CacheDecision::OfflineMiss);
    }

    #[test]
    fn a_quarantine_lasts_until_the_configuration_changes() {
        let policy = CachePolicy::default();
        let mut quarantined = entry(1, 0);
        quarantined.quarantined_at = Some(1);

        assert_eq!(
            decide(&ctx(&policy, "a/b", 5), Some(&quarantined)),
            CacheDecision::Quarantined
        );
        // Con una configuracion nueva se vuelve a preguntar al upstream, y
        // lo que este sirva ahora ya no se juzga inmutable.
        let mut changed = ctx(&policy, "a/b", 5);
        changed.config_version = 2;
        let CacheDecision::Revalidate(known) = decide(&changed, Some(&quarantined)) else {
            panic!("a new configuration should reopen the question");
        };
        assert!(!known.immutable);
        // Sin red nunca se levanta: seria servir lo que se rechazo.
        changed.offline = true;
        assert_eq!(
            decide(&changed, Some(&quarantined)),
            CacheDecision::Quarantined
        );
    }

    #[test]
    fn stale_is_served_only_inside_the_window_and_with_provenance() {
        let policy = policy(json!({
            "ttl": {"content_secs": 100},
            "stale_if_error_secs": 50,
        }));
        let entry = entry(1, 0);
        assert!(stale_allowed(&ctx(&policy, "a/b", 149), &entry));
        assert!(!stale_allowed(&ctx(&policy, "a/b", 150), &entry));

        // Por defecto nunca.
        let strict = CachePolicy::default();
        assert!(!stale_allowed(&ctx(&strict, "a/b", 0), &entry));

        // Sin procedencia del upstream vigente, tampoco.
        let mut other = entry.clone();
        other
            .confirmation
            .as_mut()
            .expect("confirmation")
            .upstream_id = "elsewhere".into();
        assert!(!stale_allowed(&ctx(&policy, "a/b", 0), &other));

        // Ni lo puesto en cuarentena.
        let mut quarantined = entry.clone();
        quarantined.quarantined_at = Some(1);
        assert!(!stale_allowed(&ctx(&policy, "a/b", 0), &quarantined));
    }

    #[test]
    fn the_entry_is_read_from_the_receipts_of_the_digest_the_catalog_serves() {
        use crate::proxy::ChecksumEvidence;
        let receipt = |digest: ContentDigest, outcome, secs: i64, version| ProvenanceReceipt {
            id: format!("prv-{secs}"),
            account: "acme".into(),
            repository: "central".into(),
            asset_path: "a/b".into(),
            upstream_id: "central".into(),
            source_url: "https://repo.example.com/a/b".into(),
            config_version: version,
            digest,
            size_bytes: 1,
            checksum: ChecksumEvidence::ComputedOnly,
            status: 200,
            redirects: 0,
            etag: Some(format!("\"e{secs}\"")),
            last_modified: None,
            outcome,
            actor: "u".into(),
            correlation_id: "c".into(),
            fetched_at: at(secs),
        };
        let served = digest('a');
        let receipts = vec![
            receipt(digest('b'), ReceiptOutcome::Quarantined, 30, 2),
            receipt(served.clone(), ReceiptOutcome::Revalidated, 20, 1),
            receipt(served.clone(), ReceiptOutcome::Fetched, 10, 1),
        ];
        let entry = CacheEntry::of(&served, 1, &receipts);
        assert_eq!(entry.quarantined_at, Some(2));
        let confirmation = entry.confirmation.expect("confirmed");
        // El recibo de cuarentena no confirma nada; el mas reciente que si, manda.
        assert_eq!(confirmation.at, at(20));
        assert_eq!(confirmation.etag.as_deref(), Some("\"e20\""));

        // Un recibo de otro digest no respalda el que se sirve.
        let strangers = vec![receipt(digest('c'), ReceiptOutcome::Fetched, 1, 1)];
        assert!(
            CacheEntry::of(&served, 1, &strangers)
                .confirmation
                .is_none()
        );
        assert!(CacheEntry::of(&served, 1, &[]).confirmation.is_none());

        // Una cuarentena que ya no es la ultima no cuenta.
        let healed = vec![
            receipt(served.clone(), ReceiptOutcome::Revalidated, 40, 3),
            receipt(digest('b'), ReceiptOutcome::Quarantined, 30, 2),
        ];
        assert_eq!(CacheEntry::of(&served, 1, &healed).quarantined_at, None);
    }

    #[test]
    fn a_floating_snapshot_is_mutable_and_a_timestamped_one_is_not() {
        let policy = CachePolicy::default();
        for path in [
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-SNAPSHOT.jar",
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-SNAPSHOT.pom",
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-SNAPSHOT-sources.jar",
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-SNAPSHOT.jar.sha1",
        ] {
            assert_eq!(policy.kind_of(path), CacheKind::Metadata, "{path}");
        }
        for path in [
            "org/acme/widget/1.0-SNAPSHOT/widget-1.0-20240101.120000-1.jar",
            "org/acme/widget/1.0/widget-1.0.jar",
            "widget-1.0-SNAPSHOT.jar",
            "org/acme/widget/1.0-SNAPSHOT/readme.txt",
            // El nombre de la version dentro de otro nombre no es flotante.
            "org/acme/lib/1.0-SNAPSHOT/lib-11.0-SNAPSHOT.jar",
            "org/acme/lib/1.0-SNAPSHOT/other-1.0-SNAPSHOT.jar",
            "org/acme/lib/1.0-SNAPSHOT/lib-1.0-SNAPSHOTS.jar",
        ] {
            assert_eq!(policy.kind_of(path), CacheKind::Content, "{path}");
        }
    }
}
