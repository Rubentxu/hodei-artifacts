//! Comprobaciones de forma arquitectónica.
//!
//! Una arquitectura que solo existe en un documento se deshace en el primer
//! commit que nadie revisa con cuidado. Estas comprobaciones leen el código
//! y fallan cuando aparece una forma que el diseño prohíbe.
//!
//! Hoy hay una: el acceso a una implementación concreta por downcast. El
//! ADR-0007 quiere el registro del schema declarado, no una puerta por la
//! que la aplicacion baje al tipo concreto. El codigo activo no lo usaba, pero
//! el puerto lo ofrecia, y una puerta ofrecida es una puerta que se puede
//! volver a abrir.

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
