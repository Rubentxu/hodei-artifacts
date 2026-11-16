//! Un repositorio de grupo: varios repositorios del mismo formato detras de
//! un unico endpoint, con una precedencia explicita.
//!
//! Un grupo no guarda bytes ni los replica: es una **lista ordenada** de
//! repositorios (hosted, proxy u otros grupos) y una regla de lectura. Este
//! modulo es puro: valida la configuracion y recorre el grafo de miembros;
//! quien lee el catalogo y decide los permisos es el servicio.
//!
//! # La configuracion
//!
//! Vive en `config_settings.group`:
//!
//! ```json
//! { "group": { "members": ["internal", "central-mirror"] } }
//! ```
//!
//! - El **orden** es la precedencia: gana el primer miembro que tiene la
//!   ruta. No hay votacion ni "el mas reciente": dos lecturas de la misma
//!   ruta con la misma configuracion dan la misma fuente.
//! - Todos los miembros son del mismo formato que el grupo y de la misma
//!   cuenta. Un grupo puede contener otros grupos (hasta [`MAX_DEPTH`]
//!   niveles); un ciclo, un miembro que no existe o de otro formato impiden
//!   guardar la configuracion.
//! - Una clave que no se conoce es un error, no un ajuste ignorado.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

use crate::{RepositoryConfig, RepositoryFormat, RepositoryMode};

/// Los miembros directos que admite un grupo.
pub const MAX_MEMBERS: usize = 16;

/// Los niveles de grupos anidados que se siguen, contando el grupo raiz
/// como nivel 0.
pub const MAX_DEPTH: usize = 3;

/// Los repositorios distintos que un recorrido puede visitar. Acota las
/// lecturas del catalogo de una sola peticion.
pub const MAX_NODES: usize = 64;

const MAX_KEY_LEN: usize = 128;

/// Por que una configuracion de grupo no es valida.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid group configuration: {0}")]
pub struct GroupConfigError(pub String);

fn invalid(detail: impl Into<String>) -> GroupConfigError {
    GroupConfigError(detail.into())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGroup {
    members: Vec<String>,
}

/// La configuracion de un grupo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupConfig {
    /// Las claves de los miembros, en orden de precedencia.
    pub members: Vec<String>,
}

impl GroupConfig {
    /// El grupo que declara la configuracion de un repositorio.
    ///
    /// `Ok(None)` es "no declara ninguno"; una declaracion que existe y no
    /// se entiende es un error, nunca una lista vacia.
    pub fn of(config: &RepositoryConfig) -> Result<Option<Self>, GroupConfigError> {
        let Some(group) = config.config_settings.get("group") else {
            return Ok(None);
        };
        Self::parse(group).map(Some)
    }

    /// Valida un grupo escrito como JSON.
    pub fn parse(value: &Value) -> Result<Self, GroupConfigError> {
        let raw: RawGroup = serde_json::from_value(value.clone())
            .map_err(|error| invalid(format!("it cannot be read: {error}")))?;
        if raw.members.is_empty() {
            return Err(invalid("a group needs at least one member"));
        }
        if raw.members.len() > MAX_MEMBERS {
            return Err(invalid(format!(
                "a group has at most {MAX_MEMBERS} direct members"
            )));
        }
        let mut seen = HashSet::new();
        for member in &raw.members {
            let key_ok = !member.is_empty()
                && member.len() <= MAX_KEY_LEN
                && !member
                    .chars()
                    .any(|char| char == '/' || char.is_whitespace() || char.is_control());
            if !key_ok {
                return Err(invalid(format!("{member:?} is not a repository key")));
            }
            if !seen.insert(member.as_str()) {
                return Err(invalid(format!("the member {member:?} is listed twice")));
            }
        }
        Ok(Self {
            members: raw.members,
        })
    }
}

/// Lo que se sabe de un repositorio al recorrer un grupo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// El formato del repositorio.
    pub format: RepositoryFormat,
    /// Su modo.
    pub mode: RepositoryMode,
    /// Sus miembros, si es un grupo (vacio en otro caso).
    pub members: Vec<String>,
}

/// Lo que dice el catalogo de una clave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// El repositorio existe y quien pregunta puede usarlo.
    Found(Node),
    /// No existe en la cuenta.
    Missing,
    /// Existe pero quien pregunta no puede leerlo: se omite del recorrido,
    /// sin decir si existe.
    Hidden,
}

/// Por que no se pudo recorrer un grupo.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GroupError {
    /// El grupo se contiene a si mismo, directa o indirectamente.
    #[error("the group contains itself: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
    /// Un miembro no existe.
    #[error("the member {0:?} does not exist")]
    MissingMember(String),
    /// Un miembro es de otro formato.
    #[error("the member {member:?} is {found:?}, not {expected:?}")]
    FormatMismatch {
        /// La clave del miembro.
        member: String,
        /// El formato del grupo.
        expected: RepositoryFormat,
        /// El formato del miembro.
        found: RepositoryFormat,
    },
    /// Hay mas niveles de grupos anidados de los admitidos.
    #[error("the groups are nested more than {} levels deep", MAX_DEPTH)]
    TooDeep,
    /// El recorrido visita mas repositorios de los admitidos.
    #[error("the group reaches more than {} repositories", MAX_NODES)]
    TooManyNodes,
}

/// Los repositorios que un recorrido encontro.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Walk {
    /// Los miembros **hoja** (hosted o proxy) en orden de precedencia, sin
    /// repetir: la primera aparicion de una clave es la que cuenta.
    pub leaves: Vec<String>,
    /// Los grupos anidados que se visitaron, en el orden en que se
    /// encontraron.
    pub groups: Vec<String>,
}

/// Recorre un grupo en profundidad y devuelve sus hojas en orden.
///
/// `root_members` son los miembros del grupo raiz **tal y como se quieren**:
/// al validar una configuracion nueva todavia no esta guardada, y al leer
/// es la guardada. `lookup` dice que es cada clave. Si el recorrido vuelve a
/// la raiz, o a cualquier grupo de su camino actual, es un ciclo.
///
/// Un miembro `Hidden` se omite (y con el, lo que contuviera si es un
/// grupo). Un miembro `Missing` es un error: una configuracion que apunta a
/// algo que no existe no se resuelve a medias.
pub fn walk(
    root: &str,
    format: RepositoryFormat,
    root_members: &[String],
    lookup: &dyn Fn(&str) -> Lookup,
) -> Result<Walk, GroupError> {
    let mut walked = Walk::default();
    let mut stack = vec![root.to_string()];
    let mut visited = HashSet::new();
    visited.insert(root.to_string());
    descend(
        format,
        root_members,
        lookup,
        &mut stack,
        &mut visited,
        &mut walked,
    )?;
    Ok(walked)
}

fn descend(
    format: RepositoryFormat,
    members: &[String],
    lookup: &dyn Fn(&str) -> Lookup,
    stack: &mut Vec<String>,
    visited: &mut HashSet<String>,
    walked: &mut Walk,
) -> Result<(), GroupError> {
    for member in members {
        if stack.iter().any(|ancestor| ancestor == member) {
            let mut path = stack.clone();
            path.push(member.clone());
            return Err(GroupError::Cycle(path));
        }
        let node = match lookup(member) {
            Lookup::Hidden => continue,
            Lookup::Missing => return Err(GroupError::MissingMember(member.clone())),
            Lookup::Found(node) => node,
        };
        if node.format != format {
            return Err(GroupError::FormatMismatch {
                member: member.clone(),
                expected: format,
                found: node.format,
            });
        }
        // Un repositorio alcanzable por dos caminos solo cuenta una vez:
        // ni se vuelve a recorrer ni repite sus hojas.
        if !visited.insert(member.clone()) {
            continue;
        }
        if visited.len() > MAX_NODES {
            return Err(GroupError::TooManyNodes);
        }
        if node.mode == RepositoryMode::Group {
            if stack.len() > MAX_DEPTH {
                return Err(GroupError::TooDeep);
            }
            walked.groups.push(member.clone());
            stack.push(member.clone());
            let outcome = descend(format, &node.members, lookup, stack, visited, walked);
            stack.pop();
            outcome?;
        } else {
            walked.leaves.push(member.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn members(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| key.to_string()).collect()
    }

    fn leaf() -> Node {
        Node {
            format: RepositoryFormat::Raw,
            mode: RepositoryMode::Hosted,
            members: Vec::new(),
        }
    }

    fn group(of: &[&str]) -> Node {
        Node {
            format: RepositoryFormat::Raw,
            mode: RepositoryMode::Group,
            members: members(of),
        }
    }

    fn world(entries: Vec<(&str, Lookup)>) -> HashMap<String, Lookup> {
        entries
            .into_iter()
            .map(|(key, lookup)| (key.to_string(), lookup))
            .collect()
    }

    fn walked(root_members: &[&str], world: &HashMap<String, Lookup>) -> Result<Walk, GroupError> {
        walk(
            "root",
            RepositoryFormat::Raw,
            &members(root_members),
            &|key| world.get(key).cloned().unwrap_or(Lookup::Missing),
        )
    }

    #[test]
    fn the_settings_are_read_in_order() {
        let config = GroupConfig::parse(&serde_json::json!({ "members": ["b", "a"] })).unwrap();
        assert_eq!(config.members, members(&["b", "a"]));
    }

    #[test]
    fn a_group_without_members_or_with_junk_is_an_error() {
        for bad in [
            serde_json::json!({ "members": [] }),
            serde_json::json!({ "members": ["a", "a"] }),
            serde_json::json!({ "members": ["a/b"] }),
            serde_json::json!({ "members": [""] }),
            serde_json::json!({ "members": ["a b"] }),
            serde_json::json!({ "members": ["a"], "strategy": "newest" }),
            serde_json::json!({ "memebrs": ["a"] }),
            serde_json::json!(["a"]),
        ] {
            assert!(GroupConfig::parse(&bad).is_err(), "{bad}");
        }
        let too_many: Vec<String> = (0..=MAX_MEMBERS).map(|n| format!("r{n}")).collect();
        assert!(GroupConfig::parse(&serde_json::json!({ "members": too_many })).is_err());
    }

    #[test]
    fn a_repository_that_does_not_declare_a_group_has_none() {
        let config = RepositoryConfig::new(crate::DigestScope::Global);
        assert_eq!(GroupConfig::of(&config), Ok(None));
    }

    #[test]
    fn the_leaves_come_out_in_the_order_of_the_members() {
        let world = world(vec![
            ("a", Lookup::Found(leaf())),
            ("b", Lookup::Found(leaf())),
            ("c", Lookup::Found(leaf())),
        ]);
        let walk = walked(&["c", "a", "b"], &world).unwrap();
        assert_eq!(walk.leaves, members(&["c", "a", "b"]));
    }

    #[test]
    fn nested_groups_are_expanded_in_place_and_repeats_count_once() {
        let world = world(vec![
            ("a", Lookup::Found(leaf())),
            ("b", Lookup::Found(leaf())),
            ("c", Lookup::Found(leaf())),
            ("inner", Lookup::Found(group(&["b", "a"]))),
        ]);
        let walk = walked(&["a", "inner", "c"], &world).unwrap();
        // `a` ya habia ganado su sitio: `inner` solo aporta `b`.
        assert_eq!(walk.leaves, members(&["a", "b", "c"]));
        assert_eq!(walk.groups, members(&["inner"]));
    }

    #[test]
    fn a_group_that_contains_itself_is_a_cycle() {
        let world = world(vec![("a", Lookup::Found(leaf()))]);
        let error = walked(&["a", "root"], &world).unwrap_err();
        assert_eq!(error, GroupError::Cycle(members(&["root", "root"])));
    }

    #[test]
    fn a_cycle_through_another_group_names_the_path() {
        let world = world(vec![
            ("one", Lookup::Found(group(&["two"]))),
            ("two", Lookup::Found(group(&["root"]))),
        ]);
        let error = walked(&["one"], &world).unwrap_err();
        assert_eq!(
            error,
            GroupError::Cycle(members(&["root", "one", "two", "root"]))
        );
    }

    #[test]
    fn a_diamond_is_not_a_cycle() {
        let world = world(vec![
            ("left", Lookup::Found(group(&["shared"]))),
            ("right", Lookup::Found(group(&["shared"]))),
            ("shared", Lookup::Found(leaf())),
        ]);
        let walk = walked(&["left", "right"], &world).unwrap();
        assert_eq!(walk.leaves, members(&["shared"]));
    }

    #[test]
    fn a_missing_member_or_another_format_is_an_error() {
        let world = world(vec![(
            "oci",
            Lookup::Found(Node {
                format: RepositoryFormat::Oci,
                ..leaf()
            }),
        )]);
        assert_eq!(
            walked(&["ghost"], &world).unwrap_err(),
            GroupError::MissingMember("ghost".into())
        );
        assert!(matches!(
            walked(&["oci"], &world).unwrap_err(),
            GroupError::FormatMismatch { .. }
        ));
    }

    #[test]
    fn a_hidden_member_is_skipped_and_not_an_error() {
        let world = world(vec![
            ("secret", Lookup::Hidden),
            ("open", Lookup::Found(leaf())),
        ]);
        let walk = walked(&["secret", "open"], &world).unwrap();
        assert_eq!(walk.leaves, members(&["open"]));
    }

    #[test]
    fn nesting_is_bounded() {
        let mut entries = vec![("leaf", Lookup::Found(leaf()))];
        entries.push(("g4", Lookup::Found(group(&["leaf"]))));
        entries.push(("g3", Lookup::Found(group(&["g4"]))));
        entries.push(("g2", Lookup::Found(group(&["g3"]))));
        entries.push(("g1", Lookup::Found(group(&["g2"]))));
        let world = world(entries);
        // root -> g1 -> g2 -> g3 -> g4: el cuarto nivel anidado no se sigue.
        assert_eq!(walked(&["g1"], &world).unwrap_err(), GroupError::TooDeep);
        assert!(walked(&["g2"], &world).is_ok());
    }
}
