//! A group repository: several repositories of the same format behind
//! a single endpoint, with an explicit precedence.
//!
//! A group neither stores bytes nor replicates them: it is an **ordered list** of
//! repositories (hosted, proxy or other groups) and a read rule. This
//! module is pure: it validates the configuration and walks the member graph;
//! whoever reads the catalog and decides permissions is the service.
//!
//! # The configuration
//!
//! It lives in `config_settings.group`:
//!
//! ```json
//! { "group": { "members": ["internal", "central-mirror"] } }
//! ```
//!
//! - The **order** is the precedence: the first member that has the
//!   path wins. There is no voting nor "the most recent": two reads of the same
//!   path with the same configuration give the same source.
//! - All members are of the same format as the group and of the same
//!   account. A group may contain other groups (up to [`MAX_DEPTH`]
//!   levels); a cycle, a member that does not exist or of another format prevents
//!   saving the configuration.
//! - An unknown key is an error, not an ignored setting.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

use crate::{RepositoryConfig, RepositoryFormat, RepositoryMode};

/// The direct members that a group admits.
pub const MAX_MEMBERS: usize = 16;

/// The levels of nested groups that are followed, counting the root group
/// as level 0.
pub const MAX_DEPTH: usize = 3;

/// The distinct repositories that a walk may visit. It bounds the
/// catalog reads of a single request.
pub const MAX_NODES: usize = 64;

const MAX_KEY_LEN: usize = 128;

/// Why a group configuration is not valid.
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

/// The configuration of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupConfig {
    /// The keys of the members, in order of precedence.
    pub members: Vec<String>,
}

impl GroupConfig {
    /// The group that the configuration of a repository declares.
    ///
    /// `Ok(None)` is "declares none"; a declaration that exists and is not
    /// understood is an error, never an empty list.
    pub fn of(config: &RepositoryConfig) -> Result<Option<Self>, GroupConfigError> {
        let Some(group) = config.config_settings.get("group") else {
            return Ok(None);
        };
        Self::parse(group).map(Some)
    }

    /// Validates a group written as JSON.
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

/// What is known about a repository when walking a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The format of the repository.
    pub format: RepositoryFormat,
    /// Its mode.
    pub mode: RepositoryMode,
    /// Its members, if it is a group (empty otherwise).
    pub members: Vec<String>,
}

/// What the catalog says about a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// The repository exists and whoever asks can use it.
    Found(Node),
    /// It does not exist in the account.
    Missing,
    /// It exists but whoever asks cannot read it: it is omitted from the walk,
    /// without saying whether it exists.
    Hidden,
}

/// Why a group could not be walked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GroupError {
    /// The group contains itself, directly or indirectly.
    #[error("the group contains itself: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
    /// A member does not exist.
    #[error("the member {0:?} does not exist")]
    MissingMember(String),
    /// A member is of another format.
    #[error("the member {member:?} is {found:?}, not {expected:?}")]
    FormatMismatch {
        /// The key of the member.
        member: String,
        /// The format of the group.
        expected: RepositoryFormat,
        /// The format of the member.
        found: RepositoryFormat,
    },
    /// There are more levels of nested groups than allowed.
    #[error("the groups are nested more than {} levels deep", MAX_DEPTH)]
    TooDeep,
    /// The walk visits more repositories than allowed.
    #[error("the group reaches more than {} repositories", MAX_NODES)]
    TooManyNodes,
}

/// The repositories that a walk found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Walk {
    /// The **leaf** members (hosted or proxy) in order of precedence, without
    /// repeats: the first appearance of a key is the one that counts.
    pub leaves: Vec<String>,
    /// The nested groups that were visited, in the order in which they
    /// were found.
    pub groups: Vec<String>,
}

/// Walks a group depth-first and returns its leaves in order.
///
/// `root_members` are the members of the root group **as they are wanted**:
/// when validating a new configuration it is not saved yet, and when reading
/// it is the saved one. `lookup` says what each key is. If the walk returns to
/// the root, or to any group on its current path, it is a cycle.
///
/// A `Hidden` member is omitted (and with it, whatever it contained if it is a
/// group). A `Missing` member is an error: a configuration that points at
/// something that does not exist is not half-resolved.
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
        // A repository reachable by two paths counts only once:
        // it is neither walked again nor repeats its leaves.
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
        // `a` had already won its place: `inner` only contributes `b`.
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
        // root -> g1 -> g2 -> g3 -> g4: the fourth nested level is not followed.
        assert_eq!(walked(&["g1"], &world).unwrap_err(), GroupError::TooDeep);
        assert!(walked(&["g2"], &world).is_ok());
    }
}
