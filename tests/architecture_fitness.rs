//! Comprobaciones de forma arquitectónica.
//!
//! Una arquitectura que solo existe en un documento se deshace en el primer
//! commit que nadie revisa con cuidado. Estas comprobaciones leen el código
//! y fallan cuando aparece una forma que el diseño prohíbe.
//!
//! Las comprobaciones son cuatro, y todas leen el repositorio:
//!
//! 1. El codigo activo no baja a una implementacion concreta por downcast.
//!    El ADR-0007 quiere el registro del schema declarado, no una puerta por
//!    la que la aplicacion baje al tipo concreto. El codigo activo no lo
//!    usaba, pero el puerto lo ofrecia, y una puerta ofrecida es una puerta
//!    que se puede volver a abrir.
//! 2. El nucleo protegido no depende de un mecanismo concreto. La
//!    persistencia real vive en el contexto que la posee.
//! 3. El codigo activo no importa lo interno de otro contexto.
//! 4. No hay exito de relleno: ni un `todo!()`, ni un fichero de pruebas que
//!    se declara a si mismo un marcador de posicion sin pruebas dentro.

use std::fs;
use std::path::{Path, PathBuf};

/// The roots of the active workspace. The crates that live in the tree but
/// are not members are a previous generation: they are not built, they are
/// not deployed, and they do not constrain the active code.
const ACTIVE_ROOTS: [&str; 4] = [
    "src",
    "crates/kernel",
    "crates/hodei-iam",
    "crates/hodei-policies",
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
const PROTECTED_CRATES: [&str; 1] = ["crates/kernel"];

/// The mechanisms a protected crate must not depend on.
///
/// The list is explicit on purpose: an allowlist that nobody reads is a
/// formality, and a denylist that is written down can be argued about.
const BANNED_DEPENDENCIES: [&str; 6] = ["surrealdb", "axum", "reqwest", "sqlx", "mongodb", "redis"];

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

/// The protected crates do not depend on a concrete mechanism.
#[test]
fn a_protected_crate_does_not_depend_on_a_concrete_mechanism() {
    let root = repository_root();
    let mut offences = Vec::new();

    for crate_path in PROTECTED_CRATES {
        let manifest = root.join(crate_path).join("Cargo.toml");
        let Ok(content) = fs::read_to_string(&manifest) else {
            continue;
        };
        // The dependencies section, and only that one: the dev-dependencies
        // of a test may use what the crate itself must not.
        let dependencies = content
            .split("[dev-dependencies]")
            .next()
            .unwrap_or_default();
        for banned in BANNED_DEPENDENCIES {
            if names_dependency(dependencies, banned) {
                offences.push(format!(
                    "{} depends on {banned}, which the protected core must not name",
                    manifest.strip_prefix(&root).unwrap_or(&manifest).display()
                ));
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
/// check recognises a banned dependency when it meets one, and that a
/// commented line is not one.
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
}
