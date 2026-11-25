//! Extraction of the shell blocks of `docs/community/ecosystems.md`.
//!
//! Shared by `hsp243_user_guides` (documented requests versus the recipe)
//! and `hsp288_native_guides` (the native-client blocks are executed as
//! written). Not a test target: it lives in a subdirectory.

#![allow(dead_code)]

/// The text of the `### ` section whose heading starts with `heading`,
/// up to the next `### ` / `## ` heading or horizontal rule.
pub fn section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("missing section '{heading}'"));
    let rest = &doc[start + heading.len()..];
    let end = ["\n### ", "\n## ", "\n---"]
        .iter()
        .filter_map(|marker| rest.find(marker))
        .min()
        .unwrap_or(rest.len());
    &doc[start..start + heading.len() + end]
}

/// One fenced `bash` block: the text preceding it (back to the previous
/// fence or heading) and its body.
pub struct Block {
    pub lead_in: String,
    pub body: String,
}

/// Every fenced `bash` block of `text`, in order.
pub fn bash_blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut lead_in = String::new();
    let mut body: Option<String> = None;
    for line in text.lines() {
        match body.as_mut() {
            Some(current) => {
                if line.trim_start().starts_with("```") {
                    blocks.push(Block {
                        lead_in: std::mem::take(&mut lead_in),
                        body: body.take().unwrap_or_default(),
                    });
                } else {
                    current.push_str(line);
                    current.push('\n');
                }
            }
            None => {
                if line.trim_start().starts_with("```bash") {
                    body = Some(String::new());
                } else if line.trim_start().starts_with("```") {
                    lead_in.clear();
                } else {
                    lead_in.push_str(line);
                    lead_in.push('\n');
                }
            }
        }
    }
    blocks
}

/// The native-client block of a section: the one introduced by the
/// `**Ejemplo con cliente nativo` paragraph.
pub fn native_block(section: &str) -> Option<String> {
    bash_blocks(section)
        .into_iter()
        .find(|block| block.lead_in.contains("**Ejemplo con cliente nativo"))
        .map(|block| block.body)
}

/// The commands of a block: backslash continuations joined, comments and
/// blank lines dropped.
pub fn commands(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if pending.is_empty() && (trimmed.is_empty() || trimmed.starts_with('#')) {
            continue;
        }
        if let Some(stripped) = trimmed.strip_suffix('\\') {
            pending.push_str(stripped);
            pending.push(' ');
            continue;
        }
        pending.push_str(trimmed);
        out.push(std::mem::take(&mut pending));
    }
    out
}

/// Shell-like word splitting: single and double quotes group, no escapes
/// or expansions are interpreted.
pub fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut in_word = false;
    for c in command.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            None => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        out.push(current);
    }
    out
}

/// The literal prefix every documented URL of the lab starts with.
pub const LAB_URL: &str = "http://127.0.0.1:${PORT}";

/// `(METHOD, path)` of a `curl` command that targets the lab, or `None`
/// for any other command. The method is `-X` when given, `POST` when the
/// command carries a body (`-F`, `-d`, `--data-binary`), `GET` otherwise.
pub fn curl_request(command: &str) -> Option<(String, String)> {
    let words = words(command);
    let position = words.iter().position(|w| w == "curl")?;
    let args = &words[position + 1..];
    let url = args.iter().find(|w| w.starts_with(LAB_URL))?;
    let path = url[LAB_URL.len()..]
        .split('?')
        .next()
        .unwrap_or_default()
        .to_string();
    let explicit = args
        .iter()
        .position(|w| w == "-X")
        .and_then(|i| args.get(i + 1));
    let method = match explicit {
        Some(method) => method.clone(),
        None if args
            .iter()
            .any(|w| w == "-F" || w == "-d" || w == "--data-binary") =>
        {
            "POST".to_string()
        }
        None => "GET".to_string(),
    };
    Some((method, path))
}

/// Every `http://` URL token (up to the first quote or whitespace) of a
/// command, verbatim.
pub fn http_urls(command: &str) -> Vec<String> {
    words(command)
        .into_iter()
        .flat_map(|word| {
            word.match_indices("http://")
                .map(|(i, _)| {
                    word[i..]
                        .split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                        .next()
                        .unwrap_or_default()
                        .to_string()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The requests the Maven, npm and PyPI recipes make, in order. The recipe
/// test asserts that it sends exactly these; the documentation test asserts
/// that the `curl` blocks of `ecosystems.md` document exactly these.
pub const RECIPE_REQUESTS: &[(&str, &str, &str)] = &[
    ("maven2", "POST", "/api/v1/catalog/repositories"),
    (
        "maven2",
        "PUT",
        "/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.pom",
    ),
    (
        "maven2",
        "PUT",
        "/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.jar",
    ),
    (
        "maven2",
        "GET",
        "/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.jar",
    ),
    ("npm", "POST", "/api/v1/catalog/repositories"),
    ("npm", "PUT", "/npm/doc-npm/@acme%2fdemo"),
    ("npm", "GET", "/npm/doc-npm/@acme%2fdemo"),
    ("npm", "GET", "/npm/doc-npm/@acme/demo/-/demo-1.0.0.tgz"),
    ("pypi", "POST", "/api/v1/catalog/repositories"),
    ("pypi", "POST", "/pypi/doc-pypi/"),
    ("pypi", "GET", "/pypi/doc-pypi/simple/demo-pkg/"),
    (
        "pypi",
        "GET",
        "/pypi/doc-pypi/files/demo-pkg/demo_pkg-1.0.0-py3-none-any.whl",
    ),
];
