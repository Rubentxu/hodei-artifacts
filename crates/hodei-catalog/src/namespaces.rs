//! La reserva de namespaces: que nombres son de la casa y nunca salen de un
//! upstream publico.
//!
//! El ataque de *dependency confusion* es simple: alguien publica en el
//! registro publico un nombre que la organizacion usa en privado, con una
//! version mayor, y un resolvedor que prefiere lo nuevo trae el paquete del
//! atacante. La defensa es la **reserva**: un namespace declarado reservado
//! se sirve desde los repositorios internos o no se sirve, y el upstream
//! publico no participa aunque tenga mas versiones.
//!
//! Este modulo es puro: lee la politica de la configuracion del repositorio
//! y juzga una ruta. Quien resuelve (el proxy y los grupos) pregunta aqui
//! **siempre**, antes de mirar cache, recibos o red: la consulta es
//! obligatoria en la resolucion, y una ruta reservada no puede esquivarla
//! ni con un TTL positivo, ni con uno negativo, ni sin red.
//!
//! # La politica
//!
//! Vive en `config_settings.namespaces`:
//!
//! ```json
//! { "namespaces": {
//!     "reserved": ["com.acme", "@acme", "acme-internal"],
//!     "on_conflict": "deny"
//! } }
//! ```
//!
//! - `reserved` son los namespaces reservados, en la escritura natural de
//!   cada formato: groupId con puntos en Maven 2 (`com.acme`, que reserva
//!   tambien sus subgrupos), `@scope` o nombre en npm, nombre de proyecto
//!   en PyPI (que se compara normalizado, como dice PEP 503).
//! - `on_conflict` dice que hacer cuando las fuentes discrepan sobre un
//!   nombre reservado. El unico valor es `deny`: la negativa se **explica**
//!   (nombra la reserva y las fuentes en conflicto) y no hay fallback: un
//!   upstream caido nunca convierte la negativa en una respuesta publica.
//! - Una clave que no se conoce, un namespace vacio o una politica que no
//!   se entiende son un error: un ajuste de seguridad ilegible no degrada
//!   a "sin reserva".
//!
//! # Como se juzga una ruta
//!
//! [`NamespacePolicy::judge`] recibe la ruta del asset y el formato, y
//! dice si cae en una reserva. El juicio es determinista: las mismas
//! entradas dan el mismo veredicto, y el veredicto nombra la regla que
//! aplico (que namespace reservado coincidio y, en PyPI, con que alias se
//! escribio).

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

use crate::model::{RepositoryConfig, RepositoryFormat};
use crate::pypi;

/// El tope de namespaces que reserva una politica.
pub const MAX_RESERVED: usize = 256;

/// El tope de longitud de un namespace reservado.
const MAX_NAME_LEN: usize = 214;

/// Por que una politica de namespaces no es valida.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the namespace policy is not valid: {0}")]
pub struct NamespacePolicyError(pub String);

fn invalid(detail: impl Into<String>) -> NamespacePolicyError {
    NamespacePolicyError(detail.into())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    reserved: Vec<String>,
    #[serde(default)]
    on_conflict: Option<String>,
}

/// Que hacer cuando las fuentes discrepan sobre un nombre reservado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnConflict {
    /// La negativa se explica y no hay fallback: la reserva gana siempre.
    #[default]
    Deny,
}

impl OnConflict {
    /// El nombre estable con que se escribe y se lee.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "deny",
        }
    }
}

/// El veredicto sobre una ruta.
///
/// Nombra la regla que aplico: que reserva coincidio y, si la ruta llego
/// escrita con un alias que normaliza igual (PyPI), como se escribio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// La ruta no cae en ninguna reserva.
    Unreserved,
    /// La ruta cae en la reserva del namespace `matched`.
    Reserved {
        /// El namespace reservado que coincidio, tal y como se declaro.
        matched: String,
        /// La escritura de la ruta que normaliza igual, si no fue literal.
        ///
        /// En PyPI, `Flask_Login` y `flask.login` son el mismo proyecto que
        /// `flask-login`: si la politica reserva `flask-login` y la peticion
        /// llega como `Flask_Login`, el alias es el que se nombra en la
        /// explicacion.
        alias_of: Option<String>,
    },
}

impl Judgement {
    /// Si la ruta cae en una reserva.
    pub fn is_reserved(&self) -> bool {
        matches!(self, Self::Reserved { .. })
    }
}

/// La politica de namespaces de un repositorio, ya validada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespacePolicy {
    reserved: Vec<String>,
    on_conflict: OnConflict,
}

impl NamespacePolicy {
    /// La politica que declara la configuracion de un repositorio.
    ///
    /// `Ok(None)` es "no declara ninguna": sin la clave `namespaces` no hay
    /// reserva y la resolucion es la de siempre. Una declaracion que existe
    /// y no se entiende es un error, nunca una politica vacia.
    pub fn of(config: &RepositoryConfig) -> Result<Option<Self>, NamespacePolicyError> {
        let Some(policy) = config.config_settings.get("namespaces") else {
            return Ok(None);
        };
        Self::parse(policy).map(Some)
    }

    /// Valida una politica escrita como JSON.
    pub fn parse(value: &Value) -> Result<Self, NamespacePolicyError> {
        let raw: RawPolicy = serde_json::from_value(value.clone())
            .map_err(|error| invalid(format!("it cannot be read: {error}")))?;
        if raw.reserved.is_empty() {
            return Err(invalid("a policy reserves at least one namespace"));
        }
        if raw.reserved.len() > MAX_RESERVED {
            return Err(invalid(format!(
                "a policy reserves at most {MAX_RESERVED} namespaces"
            )));
        }
        let mut seen = HashSet::new();
        for name in &raw.reserved {
            let well_formed = !name.is_empty()
                && name.len() <= MAX_NAME_LEN
                && !name.chars().any(|c| c.is_whitespace() || c.is_control())
                && !name.contains('/');
            if !well_formed {
                return Err(invalid(format!("{name:?} is not a namespace")));
            }
            if !seen.insert(name.as_str()) {
                return Err(invalid(format!("the namespace {name:?} is reserved twice")));
            }
        }
        let on_conflict = match raw.on_conflict.as_deref() {
            None => OnConflict::default(),
            Some(value) if value == OnConflict::Deny.as_str() => OnConflict::Deny,
            Some(other) => {
                return Err(invalid(format!(
                    "on_conflict is {:?}, and the only conflict the policy knows is {:?}",
                    other,
                    OnConflict::Deny.as_str()
                )));
            }
        };
        Ok(Self {
            reserved: raw.reserved,
            on_conflict,
        })
    }

    /// Los namespaces reservados, tal y como se declararon.
    pub fn reserved(&self) -> &[String] {
        &self.reserved
    }

    /// Que hacer ante un conflicto de fuentes.
    pub fn on_conflict(&self) -> OnConflict {
        self.on_conflict
    }

    /// El veredicto sobre una ruta del repositorio, segun su formato.
    ///
    /// La ruta es la del asset (relativa al repositorio). El juicio no mira
    /// cache ni procedencia: es la pregunta que la resolucion se hace
    /// siempre, antes de decidir de donde sale el contenido.
    pub fn judge(&self, format: RepositoryFormat, path: &str) -> Judgement {
        match format {
            RepositoryFormat::Maven2 => self.judge_maven(path),
            RepositoryFormat::Npm => self.judge_npm(path),
            RepositoryFormat::PyPi => self.judge_pypi(path),
            // OCI no tiene una nocion de namespace con alias: el namespace es
            // el primer segmento del **nombre del repositorio**, no la ruta de
            // un asset dentro de el. Un repositorio se llama "team/demo" y su
            // namespace es "team"; el juicio se hace sobre el nombre, y por eso
            // el mismo nombre "manifests/sha256:abc" --que es una ruta de asset
            // y no un nombre-- no casa con ninguna reserva.
            //
            // Y la reserva no puede llevar barras: un namespace es el primer
            // segmento, y "team/demo" no es un namespace declarable. Por eso la
            // regla es la plana, que ya compara el primer segmento por
            // prefijo de segmento: "team" cubre "team/demo" y "team/demo/x",
            // y no cubre "teamx" ni "teammate".
            RepositoryFormat::Oci => self.judge_flat(path),
            // Los demas formatos no tienen una nocion de namespace con
            // alias: el primer segmento de la ruta es el nombre, y se
            // compara tal cual.
            _ => self.judge_flat(path),
        }
    }

    /// Maven 2: un groupId con puntos cubre su prefijo de directorios y sus
    /// subgrupos.
    ///
    /// La ruta Maven es el groupId con `/` en vez de `.`; `com.acme` reserva
    /// `com/acme/...`, y tambien `com/acme/plugin/...` (el subgrupo
    /// `com.acme.plugin`). La comparacion es por segmentos: `com/acme-x/...`
    /// no cae en la reserva de `com.acme`.
    fn judge_maven(&self, path: &str) -> Judgement {
        for reserved in &self.reserved {
            let prefix = reserved.replace('.', "/");
            if path
                .strip_prefix(prefix.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
            {
                return Judgement::Reserved {
                    matched: reserved.clone(),
                    alias_of: None,
                };
            }
        }
        Judgement::Unreserved
    }

    /// npm: un `@scope` reserva todo su alcance; un nombre reserva el
    /// paquete.
    ///
    /// La ruta de un paquete con alcance empieza por dos segmentos
    /// (`@scope/pkg/...`); la de uno sin alcance, por uno. El nombre se
    /// compara en minusculas, como npm escribe los suyos.
    fn judge_npm(&self, path: &str) -> Judgement {
        let name = package_of_npm_path(path);
        if name.is_empty() {
            return Judgement::Unreserved;
        }
        let name = name.to_ascii_lowercase();
        for reserved in &self.reserved {
            let candidate = reserved.to_ascii_lowercase();
            if name == candidate
                || (candidate.starts_with('@')
                    && name
                        .strip_prefix(candidate.as_str())
                        .is_some_and(|rest| rest.starts_with('/')))
            {
                return Judgement::Reserved {
                    matched: reserved.clone(),
                    alias_of: None,
                };
            }
        }
        Judgement::Unreserved
    }

    /// PyPI: el nombre del proyecto es el primer segmento, y se compara
    /// normalizado como dice PEP 503.
    ///
    /// `Flask_Login`, `flask.login` y `flask-login` son el mismo proyecto:
    /// una reserva de `flask-login` los gobierna a los tres, y el alias con
    /// que llego la peticion queda nombrado en el veredicto.
    fn judge_pypi(&self, path: &str) -> Judgement {
        let project = path.split('/').next().unwrap_or_default();
        if project.is_empty() {
            return Judgement::Unreserved;
        }
        let normalized = pypi::normalize(project);
        for reserved in &self.reserved {
            if normalized == pypi::normalize(reserved) {
                let alias_of = (project != *reserved).then(|| project.to_string());
                return Judgement::Reserved {
                    matched: reserved.clone(),
                    alias_of,
                };
            }
        }
        Judgement::Unreserved
    }

    /// The formats without a namespace of their own: the first segment of the
    /// path is the name, and it is compared literally.
    fn judge_flat(&self, path: &str) -> Judgement {
        let name = path.split('/').next().unwrap_or_default();
        if name.is_empty() {
            return Judgement::Unreserved;
        }
        for reserved in &self.reserved {
            if name == reserved {
                return Judgement::Reserved {
                    matched: reserved.clone(),
                    alias_of: None,
                };
            }
        }
        Judgement::Unreserved
    }
}

/// El nombre de paquete npm que encabeza una ruta.
///
/// `@scope/pkg/-/pkg-1.0.0.tgz` es `@scope/pkg`; `pkg/...` es `pkg`; lo que
/// no llega a un nombre (`@solo`, la ruta vacia) no es de ningun paquete.
fn package_of_npm_path(path: &str) -> &str {
    if let Some(rest) = path.strip_prefix('@') {
        let (scope, after) = match rest.split_once('/') {
            Some(parts) => parts,
            None => return "",
        };
        let (base, _) = after.split_once('/').unwrap_or((after, ""));
        if base.is_empty() {
            return "";
        }
        // El nombre mide lo que hay desde el principio hasta el final del
        // segundo segmento.
        let length = 1 + scope.len() + 1 + base.len();
        return &path[..length];
    }
    path.split('/').next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy(settings: Value) -> Result<Option<NamespacePolicy>, NamespacePolicyError> {
        let mut config = RepositoryConfig::new(crate::DigestScope::Global);
        config.config_settings = settings;
        NamespacePolicy::of(&config)
    }

    fn reserved(names: &[&str]) -> NamespacePolicy {
        NamespacePolicy::parse(&json!({ "reserved": names })).expect("a valid policy")
    }

    #[test]
    fn a_repository_without_namespace_settings_has_no_policy() {
        assert_eq!(policy(json!({})).unwrap(), None);
        let mut config = RepositoryConfig::new(crate::DigestScope::Global);
        config.config_settings = json!({ "proxy": { "upstream": { "id": "u" } } });
        assert_eq!(NamespacePolicy::of(&config).unwrap(), None);
    }

    #[test]
    fn a_policy_declares_its_namespaces_and_the_conflict_it_knows() {
        let policy = reserved(&["com.acme", "@acme", "acme-internal"]);
        assert_eq!(policy.reserved(), ["com.acme", "@acme", "acme-internal"]);
        assert_eq!(policy.on_conflict(), OnConflict::Deny);

        let explicit = NamespacePolicy::parse(&json!({
            "reserved": ["a"],
            "on_conflict": "deny",
        }))
        .expect("the declared conflict is the one it knows");
        assert_eq!(explicit.on_conflict(), OnConflict::Deny);
    }

    #[test]
    fn what_cannot_be_read_is_refused_instead_of_defaulted() {
        for bad in [
            json!({}),
            json!({ "reserved": [] }),
            json!({ "reserved": ["a"], "on_conflict": "prefer_internal" }),
            json!({ "reserved": ["a"], "unknown": 1 }),
            json!({ "reserved": [""] }),
            json!({ "reserved": ["a", "a"] }),
            json!({ "reserved": ["with space"] }),
            json!({ "reserved": ["with/slash"] }),
            json!(["a"]),
            json!("reserved"),
        ] {
            assert!(NamespacePolicy::parse(&bad).is_err(), "{bad}");
        }
        let too_many: Vec<String> = (0..=MAX_RESERVED).map(|n| format!("n{n}")).collect();
        assert!(NamespacePolicy::parse(&json!({ "reserved": too_many })).is_err());
    }

    #[test]
    fn a_settings_key_nobody_can_read_breaks_the_policy() {
        assert!(policy(json!({ "namespaces": "strict" })).is_err());
        assert!(policy(json!({ "namespaces": { "reserve": ["a"] } })).is_err());
    }

    #[test]
    fn a_maven_group_id_covers_its_directories_and_its_subgroups() {
        let policy = reserved(&["com.acme"]);
        for path in [
            "com/acme/lib/1.0/lib-1.0.pom",
            "com/acme/lib/1.0/lib-1.0.pom.sha1",
            "com/acme/lib/maven-metadata.xml",
            "com/acme/plugin/2.0/plugin-2.0.jar",
        ] {
            assert_eq!(
                policy.judge(RepositoryFormat::Maven2, path),
                Judgement::Reserved {
                    matched: "com.acme".into(),
                    alias_of: None,
                },
                "{path}"
            );
        }
        for free in [
            "org/other/lib/1.0/lib-1.0.pom",
            "com/acmevil/lib/1.0/lib-1.0.pom",
            "com/acme-x/lib/1.0/lib-1.0.pom",
            "lib.bin",
        ] {
            assert_eq!(
                policy.judge(RepositoryFormat::Maven2, free),
                Judgement::Unreserved,
                "{free}"
            );
        }
        // La raiz del grupo reservado tambien lo esta: todo lo que cae bajo
        // `acme/` es del namespace `acme`.
        assert_eq!(
            reserved(&["acme"]).judge(RepositoryFormat::Maven2, "acme/"),
            Judgement::Reserved {
                matched: "acme".into(),
                alias_of: None,
            }
        );
    }

    #[test]
    fn an_npm_scope_governs_everything_under_it_and_a_name_governs_itself() {
        let policy = reserved(&["@acme", "left-pad"]);
        for path in [
            "@acme/pkg",
            "@acme/pkg/-/pkg-1.0.0.tgz",
            "@ACME/other/-/other-1.0.0.tgz",
            "left-pad",
            "left-pad/-/left-pad-1.0.0.tgz",
        ] {
            assert!(
                policy.judge(RepositoryFormat::Npm, path).is_reserved(),
                "{path}"
            );
        }
        for free in ["@acmevillain/pkg", "left-padx", "other-pkg"] {
            assert_eq!(
                policy.judge(RepositoryFormat::Npm, free),
                Judgement::Unreserved,
                "{free}"
            );
        }
        // Un prefijo de alcance incompleto no es un nombre de paquete.
        assert_eq!(
            policy.judge(RepositoryFormat::Npm, "@acme"),
            Judgement::Unreserved
        );
    }

    #[test]
    fn a_pypi_reservation_is_matched_the_way_pep_503_normalizes() {
        let policy = reserved(&["acme-utils"]);
        for path in [
            "acme-utils",
            "acme-utils/acme_utils-1.0-py3-none-any.whl",
            "Acme_Utils",
            "acme.utils/acme.utils-2.0.tar.gz",
            "ACME-UTILS",
        ] {
            let judgement = policy.judge(RepositoryFormat::PyPi, path);
            assert!(judgement.is_reserved(), "{path}");
            if let Judgement::Reserved { alias_of, .. } = judgement {
                let alias = path.split('/').next().unwrap_or_default();
                assert_eq!(alias_of, (alias != "acme-utils").then(|| alias.to_string()));
            }
        }
        assert_eq!(
            policy.judge(RepositoryFormat::PyPi, "acme-utils-extras"),
            Judgement::Unreserved
        );
        assert_eq!(
            policy.judge(RepositoryFormat::PyPi, "other/acme-utils-1.0.whl"),
            Judgement::Unreserved
        );
    }

    #[test]
    fn a_format_without_namespaces_judges_the_first_segment_literally() {
        let policy = reserved(&["lab"]);
        assert!(
            policy
                .judge(RepositoryFormat::Raw, "lab/file.bin")
                .is_reserved()
        );
        assert_eq!(
            policy.judge(RepositoryFormat::Raw, "laboratory/file.bin"),
            Judgement::Unreserved
        );
    }

    #[test]
    fn an_oci_reservation_is_matched_over_the_name_of_the_repository() {
        // Un namespace reservado no puede llevar barras: es el primer
        // segmento, y "team/demo" no es un namespace declarable. La reserva
        // que protege al grupo se escribe "team", y cubre "team/demo".
        let policy = reserved(&["team"]);
        for name in ["team", "team/demo", "team/demo/extra"] {
            assert!(
                policy.judge(RepositoryFormat::Oci, name).is_reserved(),
                "{name}"
            );
        }
        for free in ["teamx", "teammate", "other/demo"] {
            assert_eq!(
                policy.judge(RepositoryFormat::Oci, free),
                Judgement::Unreserved,
                "{free}"
            );
        }
        // The judgement is made over the name of the repository, and not over
        // the path of an asset inside it.
        assert_eq!(
            policy.judge(RepositoryFormat::Oci, "manifests/sha256:abc"),
            Judgement::Unreserved
        );
    }

    #[test]
    fn the_verdict_names_its_rule_and_the_alias_it_saw() {
        let policy = reserved(&["com.acme"]);
        let judgement = policy.judge(RepositoryFormat::Maven2, "com/acme/lib/1.0/lib-1.0.pom");
        assert_eq!(
            judgement,
            Judgement::Reserved {
                matched: "com.acme".into(),
                alias_of: None,
            }
        );
    }
}
