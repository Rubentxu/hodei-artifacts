//! Verification of HSP-243 user guides, links and capabilities matrix.
//!
//! Documentation only: no service, no Dgraph and no `legacy` feature, so it
//! runs in the default build. The executable recipes of the same guides are in
//! `hsp243_user_guides_recipes` (legacy only).

mod guide_blocks;

use guide_blocks::RECIPE_REQUESTS;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use hodei_artifacts_api::AppConfig;
use hodei_catalog::{RepositoryFormat, RepositoryMode};
use serde_json::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every internal link target of a document, verbatim (path and optional
/// `#anchor`). External URLs are skipped; a bare `#anchor` is kept because it
/// points into the same document.
fn extract_link_targets(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(end) = content[i + 2..].find(')')
        {
            let target = content[i + 2..i + 2 + end].trim();
            if !target.is_empty()
                && !target.starts_with("http://")
                && !target.starts_with("https://")
            {
                links.push(target.to_string());
            }
            i += 2 + end + 1;
            continue;
        }
        i += 1;
    }
    links
}

fn extract_markdown_links(content: &str) -> Vec<String> {
    extract_link_targets(content)
        .into_iter()
        .filter_map(|target| {
            let path_only = target.split('#').next().unwrap_or("");
            (!path_only.is_empty()).then(|| path_only.to_string())
        })
        .collect()
}

/// GitHub's slug of one heading text: lowercase, inline markup and
/// punctuation dropped (letters, digits, `_` and `-` survive, Unicode letters
/// included), spaces turned into hyphens.
fn github_slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// The anchors GitHub generates for a Markdown document: one per ATX heading
/// outside fenced code, with `-1`, `-2`... appended to repeated slugs.
fn heading_anchors(content: &str) -> BTreeSet<String> {
    let mut anchors = BTreeSet::new();
    let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
    let mut in_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
            let slug = github_slug(line[hashes..].trim().trim_end_matches('#'));
            let count = seen.entry(slug.clone()).or_insert(0);
            let anchor = if *count == 0 {
                slug
            } else {
                format!("{slug}-{count}")
            };
            *count += 1;
            anchors.insert(anchor);
        }
    }
    anchors
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_workspace_members(cargo_toml: &str) -> BTreeSet<String> {
    let mut members = BTreeSet::new();
    let mut in_members = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members = [") {
            in_members = true;
            continue;
        }
        if in_members {
            if trimmed == "]" {
                break;
            }
            if let Some(stripped) = trimmed.strip_prefix('"')
                && let Some(end) = stripped.find('"')
            {
                members.insert(stripped[..end].to_string());
            }
        }
    }
    members
}

#[test]
fn documentation_links_and_workspace_crates_are_valid() {
    let root = repo_root();
    let mut doc_files = vec![root.join("README.md"), root.join("README.es.md")];
    for entry in fs::read_dir(root.join("docs/community")).expect("docs/community") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            doc_files.push(path);
        }
    }

    for doc in &doc_files {
        let content = fs::read_to_string(doc).expect("read doc");
        let base_dir = doc.parent().expect("parent");
        for link in extract_markdown_links(&content) {
            let resolved = base_dir.join(&link);
            assert!(
                resolved.exists(),
                "broken markdown link in {}: '{}' -> {}",
                doc.display(),
                link,
                resolved.display()
            );
        }
        // Anchors: `file.md#slug` and bare `#slug` must name a real heading.
        for target in extract_link_targets(&content) {
            let Some((path_part, anchor)) = target.split_once('#') else {
                continue;
            };
            if anchor.is_empty() {
                continue;
            }
            let target_file = if path_part.is_empty() {
                doc.clone()
            } else {
                base_dir.join(path_part)
            };
            if target_file.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let text = fs::read_to_string(&target_file).expect("read link target");
            let anchors = heading_anchors(&text);
            assert!(
                anchors.contains(&percent_decode(anchor).to_lowercase()),
                "broken anchor in {}: '{}' (no such heading in {})",
                doc.display(),
                target,
                target_file.display()
            );
        }
    }

    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    let members = parse_workspace_members(&cargo_toml);
    assert!(
        members.len() >= 20,
        "expected full workspace members list, got {}",
        members.len()
    );

    for readme_name in ["README.md", "README.es.md"] {
        let text = fs::read_to_string(root.join(readme_name)).expect(readme_name);
        for member in &members {
            let needle = if member == "." {
                "`hodei-artifacts-api`"
            } else {
                member.as_str()
            };
            assert!(
                text.contains(needle),
                "{readme_name} is missing workspace member '{member}'"
            );
        }
    }
}

#[test]
fn capabilities_table_matches_catalog_and_conformance() {
    let root = repo_root();
    let docs = [
        ("README.md", "Yes", "No"),
        ("README.es.md", "Sí", "No"),
        ("docs/community/ecosystems.md", "Sí", "No"),
    ];

    for (rel_path, yes, no) in docs {
        let content = fs::read_to_string(root.join(rel_path)).expect(rel_path);
        for format in RepositoryFormat::served() {
            let modes = format.serves_modes();
            let hosted = if modes.contains(&RepositoryMode::Hosted) {
                yes
            } else {
                no
            };
            let proxy = if modes.contains(&RepositoryMode::Proxy) {
                yes
            } else {
                no
            };
            let group = if modes.contains(&RepositoryMode::Group) {
                yes
            } else {
                no
            };
            let expected_prefix =
                format!("| `{}` | {hosted} | {proxy} | {group} |", format.as_str());
            assert!(
                content.contains(&expected_prefix),
                "{rel_path} capabilities row mismatch for '{}': expected '{expected_prefix}'",
                format.as_str()
            );
        }
        let helm_row = format!("| `helm` | {no} | {no} | {no} |");
        assert!(
            content.contains(&helm_row),
            "{rel_path} must declare helm as not served: '{helm_row}'"
        );
    }
}

#[test]
fn guides_have_required_metadata_and_no_insecure_production_advice() {
    let root = repo_root();
    let ecosystems =
        fs::read_to_string(root.join("docs/community/ecosystems.md")).expect("ecosystems.md");

    let required_sections = [
        "### Maven 2 (`maven2`)",
        "### npm (`npm`)",
        "### PyPI (`pypi`)",
        "### OCI (`oci`)",
        "### Cargo (`cargo`)",
        "### NuGet (`nuget`)",
        "### Go (`go`)",
        "### APT (`apt`)",
        "### RPM / Yum (`rpm`)",
        "### Helm-OCI (`oci` + `helm`)",
    ];

    for (idx, heading) in required_sections.iter().enumerate() {
        let start = ecosystems
            .find(heading)
            .unwrap_or_else(|| panic!("missing section '{heading}'"));
        let end = required_sections
            .get(idx + 1)
            .and_then(|next| ecosystems[start..].find(next).map(|o| start + o))
            .unwrap_or(ecosystems.len());
        let section = &ecosystems[start..end];

        assert!(
            section.contains("- **Estado:** `VERIFIED`")
                || section.contains("- **Estado:** `NOT_RUN`"),
            "section '{heading}' must declare Estado VERIFIED or NOT_RUN"
        );
        assert!(
            section.contains("- **Versión de cliente:**"),
            "section '{heading}' must declare Versión de cliente"
        );
        assert!(
            section.contains("- **Política de upstream:**")
                && section.contains("proxy.allowed_hosts")
                && section.contains("settings.upstream"),
            "section '{heading}' must declare Política de upstream with proxy.allowed_hosts and settings.upstream"
        );
        assert!(
            section.contains("**Ejemplo offline"),
            "section '{heading}' must include an offline example"
        );
    }

    // Ensure no production HTTP-without-TLS or signature-disable advice exists.
    // The exceptions are structural: an insecure flag may only appear on an
    // exact, listed prose line outside any code fence, and an `http://` URL
    // is accepted only when its parsed host is a loopback host.
    for rel_path in [
        "README.md",
        "README.es.md",
        "docs/community/quickstart.md",
        "docs/community/ecosystems.md",
    ] {
        let text = fs::read_to_string(root.join(rel_path)).expect(rel_path);
        let violations = insecure_advice_violations(rel_path, &text);
        assert!(
            violations.is_empty(),
            "insecure advice in {rel_path}:\n{}",
            violations.join("\n")
        );
    }

    // Verify backup/restore and upgrade operations are linked.
    for rel_path in ["README.md", "README.es.md", "docs/community/ecosystems.md"] {
        let text = fs::read_to_string(root.join(rel_path)).expect(rel_path);
        assert!(
            text.contains("UAT-OPERATIONS.md") && text.contains("hodei-recovery"),
            "{rel_path} must link backup/restore operations (UAT-OPERATIONS.md / hodei-recovery)"
        );
        assert!(
            text.contains("package-release.sh"),
            "{rel_path} must link upgrade/release packaging (package-release.sh)"
        );
    }
}

/// Insecure flags that may be named only inside a prohibition.
const INSECURE_FLAGS: [&str; 6] = [
    "insecure-registries",
    "strict-ssl=false",
    "[trusted=yes]",
    "gpgcheck=0",
    "sslverify=0",
    "--insecure",
];

/// The exact prose lines (trimmed, per file) that may name an insecure flag
/// because they prohibit it. Any other line naming a flag, and any line
/// inside a code fence, is a violation: wording a line differently does not
/// make it an exception, it must be reviewed and listed here.
const ALLOWED_PROHIBITIONS: &[(&str, &str)] = &[
    (
        "docs/community/ecosystems.md",
        "- **Credenciales seguras:** en `.npmrc` usa `//artifacts.example.com/npm/doc-npm/:_authToken=${HODEI_TOKEN}` (mantén TLS activo en producción; no uses `strict-ssl=false`).",
    ),
    (
        "docs/community/ecosystems.md",
        "- **Credenciales seguras:** `docker login artifacts.example.com` (credenciales almacenadas en credential helper del sistema; en producción siempre HTTPS, nunca `insecure-registries`).",
    ),
    (
        "docs/community/ecosystems.md",
        "- **Credenciales seguras y firma:** instala la clave pública de `/apt/{repo}/key.asc` en `/etc/apt/keyrings/` (`signed-by=...`); **nunca** uses `[trusted=yes]` en producción. Usa `/etc/apt/auth.conf.d/` (permisos `600`) para credenciales.",
    ),
    (
        "docs/community/ecosystems.md",
        "- **Credenciales seguras y firma:** configura `gpgcheck=1` y `repo_gpgcheck=1` con `gpgkey=https://artifacts.example.com/yum/doc-rpm/key.asc`; **nunca** uses `gpgcheck=0` ni `sslverify=0` en producción.",
    ),
    (
        "docs/community/ecosystems.md",
        "- **Credenciales seguras:** `helm registry login artifacts.example.com` (sobre HTTPS en producción; verifica provenance/digests y no uses `--insecure` en producción).",
    ),
];

/// Hosts an `http://` URL may name: the offline lab only.
const LOOPBACK_HOSTS: [&str; 2] = ["127.0.0.1", "localhost"];

/// The host of every `http://` URL in `line`, parsed (not matched as a
/// substring): the characters after the scheme up to the first delimiter.
fn http_hosts(line: &str) -> Vec<String> {
    line.match_indices("http://")
        .map(|(i, _)| {
            line[i + "http://".len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
                .collect::<String>()
        })
        .collect()
}

/// Every insecure-advice violation of one document.
fn insecure_advice_violations(rel_path: &str, text: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        let lower = line.to_lowercase();
        for flag in INSECURE_FLAGS {
            if !lower.contains(flag) {
                continue;
            }
            let listed = !in_fence
                && ALLOWED_PROHIBITIONS
                    .iter()
                    .any(|(file, exact)| *file == rel_path && *exact == line.trim());
            if !listed {
                violations.push(format!("unlisted use of '{flag}': {line}"));
            }
        }
        for host in http_hosts(line) {
            if !LOOPBACK_HOSTS.contains(&host.as_str()) {
                violations.push(format!("non-loopback http:// host '{host}': {line}"));
            }
        }
    }
    violations
}

#[test]
fn insecure_advice_filter_has_no_word_based_exceptions() {
    // The old filter accepted any line that merely contained "nunca",
    // "ejemplo", "local" or "https://". None of these may excuse a violation.
    let cases = [
        "nunca desactives nada, pero usa gpgcheck=0 en producción",
        "Ejemplo: `sslverify=0` en el servidor local",
        "usa --insecure con https://artifacts.example.com",
    ];
    for line in cases {
        assert!(
            !insecure_advice_violations("docs/community/ecosystems.md", line).is_empty(),
            "a word in the line must not excuse an insecure flag: {line}"
        );
    }
    for line in [
        "descarga de http://artifacts.example.com/x (ejemplo, local, https://)",
        "ver http://evil.localhost.example.com/ejemplo",
        "mira http://127.0.0.1.evil.com/ejemplo",
    ] {
        assert!(
            !insecure_advice_violations("docs/community/ecosystems.md", line).is_empty(),
            "a word in the line must not excuse a non-loopback http:// URL: {line}"
        );
    }
    // A listed prohibition is accepted in prose, never inside a fence, and
    // never in a file it is not listed for.
    let (file, exact) = ALLOWED_PROHIBITIONS[0];
    assert!(insecure_advice_violations(file, exact).is_empty());
    assert!(!insecure_advice_violations("README.md", exact).is_empty());
    let fenced = format!("```bash\n{exact}\n```");
    assert!(!insecure_advice_violations(file, &fenced).is_empty());
    // Loopback URLs, with or without port, stay valid.
    assert!(
        insecure_advice_violations(
            file,
            "curl http://127.0.0.1:${PORT}/v2/ y http://localhost/"
        )
        .is_empty()
    );
}

#[test]
fn readme_headers_link_the_visual_demo_and_its_generator() {
    let root = repo_root();
    let asset = "docs/community/quickstart-demo.gif";
    for readme_name in ["README.md", "README.es.md"] {
        let text = fs::read_to_string(root.join(readme_name)).expect(readme_name);
        let header = text.split("\n## ").next().expect("header");
        assert!(
            header.contains(&format!("]({asset})")),
            "{readme_name} must link the demo asset in its header"
        );
        assert!(
            text.contains("scripts/render-demo.sh"),
            "{readme_name} must point to the demo generator"
        );
    }
    for rel in [
        asset,
        "docs/community/quickstart-demo.cast",
        "scripts/render-demo.sh",
    ] {
        let meta = fs::metadata(root.join(rel)).unwrap_or_else(|_| panic!("{rel} must exist"));
        assert!(meta.len() > 0, "{rel} must not be empty");
    }
    let gif = fs::read(root.join(asset)).expect(asset);
    assert!(gif.starts_with(b"GIF8"), "{asset} must be a GIF");
    assert!(
        !root.join("docs/community/quickstart-demo.tape").exists(),
        "the obsolete VHS tape must not coexist with render-demo.sh"
    );
}

/// Every leaf path of a JSON document, dotted (`a.b.c`).
fn leaf_paths(prefix: &str, value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                leaf_paths(&path, child, out);
            }
        }
        _ => {
            out.insert(prefix.to_string());
        }
    }
}

/// The text of the section of `configuration.md` whose heading names
/// `` `[section]` ``, up to the next heading of the same or higher level.
fn section_text<'a>(doc: &'a str, section: &str) -> Option<&'a str> {
    let marker = format!("`[{section}]`");
    let mut start = None;
    let mut offset = 0;
    for line in doc.split_inclusive('\n') {
        if line.starts_with("### ") || line.starts_with("## ") {
            if let Some(begin) = start {
                return Some(&doc[begin..offset]);
            }
            if line.contains(&marker) {
                start = Some(offset);
            }
        }
        offset += line.len();
    }
    start.map(|begin| &doc[begin..])
}

#[test]
fn every_serde_config_field_is_documented_in_configuration_md() {
    use hodei_artifacts_api::config::{ContentBackend, S3ContentConfig};
    use hodei_graph_client::{GraphClientConfig, GraphLoginCredentials, GraphTlsConfig};

    // A configuration with every optional table populated, so that the real
    // serde output enumerates every field of AppConfig and GraphClientConfig.
    // The struct literals are exhaustive on purpose: adding a field breaks
    // this test at compile time until it is documented and covered here.
    let mut config = AppConfig::default();
    config.content.backend = ContentBackend::default();
    config.content.s3 = Some(S3ContentConfig {
        bucket: "b".into(),
        endpoint: Some("e".into()),
        region: Some("r".into()),
        tls: true,
    });
    config.graph = Some(GraphClientConfig {
        endpoint: "127.0.0.1:9080".into(),
        tls: GraphTlsConfig {
            enabled: true,
            ca_cert_path: Some("ca".into()),
            client_cert_path: Some("cert".into()),
            client_key_path: Some("key".into()),
            server_name: Some("name".into()),
        },
        credentials: Some(GraphLoginCredentials {
            userid: "u".into(),
            password: "p".into(),
        }),
        connect_timeout_ms: 1,
        request_timeout_ms: 1,
        namespace: 0,
    });
    let value = serde_json::to_value(&config).expect("serialize AppConfig");
    let mut paths = BTreeSet::new();
    leaf_paths("", &value, &mut paths);

    // Fields that are `skip_serializing` (secrets or paths that must never be
    // dumped) cannot show up in the serialized form; they are documented in
    // section 5.1 and listed here explicitly.
    for hidden in ["security.auth_jwt_secret", "security.auth_jwks_path"] {
        paths.insert(hidden.to_string());
    }
    assert!(
        paths.contains("graph.tls.ca_cert_path") && paths.contains("content.s3.bucket"),
        "the reflection must reach nested tables: {paths:?}"
    );

    let doc = fs::read_to_string(repo_root().join("docs/community/configuration.md"))
        .expect("configuration.md");
    let mut missing = Vec::new();
    for path in &paths {
        let full = format!("`{path}`");
        if doc.contains(&full) {
            continue;
        }
        // Two-level keys are documented as bare `key` rows under the
        // section heading `[section]` (sections 5.x).
        let documented_in_section = path
            .split_once('.')
            .filter(|(_, leaf)| !leaf.contains('.'))
            .and_then(|(section, leaf)| {
                section_text(&doc, section).map(|text| text.contains(&format!("| `{leaf}` |")))
            })
            .unwrap_or(false);
        if !documented_in_section {
            missing.push(path.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "configuration keys missing from docs/community/configuration.md: {missing:?}"
    );

    // Every documented graph key also names its real environment variable.
    for path in paths.iter().filter(|p| p.starts_with("graph.")) {
        let variable = format!("HODEI_{}", path.to_uppercase().replace('.', "__"));
        assert!(
            doc.contains(&variable),
            "configuration.md must name the variable {variable} for {path}"
        );
    }
}

/// AC2 (HSP-288): the `curl` blocks of the Maven, npm and PyPI sections of
/// `ecosystems.md` are extracted and compared, method and path, with the
/// requests the executable recipe sends (`RECIPE_REQUESTS`).
#[test]
fn documented_curl_blocks_match_the_requests_the_recipe_sends() {
    let doc = fs::read_to_string(repo_root().join("docs/community/ecosystems.md"))
        .expect("ecosystems.md");
    for (ecosystem, heading) in [
        ("maven2", "### Maven 2 (`maven2`)"),
        ("npm", "### npm (`npm`)"),
        ("pypi", "### PyPI (`pypi`)"),
    ] {
        let section = guide_blocks::section(&doc, heading);
        let documented: Vec<(String, String)> = guide_blocks::bash_blocks(section)
            .iter()
            // The native-client block is executed by hsp288_native_guides.
            .filter(|block| !block.lead_in.contains("**Ejemplo con cliente nativo"))
            .flat_map(|block| guide_blocks::commands(&block.body))
            .filter_map(|command| guide_blocks::curl_request(&command))
            .collect();
        let tested: Vec<(String, String)> = RECIPE_REQUESTS
            .iter()
            .filter(|(eco, _, _)| *eco == ecosystem)
            .map(|(_, method, path)| (method.to_string(), path.to_string()))
            .collect();
        assert!(
            !documented.is_empty(),
            "{ecosystem}: no curl request documented"
        );
        assert_eq!(
            documented, tested,
            "{ecosystem}: documented curl requests differ from the recipe test"
        );
    }
}
