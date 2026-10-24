//! Guard for the source tree of the application crate.
//!
//! # Why this file exists
//!
//! `src/infrastructure/` used to be a directory of Rust sources that no
//! module declaration reached: it was not declared in `src/lib.rs`, so rustc
//! never saw it and the `GetEffectiveScpsAdapter` it contained did not exist
//! in the binary. The code was not *wrong* — it was **outside the
//! compilation unit**, which made it documentation pretending to be product.
//!
//! rustc cannot catch this on its own. A `.rs` file that no `mod`
//! declaration reaches is simply not an input to the compilation, so the
//! build stays green while the file drifts away from the code it claims to
//! belong to.
//!
//! So the guard is a small resolver: it walks the `mod` declarations from the
//! crate roots and reports every source file that is never reached.
//!
//! # Guarantees
//!
//! - T1 Every `.rs` file under `src/` is reached from a crate root by
//!   following `mod` declarations. A file that is not reached is code that
//!   does not exist as far as the product is concerned.
//! - T2 The resolver has power: given a synthetic tree with an undeclared
//!   module, it reports it as unreached. Without this, T1 could pass
//!   vacuously — a resolver that found nothing would also pass T1.
//! - T3 The resolver does not report a false orphan: a file declared inside
//!   an inline module (`mod inline { mod helper; }`) is reached, so the
//!   legitimate Rust 2018 layout does not fail the guard.
//!
//! The test is deliberately cheap: it only reads the working tree, spawns
//! nothing and binds no port.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Root of the package under test.
fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The source root of the application crate.
fn src_root() -> PathBuf {
    manifest_dir().join("src")
}

/// A `mod` declaration found in a source file.
#[derive(Debug)]
struct Declaration {
    /// The declared module name.
    name: String,
    /// The chain of enclosing inline modules, outermost first.
    ///
    /// Rust 2018 puts the children of an inline module in a directory named
    /// after it, so the scope is needed to resolve `mod helper;` declared
    /// inside `mod inline { .. }`.
    scope: Vec<String>,
}

/// The outcome of walking the module declarations of a source tree.
#[derive(Debug, Default)]
struct Walk {
    /// Files reached from a crate root, relative to the source root.
    reached: BTreeSet<String>,
    /// Declarations that point at a file that does not exist.
    dangling: Vec<String>,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Returns the source with comments and literals removed.
///
/// The resolver must not be fooled by a `mod` declaration that only appears
/// inside a comment, a string or a character literal, so all of them are
/// dropped before scanning. Rust block comments nest, so the depth is
/// tracked.
fn code_only(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                let mut depth = 0usize;
                while i < chars.len() {
                    if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            'r' if matches!(chars.get(i + 1), Some('"') | Some('#')) => {
                let mut j = i + 1;
                let mut hashes = 0usize;
                while chars.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if chars.get(j) == Some(&'"') {
                    j += 1;
                    loop {
                        if j >= chars.len() {
                            break;
                        }
                        if chars[j] == '"' {
                            let mut k = j + 1;
                            let mut seen = 0usize;
                            while seen < hashes && chars.get(k) == Some(&'#') {
                                seen += 1;
                                k += 1;
                            }
                            if seen == hashes {
                                j = k;
                                break;
                            }
                        }
                        j += 1;
                    }
                    i = j;
                } else {
                    // A plain identifier that starts with `r`.
                    out.push(chars[i]);
                    i += 1;
                }
            }
            '"' => {
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == '"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            '\'' => {
                match chars.get(i + 1).copied() {
                    // A lifetime (`'a`) or a character literal (`'a'`).
                    Some(ch) if ch != '\\' && (ch.is_alphanumeric() || ch == '_') => {
                        if chars.get(i + 2) == Some(&'\'') {
                            i += 3;
                        } else {
                            i += 1;
                        }
                    }
                    _ => {
                        i += 1;
                        while i < chars.len() {
                            if chars[i] == '\\' {
                                i += 2;
                                continue;
                            }
                            if chars[i] == '\'' {
                                i += 1;
                                break;
                            }
                            i += 1;
                        }
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }

    out.into_iter().collect()
}

/// Scans the code for `mod` declarations.
///
/// Only file modules (`mod name;`) are reported: an inline module
/// (`mod name { .. }`) has no file of its own, but its children are resolved
/// through the recorded scope.
fn declarations(code: &str) -> Vec<Declaration> {
    let chars: Vec<char> = code.chars().collect();
    let mut decls: Vec<Declaration> = Vec::new();
    // Whether each open brace was opened by an inline module.
    let mut braces: Vec<bool> = Vec::new();
    let mut scope: Vec<String> = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if is_ident_start(c) {
            let start = i;
            while i < chars.len() && is_ident_continue(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if word != "mod" {
                continue;
            }

            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j >= chars.len() || !is_ident_start(chars[j]) {
                continue;
            }
            let name_start = j;
            while j < chars.len() && is_ident_continue(chars[j]) {
                j += 1;
            }
            let name: String = chars[name_start..j].iter().collect();

            let mut k = j;
            while k < chars.len() && chars[k].is_whitespace() {
                k += 1;
            }
            match chars.get(k) {
                Some(';') => {
                    decls.push(Declaration {
                        name,
                        scope: scope.clone(),
                    });
                    i = j;
                }
                Some('{') => {
                    // Inline module: its children live in a directory named
                    // after it, so the scope grows.
                    braces.push(true);
                    scope.push(name);
                    i = k + 1;
                }
                _ => {
                    i = j;
                }
            }
            continue;
        }

        if c == '{' {
            braces.push(false);
            i += 1;
            continue;
        }
        if c == '}' {
            if braces.pop() == Some(true) {
                scope.pop();
            }
            i += 1;
            continue;
        }
        i += 1;
    }

    decls
}

/// Every `.rs` file under `root`, relative to `root`, sorted.
fn source_files(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    collect_rs_files(root, root, &mut found);
    found.sort();
    found
}

fn collect_rs_files(root: &Path, dir: &Path, found: &mut Vec<String>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("cannot read entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(root, &path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

/// Walks the module declarations of `root` from the given crate roots.
fn walk(root: &Path, crate_roots: &[&str]) -> Walk {
    let mut result = Walk::default();
    let mut queue: Vec<PathBuf> = crate_roots.iter().map(|r| root.join(r)).collect();

    while let Some(file) = queue.pop() {
        let relative = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        if !result.reached.insert(relative.clone()) {
            continue;
        }
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };

        for decl in declarations(&code_only(&source)) {
            let mut dir = file.parent().unwrap_or(root).to_path_buf();
            for scope in &decl.scope {
                dir = dir.join(scope);
            }
            let as_file = dir.join(format!("{}.rs", decl.name));
            let as_dir = dir.join(&decl.name).join("mod.rs");
            if as_file.is_file() {
                queue.push(as_file);
            } else if as_dir.is_file() {
                queue.push(as_dir);
            } else {
                result
                    .dangling
                    .push(format!("{relative}: mod {};", decl.name));
            }
        }
    }

    result
}

/// The source files that no crate root reaches.
fn orphans(root: &Path, crate_roots: &[&str]) -> (Vec<String>, Walk) {
    let walked = walk(root, crate_roots);
    let unreached: Vec<String> = source_files(root)
        .into_iter()
        .filter(|relative| !walked.reached.contains(relative))
        .collect();
    (unreached, walked)
}

/// T1 — every source file of the application crate is part of the build.
#[test]
fn t1_every_source_file_under_src_is_reached_from_a_crate_root() {
    let root = src_root();
    let (unreached, walked) = orphans(&root, &["lib.rs", "main.rs"]);

    assert!(
        unreached.is_empty(),
        "these files under src/ are not reached by any `mod` declaration, so rustc never \
         compiles them and they are not part of the product: {unreached:?}. Declare the \
         module or delete the file; a source file that pretends to belong to the \
         application is documentation pretending to be code."
    );

    assert!(
        walked.dangling.is_empty(),
        "these module declarations point at a file that does not exist: {:?}",
        walked.dangling
    );
}

/// T2 — the resolver has power: an undeclared module is reported.
#[test]
fn t2_the_resolver_reports_a_module_that_is_not_declared() {
    let dir = tempfile::tempdir().expect("a temporary directory must be available");
    fs::write(dir.path().join("lib.rs"), "pub mod declared;\n").unwrap();
    fs::write(dir.path().join("declared.rs"), "// reached\n").unwrap();
    fs::write(dir.path().join("orphan.rs"), "// nobody declares me\n").unwrap();
    fs::create_dir(dir.path().join("orphan_dir")).unwrap();
    fs::write(
        dir.path().join("orphan_dir").join("mod.rs"),
        "// nobody declares me\n",
    )
    .unwrap();

    let (unreached, walked) = orphans(dir.path(), &["lib.rs"]);

    assert!(
        walked.reached.contains("declared.rs"),
        "a declared module must be reached: {:?}",
        walked.reached
    );
    assert_eq!(
        unreached,
        vec!["orphan.rs".to_string(), "orphan_dir/mod.rs".to_string()],
        "both the orphan file and the orphan directory must be reported"
    );
}

/// T3 — a file declared inside an inline module is reached, not reported.
#[test]
fn t3_the_resolver_reaches_a_file_declared_inside_an_inline_module() {
    let dir = tempfile::tempdir().expect("a temporary directory must be available");
    fs::write(
        dir.path().join("lib.rs"),
        "pub mod alpha;\nmod inline { mod helper; }\n",
    )
    .unwrap();
    fs::write(dir.path().join("alpha.rs"), "// reached\n").unwrap();
    fs::create_dir(dir.path().join("inline")).unwrap();
    fs::write(dir.path().join("inline").join("helper.rs"), "// reached\n").unwrap();

    let (unreached, walked) = orphans(dir.path(), &["lib.rs"]);

    assert!(
        walked.reached.contains("inline/helper.rs"),
        "a module declared inside an inline module must be reached: {:?}",
        walked.reached
    );
    assert!(
        unreached.is_empty(),
        "the inline layout is legitimate and must not be reported: {unreached:?}"
    );
}
