//! Fix to the SurrealDB export so that it is importable.
//!
//! The embedded engine's export (2.7) writes, for an `array<T>` field,
//! the definition of the field **and** that of its implicit element `field[*]`.
//! Its own `import` fails on reaching the second one: the element already exists
//! because the first one created it. Without this fix a backup would
//! be written fine and could not be restored, which is the worst way to fail.
//!
//! The rule is strict: only the `field[*]` line that **immediately follows**
//! that of its field is removed, which declares exactly the element's type
//! and adds nothing else (no `ASSERT`, no `DEFAULT`, no `VALUE`).
//! Any other definition is left as is: if it collided, the import
//! would fail loudly, which is better than losing a constraint.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

const DEFINE: &str = "DEFINE FIELD ";
const PLAIN_TAIL: &str = " PERMISSIONS FULL;";

/// What is known about the last collection-typed field definition.
struct Parent {
    name: String,
    table: String,
    element: String,
}

/// The element type of an `array<T>`, `set<T>` or `option<array<T>>` type.
fn element_of(declared: &str) -> Option<String> {
    let declared = declared
        .strip_prefix("option<")
        .and_then(|inner| inner.strip_suffix('>'))
        .unwrap_or(declared);
    let inner = declared
        .strip_prefix("array<")
        .or_else(|| declared.strip_prefix("set<"))?
        .strip_suffix('>')?;
    // `array<string, 5>` carries the maximum length after the comma.
    let element = match inner.rsplit_once(", ") {
        Some((element, length)) if length.chars().all(|c| c.is_ascii_digit()) => element,
        _ => inner,
    };
    Some(element.to_string())
}

/// Reads `DEFINE FIELD <name> ON <table> TYPE <type> PERMISSIONS FULL;`.
fn parse(line: &str) -> Option<(&str, &str, &str)> {
    let rest = line.strip_prefix(DEFINE)?;
    let (name, rest) = rest.split_once(" ON ")?;
    let (table, rest) = rest.split_once(" TYPE ")?;
    let declared = rest.strip_suffix(PLAIN_TAIL)?;
    Some((name, table, declared))
}

fn parent_of(line: &str) -> Option<Parent> {
    let (name, table, declared) = parse(line)?;
    let element = element_of(declared)?;
    Some(Parent {
        name: name.to_string(),
        table: table.to_string(),
        element,
    })
}

/// Whether `line` is the implicit element of the `parent` field.
fn is_implied(parent: &Parent, line: &str) -> bool {
    parse(line).is_some_and(|(name, table, declared)| {
        name.strip_suffix("[*]") == Some(parent.name.as_str())
            && table == parent.table
            && declared == parent.element
    })
}

/// Copies `input` into `output` without the implicit elements.
///
/// Returns how many lines it removed. Reads and writes by lines: the export
/// carries the data and can be large.
pub fn drop_implied_elements(input: &Path, output: &Path) -> std::io::Result<usize> {
    let mut reader = BufReader::new(std::fs::File::open(input)?);
    let mut writer = BufWriter::new(std::fs::File::create(output)?);
    let mut parent: Option<Parent> = None;
    let mut dropped = 0usize;
    // It is read as bytes and what was read is written as is: a text with `\r\n`
    // line breaks inside a value must not change its content.
    let mut raw: Vec<u8> = Vec::new();
    loop {
        raw.clear();
        if reader.read_until(b'\n', &mut raw)? == 0 {
            break;
        }
        let text = std::str::from_utf8(&raw).ok().map(|text| {
            text.strip_suffix('\n')
                .map(|text| text.strip_suffix('\r').unwrap_or(text))
                .unwrap_or(text)
        });
        if let (Some(known), Some(line)) = (parent.as_ref(), text)
            && is_implied(known, line)
        {
            dropped += 1;
            parent = None;
            continue;
        }
        parent = text.and_then(parent_of);
        writer.write_all(&raw)?;
    }
    writer.flush()?;
    writer.into_inner()?.sync_all()?;
    Ok(dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filtered(text: &str) -> (String, usize) {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.surql");
        let output = dir.path().join("out.surql");
        std::fs::write(&input, text).unwrap();
        let dropped = drop_implied_elements(&input, &output).unwrap();
        (std::fs::read_to_string(&output).unwrap(), dropped)
    }

    #[test]
    fn the_implied_element_of_an_array_is_dropped() {
        let text = "DEFINE FIELD candidates ON gc_run TYPE array<string> PERMISSIONS FULL;\n\
                    DEFINE FIELD candidates[*] ON gc_run TYPE string PERMISSIONS FULL;\n\
                    DEFINE FIELD other ON gc_run TYPE int PERMISSIONS FULL;\n";
        let (out, dropped) = filtered(text);
        assert_eq!(dropped, 1);
        assert_eq!(
            out,
            "DEFINE FIELD candidates ON gc_run TYPE array<string> PERMISSIONS FULL;\n\
             DEFINE FIELD other ON gc_run TYPE int PERMISSIONS FULL;\n"
        );
    }

    #[test]
    fn sets_optional_arrays_and_bounded_arrays_are_recognized() {
        for declared in ["set<int>", "option<array<int>>", "array<int, 5>"] {
            let text = format!(
                "DEFINE FIELD xs ON t TYPE {declared} PERMISSIONS FULL;\n\
                 DEFINE FIELD xs[*] ON t TYPE int PERMISSIONS FULL;\n"
            );
            let (out, dropped) = filtered(&text);
            assert_eq!(dropped, 1, "{declared}");
            assert_eq!(out.lines().count(), 1, "{declared}");
        }
    }

    #[test]
    fn an_element_with_its_own_constraints_is_kept() {
        let text = "DEFINE FIELD xs ON t TYPE array<string> PERMISSIONS FULL;\n\
                    DEFINE FIELD xs[*] ON t TYPE string ASSERT $value != '' PERMISSIONS FULL;\n";
        let (out, dropped) = filtered(text);
        assert_eq!(dropped, 0);
        assert_eq!(out, text);
    }

    #[test]
    fn an_element_of_another_type_or_table_or_field_is_kept() {
        for child in [
            "DEFINE FIELD xs[*] ON t TYPE int PERMISSIONS FULL;",
            "DEFINE FIELD xs[*] ON u TYPE string PERMISSIONS FULL;",
            "DEFINE FIELD ys[*] ON t TYPE string PERMISSIONS FULL;",
        ] {
            let text =
                format!("DEFINE FIELD xs ON t TYPE array<string> PERMISSIONS FULL;\n{child}\n");
            let (out, dropped) = filtered(&text);
            assert_eq!(dropped, 0, "{child}");
            assert_eq!(out, text, "{child}");
        }
    }

    #[test]
    fn an_element_that_does_not_follow_its_field_is_kept() {
        let text = "DEFINE FIELD xs ON t TYPE array<string> PERMISSIONS FULL;\n\
                    DEFINE FIELD z ON t TYPE int PERMISSIONS FULL;\n\
                    DEFINE FIELD xs[*] ON t TYPE string PERMISSIONS FULL;\n";
        let (out, dropped) = filtered(text);
        assert_eq!(dropped, 0);
        assert_eq!(out, text);
    }

    #[test]
    fn data_lines_are_copied_byte_for_byte() {
        let text = "INSERT [ { id: t:1, note: 'DEFINE FIELD xs[*] ON t TYPE string PERMISSIONS FULL;' } ];\n";
        let (out, dropped) = filtered(text);
        assert_eq!(dropped, 0);
        assert_eq!(out, text);
    }

    #[test]
    fn crlf_line_endings_are_preserved() {
        let text = "INSERT [ { id: t:1, note: 'a\r\nb' } ];\r\n\
                    DEFINE FIELD xs ON t TYPE array<string> PERMISSIONS FULL;\r\n\
                    DEFINE FIELD xs[*] ON t TYPE string PERMISSIONS FULL;\r\n\
                    DEFINE FIELD z ON t TYPE int PERMISSIONS FULL;\r\n";
        let (out, dropped) = filtered(text);
        assert_eq!(dropped, 1);
        assert_eq!(
            out,
            "INSERT [ { id: t:1, note: 'a\r\nb' } ];\r\n\
             DEFINE FIELD xs ON t TYPE array<string> PERMISSIONS FULL;\r\n\
             DEFINE FIELD z ON t TYPE int PERMISSIONS FULL;\r\n"
        );
    }
}
