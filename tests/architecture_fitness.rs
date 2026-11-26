//! Architectural shape checks.
//!
//! An architecture that only exists in a document comes undone in the first
//! commit nobody reviews carefully. These checks read the code
//! and fail when a shape the design forbids appears.
//!
//! There are four checks, and all of them read the repository:
//!
//! 1. Active code does not drop down to a concrete implementation by downcast.
//!    ADR-0007 wants the registry of the declared schema, not a door through
//!    which the application goes down to the concrete type. Active code did
//!    not use it, but the port offered it, and an offered door is a door
//!    that can be opened again.
//! 2. The protected kernel and the evidence contracts do not depend on a
//!    concrete mechanism. Real persistence lives in the context that
//!    owns it.
//! 3. Active code does not import the internals of another context.
//! 4. There is no filler success: neither a `todo!()`, nor a test file that
//!    declares itself a placeholder with no tests inside.

use std::fs;
use std::path::{Path, PathBuf};

/// The roots of the active workspace. The crates that live in the tree but
/// are not members are a previous generation: they are not built, they are
/// not deployed, and they do not constrain the active code.
const ACTIVE_ROOTS: [&str; 7] = [
    "src",
    "crates/kernel",
    "crates/hodei-iam",
    "crates/hodei-policies",
    "crates/hodei-evidence",
    "crates/hodei-catalog",
    "crates/hodei-audit",
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every Rust source file of the active workspace.
///
/// The test's own file is skipped: it names the patterns it looks for, and a
/// check that trips over itself is a check nobody trusts.
fn active_sources() -> Vec<PathBuf> {
    let root = repository_root();
    let this_file: PathBuf = Path::new(file!())
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut files = Vec::new();
    for root_path in ACTIVE_ROOTS {
        collect(&root.join(root_path), &mut files);
    }
    files.retain(|path| path.file_name() != Some(this_file.as_os_str()));
    files.sort();
    files
}

fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// The patterns that a downcast would use.
///
/// They are assembled from pieces so that this file does not contain them
/// literally and does not trip its own check.
fn downcast_patterns() -> Vec<String> {
    vec![
        format!("as_{}", "any"),
        format!("downcast_{}", "ref"),
        format!("downcast_{}", "mut"),
    ]
}

/// The active code does not reach a concrete implementation by downcast.
#[test]
fn the_active_code_does_not_downcast() {
    let patterns = downcast_patterns();
    let mut offences = Vec::new();

    for file in active_sources() {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        for (number, line) in content.lines().enumerate() {
            for pattern in &patterns {
                if line.contains(pattern.as_str()) {
                    offences.push(format!(
                        "{}:{}: {pattern}",
                        file.strip_prefix(repository_root())
                            .unwrap_or(&file)
                            .display(),
                        number + 1
                    ));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "the active code reaches a concrete implementation by downcast, which the \
         declared design does not allow:\n{}",
        offences.join("\n")
    );
}

/// The check finds a downcast when there is one.
///
/// A check that cannot fail is not a check. This proves it would catch the
/// escape hatch if it came back.
#[test]
fn the_check_would_catch_a_downcast() {
    let pattern = downcast_patterns()[0].clone();
    let line = format!("    fn {pattern}(&self) -> &dyn std::any::Any {{ self }}");
    let found = downcast_patterns()
        .iter()
        .any(|pattern| line.contains(pattern.as_str()));
    assert!(
        found,
        "the check must recognise a downcast when it meets one: {line}"
    );
}

/// The crates whose dependency set is protected.
///
/// The kernel is the shared core: it names the vocabulary everybody uses,
/// and it must not drag a database engine or an HTTP framework into every
/// crate that uses it.
///
/// The catalog's domain is protected for the same reason and one more: the
/// seam between what the catalog means and where the catalog is stored is
/// only a seam while the domain cannot name a database. The adapter lives in
/// `hodei-catalog-storage`; the domain must stay ignorant of it.
///
/// The audit contract is protected for the same reason: it is the event, the
/// sink and the outbox that everybody speaks, and a contract that drags a
/// database engine into every crate that names it is a contract that stops
/// being a contract.
const PROTECTED_CRATES: [&str; 8] = [
    "crates/kernel",
    "crates/hodei-evidence",
    "crates/hodei-catalog",
    "crates/hodei-audit",
    "crates/hodei-trust-policy",
    "crates/hodei-risk",
    "crates/hodei-reachability",
    "crates/hodei-policies",
];

/// The mechanisms a protected crate must not depend on.
///
/// The list is explicit on purpose: an allowlist that nobody reads is a
/// formality, and a denylist that is written down can be argued about.
const BANNED_DEPENDENCIES: [&str; 11] = [
    "surrealdb",
    "axum",
    "reqwest",
    "http",
    "hyper",
    "tonic",
    "dgraph",
    "hodei-graph-client",
    "sqlx",
    "mongodb",
    "redis",
];

/// Whether a dependency section names the given crate.
///
/// A commented line is not a dependency, and a dependency of the development
/// section is not a dependency of the crate.
fn names_dependency(dependencies: &str, crate_name: &str) -> bool {
    dependencies
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .any(|line| {
            let line = line.trim();
            line.starts_with(crate_name) && line[crate_name.len()..].trim_start().starts_with('=')
        })
}

/// The quoted text of a fragment such as `"crates/kernel",`.
fn quoted(text: &str) -> Option<String> {
    let start = text.find('"')?;
    let rest = &text[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// The `path = "..."` of a dependency line, when it has one.
///
/// A dependency on another crate of this repository is written that way, and
/// it is the edge by which a crate can drag in a mechanism it does not name
/// itself.
fn path_dependency(line: &str) -> Option<String> {
    let at = line.find("path")?;
    let rest = line[at + "path".len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    quoted(rest)
}

/// The members of the workspace, by their path in the repository.
///
/// Only the members: the crates under `exclude` live in the tree but are not
/// built, so they are not edges that anything can reach.
fn workspace_members(root: &Path) -> Vec<String> {
    let Ok(manifest) = fs::read_to_string(root.join("Cargo.toml")) else {
        return Vec::new();
    };
    let mut members = Vec::new();
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if !inside {
            if trimmed.starts_with("members") && trimmed.contains('[') {
                inside = true;
            }
            continue;
        }
        if trimmed.starts_with(']') {
            break;
        }
        if let Some(path) = quoted(trimmed.trim_end_matches(',')) {
            members.push(path);
        }
    }
    members
}

/// A path without its `.` and `..` parts.
fn normalized(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The crates of this repository that a member depends on.
fn workspace_dependencies(root: &Path, member: &str) -> Vec<String> {
    let Ok(manifest) = fs::read_to_string(root.join(member).join("Cargo.toml")) else {
        return Vec::new();
    };
    // The dependencies section, and only that one: the dev-dependencies of
    // a test may use what the crate itself must not.
    let dependencies = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or_default();
    let mut edges = Vec::new();
    for line in dependencies.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(path) = path_dependency(trimmed) else {
            continue;
        };
        let joined = root.join(member).join(path);
        let relative = joined.strip_prefix(root).unwrap_or(&joined);
        let target = normalized(&relative.to_string_lossy());
        if target != member {
            edges.push(target);
        }
    }
    edges
}

/// The crates of this repository that the given member reaches.
///
/// The check follows the edges **inside** this repository: a crate can only
/// drag a mechanism in through another crate of this repository, and those
/// are the edges a commit introduces. The edges that leave the workspace
/// (those of the legacy crates, which are not compiled) are not followed.
fn the_workspace_closure(root: &Path, member: &str) -> Vec<String> {
    let members = workspace_members(root);
    let mut seen: Vec<String> = Vec::new();
    let mut pending = vec![member.to_string()];
    while let Some(current) = pending.pop() {
        if seen.contains(&current) {
            continue;
        }
        seen.push(current.clone());
        for target in workspace_dependencies(root, &current) {
            if members.contains(&target) && !seen.contains(&target) {
                pending.push(target);
            }
        }
    }
    seen
}

/// The protected crates do not depend on a concrete mechanism.
///
/// The check is not only about the manifest of the protected crate: a crate
/// that does not name a database engine can still drag one in through
/// another crate of this repository, and then the boundary it was supposed
/// to keep is a line in a file. So the check follows the edges inside the
/// workspace.
#[test]
fn a_protected_crate_does_not_depend_on_a_concrete_mechanism() {
    let root = repository_root();
    let mut offences = Vec::new();

    for crate_path in PROTECTED_CRATES {
        for member in the_workspace_closure(&root, crate_path) {
            let manifest = root.join(&member).join("Cargo.toml");
            let Ok(content) = fs::read_to_string(&manifest) else {
                continue;
            };
            // The dependencies section, and only that one: the
            // dev-dependencies of a test may use what the crate itself must
            // not.
            let dependencies = content
                .split("[dev-dependencies]")
                .next()
                .unwrap_or_default();
            for banned in BANNED_DEPENDENCIES {
                if names_dependency(dependencies, banned) {
                    offences.push(format!(
                        "{member} depends on {banned}, and {crate_path} reaches it; the protected \
                         core must not name a concrete mechanism"
                    ));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "a protected crate depends on a concrete mechanism:\n{}",
        offences.join("\n")
    );
}

/// The active code does not import what belongs to another context's
/// internals.
///
/// A context publishes its ports; its internals are its own. Importing them
/// turns two bounded contexts into one, and the boundary only exists on
/// paper until somebody needs a field.
#[test]
fn the_active_code_does_not_import_another_context_internals() {
    // The other contexts' internal modules, by name.
    let forbidden = ["hodei_iam::internal", "hodei_policies::internal"];

    let mut offences = Vec::new();
    for file in active_sources() {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        for (number, line) in content.lines().enumerate() {
            for module in forbidden {
                if line.contains(module) {
                    offences.push(format!(
                        "{}:{}: {module}",
                        file.strip_prefix(repository_root())
                            .unwrap_or(&file)
                            .display(),
                        number + 1
                    ));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "the active code imports the internals of another context:\n{}",
        offences.join("\n")
    );
}

/// There is no success of filler in the active code.
///
/// Two shapes are checked: a `todo!()` that panics where a result was
/// promised, and a file of tests that declares itself a placeholder for
/// future tests while containing none. The second one is worse than it
/// looks: the file exists, so the inventory of tests counts it, and nobody
/// notices that nothing runs behind it.
#[test]
fn there_is_no_success_of_filler() {
    let mut offences = Vec::new();

    for file in active_sources() {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        for (number, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("todo!(") || trimmed.starts_with("unimplemented!(") {
                offences.push(format!(
                    "{}:{}: {}",
                    file.strip_prefix(repository_root())
                        .unwrap_or(&file)
                        .display(),
                    number + 1,
                    trimmed
                ));
            }
        }

        // A file whose name says it holds tests, with no test inside.
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let looks_like_a_test_file = name.ends_with("_test.rs")
            || file.parent().is_some_and(|parent| {
                parent
                    .file_name()
                    .is_some_and(|directory| directory == "__tests__")
            });
        if looks_like_a_test_file
            && !content.contains("#[test]")
            && !content.contains("#[tokio::test]")
        {
            offences.push(format!(
                "{}: a file of tests with no test inside",
                file.strip_prefix(repository_root())
                    .unwrap_or(&file)
                    .display()
            ));
        }
    }

    assert!(
        offences.is_empty(),
        "the active code has a success of filler:\n{}",
        offences.join("\n")
    );
}

/// The checks are able to fail.
///
/// A check that cannot fail is not a check. This proves that the dependency
/// check recognises a banned dependency when it meets one, that a
/// commented line is not one, and that the edge between two crates of this
/// repository is recognised as an edge.
#[test]
fn the_dependency_check_would_catch_a_banned_dependency() {
    let manifest = "[dependencies]\nsurrealdb = { workspace = true }\n";
    assert!(
        names_dependency(manifest, "surrealdb"),
        "the check must recognise a banned dependency"
    );
    assert!(
        !names_dependency("[dependencies]\nserde = \"1\"\n", "surrealdb"),
        "an unrelated dependency is not a banned one"
    );
    assert!(
        !names_dependency("[dependencies]\n# surrealdb = \"1\"\n", "surrealdb"),
        "a commented line is not a dependency"
    );
    assert_eq!(
        path_dependency("hodei-audit = { path = \"../hodei-audit\" }"),
        Some("../hodei-audit".to_string()),
        "the check must recognise the edge between two crates of this repository"
    );
    assert_eq!(
        path_dependency("serde = { workspace = true }"),
        None,
        "a dependency of the workspace is not an edge inside it"
    );
}

/// The closure follows the edges inside the workspace.
///
/// A check that walks nothing finds nothing, and then it passes forever.
/// This proves that the closure of a protected crate reaches the crates
/// that crate reaches.
#[test]
fn the_closure_follows_the_edges_inside_the_workspace() {
    let root = repository_root();

    let closure = the_workspace_closure(&root, "crates/hodei-catalog-storage");
    assert!(
        closure.contains(&"crates/hodei-catalog-storage".to_string()),
        "a crate reaches itself: {closure:?}"
    );
    assert!(
        closure.contains(&"crates/hodei-audit".to_string()),
        "the adapter reaches the audit contract: {closure:?}"
    );
    assert!(
        closure.contains(&"crates/hodei-catalog".to_string()),
        "the adapter reaches the catalog domain: {closure:?}"
    );

    // And the domain of the catalog does not reach the adapter: that is the
    // frontier the name of the crate protects.
    let domain = the_workspace_closure(&root, "crates/hodei-catalog");
    assert!(
        !domain.contains(&"crates/hodei-catalog-storage".to_string()),
        "the domain must not reach the adapter: {domain:?}"
    );
}

/// Only the authorized crates in the active workspace may depend on `cedar-policy`.
///
/// No duplicate Cedar engine or copy may be introduced in another workspace member.
#[test]
fn no_new_cedar_copies_in_workspace() {
    let root = repository_root();
    let allowed = ["crates/kernel", "crates/hodei-policies", "."];
    let mut offences = Vec::new();

    for member in workspace_members(&root) {
        if allowed.contains(&member.as_str()) {
            continue;
        }
        let manifest = root.join(&member).join("Cargo.toml");
        let Ok(content) = fs::read_to_string(&manifest) else {
            continue;
        };
        let dependencies = content
            .split("[dev-dependencies]")
            .next()
            .unwrap_or_default();
        if names_dependency(dependencies, "cedar-policy") {
            offences.push(format!(
                "{member} declares cedar-policy outside the authorized policy/kernel crates"
            ));
        }
    }

    assert!(
        offences.is_empty(),
        "new cedar-policy dependency found in workspace:\n{}",
        offences.join("\n")
    );
}

/// Handlers and the shared resolution seam must not construct duplicate
/// authorization authorities; all authorization goes through `AuthorizeResourceUseCase`.
#[test]
fn no_duplicate_cedar_authorities_in_handlers_or_seam() {
    let root = repository_root();
    let mut files = Vec::new();
    collect(&root.join("src/handlers"), &mut files);
    files.push(root.join("src/resolution.rs"));

    let forbidden = ["CedarAuthorizer", "hodei_authz::cedar"];
    let mut offences = Vec::new();
    for file in files {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        for (number, line) in content.lines().enumerate() {
            for pattern in forbidden {
                if line.contains(pattern) {
                    offences.push(format!(
                        "{}:{}: {pattern}",
                        file.strip_prefix(&root).unwrap_or(&file).display(),
                        number + 1
                    ));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "handlers or resolution seam construct a duplicate Cedar authority:\n{}",
        offences.join("\n")
    );
}

/// Migrated protocol handlers consume the shared resolution seam (`crate::resolution`)
/// and must not cross-import `handlers/raw.rs`.
#[test]
fn migrated_protocol_handlers_do_not_cross_import_raw_handler() {
    let root = repository_root();
    let migrated = ["src/handlers/npm.rs", "src/handlers/pypi.rs"];
    let forbidden = [
        "super::raw::",
        "super::raw;",
        "super::raw ",
        "crate::handlers::raw::",
        "crate::handlers::raw;",
        "crate::handlers::raw ",
    ];
    let mut offences = Vec::new();

    for rel in migrated {
        let path = root.join(rel);
        let content = fs::read_to_string(&path).expect("migrated handler must exist");
        for (number, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            for pattern in forbidden {
                if line.contains(pattern) {
                    offences.push(format!("{rel}:{}: {pattern}", number + 1));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "migrated protocol handlers must not cross-import handlers/raw.rs:\n{}",
        offences.join("\n")
    );
}
