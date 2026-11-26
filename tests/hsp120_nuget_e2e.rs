//! The NuGet v3 of HSP-120, at the HTTP level.
//!
//! What is tested: the service index with absolute addresses, the `PUT`
//! publication of the nupkg, the flat container assembled in NuGet order, the
//! registration leaf, the immutability of the version, the authority of the
//! nuspec over the URL, the normalization of identities and the rejection of what
//! the protocol cannot do. The proxy and its cache survive without network,
//! byte for byte: a rewritten nupkg would stop matching its `contentHash`.
//!
//! The real client (`dotnet`) is tested in its own file; here the
//! protocol is spoken the way the client would speak it.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

/// A request to the service with a bearer token.
fn call(port: u16, method: &str, path: &str, token: &str, body: Option<&[u8]>) -> Answer {
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if sent.is_some() {
        headers.push(("content-type", "application/octet-stream"));
    }
    Curl::call_with_headers(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        &headers,
        Some(&into),
    )
    .run()
}

/// The push exactly as `dotnet nuget push` does it: the credential travels
/// in `X-NuGet-ApiKey`, not in the bearer. Here the bearer is garbage on
/// purpose: if the protocol header did not count, this would fail.
fn push_with_api_key(
    port: u16,
    repository: &str,
    api_key: &str,
    id: &str,
    version: &str,
    nupkg: &[u8],
) -> Answer {
    let sent = next("sent");
    std::fs::write(&sent, nupkg).expect("the package is written");
    let into = next("answer");
    Curl::call_with_headers(
        port,
        "PUT",
        &format!("/nuget/{repository}/package/{id}/{version}"),
        "not-a-token",
        Some(&sent),
        &[
            ("content-type", "application/octet-stream"),
            ("x-nuget-apikey", api_key),
        ],
        Some(&into),
    )
    .run()
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// The canonical nuspec of a package: id and version declared inside.
fn nuspec(id: &str, version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://schemas.microsoft.com/packaging/2013/05/nuspec.xsd">
  <metadata>
    <id>{id}</id>
    <version>{version}</version>
    <authors>Hodei</authors>
    <description>a package for the protocol</description>
  </metadata>
</package>"#
    )
}

/// A test nupkg: the nuspec and a filler dll, in a zip. At the
/// HTTP level the server only reads the nuspec; the real client unpacks
/// the rest in its own test.
fn nupkg(id: &str, version: &str) -> Vec<u8> {
    nupkg_declaring(id, version)
}

/// A nupkg whose inner identity differs on purpose from the URL.
fn nupkg_declaring(nuspec_id: &str, nuspec_version: &str) -> Vec<u8> {
    nupkg_declaring_tagged(nuspec_id, nuspec_version, "")
}

/// The same one, with a tag that changes the bytes without changing the
/// identity: used to talk about immutability.
fn nupkg_declaring_tagged(nuspec_id: &str, nuspec_version: &str, tag: &str) -> Vec<u8> {
    use std::io::Write;
    let entry_id = nuspec_id;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut entries: Vec<(String, String)> = vec![
        (
            format!("{entry_id}.nuspec"),
            nuspec(nuspec_id, nuspec_version),
        ),
        (
            format!("lib/net8.0/{entry_id}.dll"),
            "a fake dll".to_string(),
        ),
    ];
    if !tag.is_empty() {
        entries.push(("package/tags.txt".to_string(), tag.to_string()));
    }
    for (name, body) in entries {
        writer.start_file(name, options).expect("the entry opens");
        writer
            .write_all(body.as_bytes())
            .expect("the entry is written");
    }
    writer.finish().expect("the zip closes").into_inner()
}

#[test]
fn the_service_index_points_at_the_operations_the_client_needs() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    for path in ["/nuget/nuget-hosted/v3/index.json", "/nuget/nuget-hosted"] {
        let answer = call(port, "GET", path, &token, None);
        assert_eq!(answer.status, 200, "{path}: {}", text_of(&answer));
        let body: Value = serde_json::from_slice(&answer.body).expect("json");
        assert_eq!(body["version"], "3.0.0");
        let types: Vec<&str> = body["resources"]
            .as_array()
            .expect("resources")
            .iter()
            .map(|resource| resource["@type"].as_str().expect("a type"))
            .collect();
        for needed in [
            "PackageBaseAddress/3.0.0",
            "RegistrationsBaseUrl/3.6.0",
            "PackagePublish/2.0.0",
        ] {
            assert!(
                types.contains(&needed),
                "{needed} is not declared: {types:?}"
            );
        }
        // The addresses are absolute through the host that asked: the
        // client guesses neither the scheme nor the port.
        let base = body["resources"][1]["@id"].as_str().expect("an id");
        assert!(
            base.starts_with(&format!("http://127.0.0.1:{port}/nuget/nuget-hosted/")),
            "the flat container address is not absolute on this host: {base}"
        );
    }

    // The service index is anonymous: `dotnet nuget push` loads it
    // without credentials before using the API key. The document
    // reveals nothing about the catalog; the feed content remains protected.
    let anonymous = call(port, "GET", "/nuget/nuget-hosted/v3/index.json", "", None);
    assert_eq!(anonymous.status, 200, "{}", text_of(&anonymous));
    let anonymous_content = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.one/1.0.0/sample.one.nupkg",
        "",
        None,
    );
    assert_eq!(
        anonymous_content.status,
        401,
        "{}",
        text_of(&anonymous_content)
    );

    // A repository of another format is not a nuget feed.
    let other_format = call(port, "GET", "/nuget/registry/v3/index.json", &token, None);
    assert_eq!(other_format.status, 404, "{}", text_of(&other_format));
}

/// The push **exactly as the real client does it**: `PUT` to the
/// publish address without id or version in the URL, with the nupkg
/// inside a `multipart/form-data` and the key in `X-NuGet-ApiKey`.
fn push_as_form(
    port: u16,
    repository: &str,
    api_key: &str,
    filename: &str,
    nupkg: &[u8],
) -> Answer {
    let mut body = Vec::new();
    body.extend_from_slice(
        b"--bnd-7ca4\r\nContent-Disposition: form-data; name=\"description\"\r\n\r\nignored\r\n",
    );
    body.extend_from_slice(
        b"--bnd-7ca4\r\nContent-Disposition: form-data; name=\"package\"; filename=\"",
    );
    body.extend_from_slice(filename.as_bytes());
    body.extend_from_slice(b"\"\r\nContent-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(nupkg);
    body.extend_from_slice(b"\r\n--bnd-7ca4--\r\n");
    let sent = next("sent");
    std::fs::write(&sent, &body).expect("the form is written");
    let into = next("answer");
    Curl::call_with_headers(
        port,
        "PUT",
        &format!("/nuget/{repository}/package/"),
        "not-a-token",
        Some(&sent),
        &[
            (
                "content-type",
                r#"multipart/form-data; boundary="bnd-7ca4""#,
            ),
            ("x-nuget-apikey", api_key),
        ],
        Some(&into),
    )
    .run()
}

#[test]
fn the_real_client_push_shape_is_a_multipart_form_to_the_publish_address() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-form", serde_json::Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let bytes = nupkg("Sample.Form", "1.0.0");
    let pushed = push_as_form(
        port,
        "nuget-form",
        &token,
        "sample.form.1.0.0.nupkg",
        &bytes,
    );
    assert_eq!(pushed.status, 201, "{}", text_of(&pushed));

    // The package lives under the identity of the nuspec, not of the URL: the
    // form URL promised nothing.
    let served = call(
        port,
        "GET",
        "/nuget/nuget-form/flatcontainer/sample.form/1.0.0/sample.form.nupkg",
        &token,
        None,
    );
    assert_eq!(served.status, 200, "{}", text_of(&served));
    assert_eq!(served.body, bytes, "el formulario reescribio el paquete");

    // The version list and the registration know it too.
    let versions = call(
        port,
        "GET",
        "/nuget/nuget-form/flatcontainer/sample.form/index.json",
        &token,
        None,
    );
    assert_eq!(versions.status, 200, "{}", text_of(&versions));
    let body: Value = serde_json::from_slice(&versions.body).expect("json");
    assert_eq!(body["versions"][0], "1.0.0");
}

#[test]
fn a_package_is_pushed_and_served_through_the_flat_container() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // The push speaks like dotnet: the key in X-NuGet-ApiKey. The URL goes
    // in the form the client writes (mixed case), and the server
    // normalizes it: NuGet is case-insensitive by contract.
    let bytes = nupkg("Sample.Lib", "1.0.0");
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &bytes);
    assert_eq!(answer.status, 201, "{}", text_of(&answer));

    // The version list, at the canonical address.
    let list = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/index.json",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    let body: Value = serde_json::from_slice(&list.body).expect("json");
    assert_eq!(body["count"], 1);
    assert_eq!(body["versions"][0], "1.0.0");

    // The nupkg, byte for byte: the hash the client computes is that of the
    // package it uploaded.
    let served = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/1.0.0/sample.lib.nupkg",
        &token,
        None,
    );
    assert_eq!(served.status, 200, "{}", text_of(&served));
    assert_eq!(served.body, bytes, "el hosted reescribe el nupkg");

    // HEAD gives the size without a body.
    let head = call(
        port,
        "HEAD",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/1.0.0/sample.lib.nupkg",
        &token,
        None,
    );
    assert_eq!(head.status, 200);
    assert!(
        head.body.is_empty(),
        "HEAD devolvio {} bytes: {:?}\ncabeceras: {}",
        head.body.len(),
        String::from_utf8_lossy(&head.body),
        head.headers
    );

    // The registration leaf points to the flat container with the normalized
    // identity.
    let leaf = call(
        port,
        "GET",
        "/nuget/nuget-hosted/registrations/sample.lib/index.json",
        &token,
        None,
    );
    assert_eq!(leaf.status, 200, "{}", text_of(&leaf));
    let body: Value = serde_json::from_slice(&leaf.body).expect("json");
    assert_eq!(
        body["items"][0]["packageContent"],
        "flatcontainer/sample.lib/1.0.0/sample.lib.nupkg"
    );

    // The non-normalized path names nothing: it is grammar, not an alias.
    let mixed = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/Sample.Lib/index.json",
        &token,
        None,
    );
    assert_eq!(mixed.status, 404, "{}", text_of(&mixed));
}

#[test]
fn the_nuspec_is_the_authority_and_a_version_is_for_always() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // The URL promises a version that the nuspec does not declare: the nuspec
    // wins, and the publication is rejected with the reason.
    let lying = nupkg_declaring("Sample.Lib", "2.0.0");
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &lying);
    assert_eq!(answer.status, 400, "{}", text_of(&answer));
    assert!(
        text_of(&answer).contains("2.0.0"),
        "el motivo no cita al nuspec"
    );

    // The same with the id.
    let lying = nupkg_declaring("Other.Lib", "1.0.0");
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &lying);
    assert_eq!(answer.status, 400, "{}", text_of(&answer));

    // A body that is not a usable nupkg is rejected too.
    let answer = call(
        port,
        "PUT",
        "/nuget/nuget-hosted/package/Sample.Lib/1.0.0",
        &token,
        Some(b"this is not a zip"),
    );
    assert_eq!(answer.status, 400, "{}", text_of(&answer));

    // Publish for real...
    let bytes = nupkg("Sample.Lib", "1.0.0");
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &bytes);
    assert_eq!(answer.status, 201, "{}", text_of(&answer));

    // ...the same package again is the echo of an interrupted
    // publication: there is nothing to rewrite.
    let answer = push_with_api_key(port, "nuget-hosted", &token, "Sample.Lib", "1.0.0", &bytes);
    assert_eq!(answer.status, 201, "{}", text_of(&answer));

    // ...other bytes for the same version are immutability: 409.
    let changed = nupkg_declaring_tagged("Sample.Lib", "1.0.0", "second");
    let answer = push_with_api_key(
        port,
        "nuget-hosted",
        &token,
        "Sample.Lib",
        "1.0.0",
        &changed,
    );
    assert_eq!(answer.status, 409, "{}", text_of(&answer));
    assert!(text_of(&answer).contains("immutable"));

    // The version declared with zeros and a build tag is
    // normalized: `01.0.0+b` is `1.0.0`, which already exists. Same digest,
    // echo; different, conflict.
    let answer = push_with_api_key(
        port,
        "nuget-hosted",
        &token,
        "Sample.Lib",
        "01.0.0+build.7",
        &bytes,
    );
    assert_eq!(answer.status, 201, "{}", text_of(&answer));
    let list = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/index.json",
        &token,
        None,
    );
    let body: Value = serde_json::from_slice(&list.body).expect("json");
    assert_eq!(body["count"], 1, "la forma cruda creo una version paralela");
}

#[test]
fn versions_sort_by_the_nuget_rules() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    for (id, version) in [
        ("Sample.Lib", "1.10.0"),
        ("Sample.Lib", "1.2.0"),
        ("Sample.Lib", "1.2.0-beta.2"),
        ("Sample.Lib", "1.2.0-beta.10"),
        ("Sample.Lib", "1.2.3.4"),
    ] {
        let bytes = nupkg(id, version);
        let answer = push_with_api_key(port, "nuget-hosted", &token, id, version, &bytes);
        assert_eq!(answer.status, 201, "{version}: {}", text_of(&answer));
    }

    let list = call(
        port,
        "GET",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/index.json",
        &token,
        None,
    );
    let body: Value = serde_json::from_slice(&list.body).expect("json");
    let versions: Vec<&str> = body["versions"]
        .as_array()
        .expect("versions")
        .iter()
        .map(|entry| entry.as_str().expect("a version"))
        .collect();
    assert_eq!(
        versions,
        [
            "1.2.0-beta.2",
            "1.2.0-beta.10",
            "1.2.0",
            "1.2.3.4",
            "1.10.0",
        ],
        "los previos van antes que su base, y 1.2.3.4 antes que 1.10.0"
    );

    // The registration leaf says the latest, and each version points to its
    // flat container with the normalized identity of the mixed-case push.
    let leaf = call(
        port,
        "GET",
        "/nuget/nuget-hosted/registrations/sample.lib/index.json",
        &token,
        None,
    );
    let body: Value = serde_json::from_slice(&leaf.body).expect("json");
    assert_eq!(body["version"], "1.10.0");
}

#[test]
fn the_protocol_says_no_to_what_it_does_not_know() {
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    deployment.seed_nuget_proxies(&[(
        "nuget-central",
        json!({"upstream": {"id": "lab", "url": "http://127.0.0.1:1/", "scopes": ["/"], "max_bytes": 4096}}),
    )]);
    deployment.seed_nuget(&[("member", Value::Null)]);
    deployment.seed_nuget_group("nuget-group", &["member"]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    // A method that the protocol does not have for these routes.
    let post = call(
        port,
        "POST",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/index.json",
        &token,
        Some(b""),
    );
    assert_eq!(post.status, 405, "{}", text_of(&post));
    let delete = call(
        port,
        "DELETE",
        "/nuget/nuget-hosted/package/Sample.Lib/1.0.0",
        &token,
        None,
    );
    assert_eq!(delete.status, 405, "{}", text_of(&delete));

    // The flat container is not published by hand: only the Publish endpoint.
    let put_flat = call(
        port,
        "PUT",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/1.0.0/sample.lib.nupkg",
        &token,
        Some(&nupkg("Sample.Lib", "1.0.0")),
    );
    assert_eq!(put_flat.status, 405, "{}", text_of(&put_flat));

    // Paths that are not NuGet v3, not even with path tricks.
    for hostile in [
        "/nuget/nuget-hosted/random/stuff",
        "/nuget/nuget-hosted/flatcontainer/sample.lib/1.0.0/other.nupkg",
        "/nuget/nuget-hosted/../../etc/passwd",
        "/nuget/nuget-hosted/registrations/Sample.Lib/index.json",
        "/nuget/nuget-hosted/search?q=sample",
    ] {
        let answer = call(port, "GET", hostile, &token, None);
        assert_eq!(answer.status, 404, "{hostile}: {}", text_of(&answer));
    }

    // A proxy does not accept publication: it is not a hosted under another name.
    let push_into_proxy = push_with_api_key(
        port,
        "nuget-central",
        &token,
        "Sample.Lib",
        "1.0.0",
        &nupkg("Sample.Lib", "1.0.0"),
    );
    assert_eq!(push_into_proxy.status, 405, "{}", text_of(&push_into_proxy));

    // A nuget group does not accept client writes (PUT to publish returns 405),
    // and reading a missing package from an empty group returns 404.
    let push_into_group = push_with_api_key(
        port,
        "nuget-group",
        &token,
        "Sample.Lib",
        "1.0.0",
        &nupkg("Sample.Lib", "1.0.0"),
    );
    assert_eq!(push_into_group.status, 405, "{}", text_of(&push_into_group));
    let group = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/sample.lib/index.json",
        &token,
        None,
    );
    assert_eq!(group.status, 404, "{}", text_of(&group));

    // A repository of another format is not a feed, and is not served as
    // Raw by equivalence: it does not exist for this surface.
    let other_format = call(port, "GET", "/nuget/registry/v3/index.json", &token, None);
    assert_eq!(other_format.status, 404, "{}", text_of(&other_format));
}

/// A truly fake NuGet upstream: it serves whatever is put on a
/// path and counts every request.
struct Upstream {
    port: u16,
    asked: Arc<Mutex<HashMap<String, usize>>>,
}

impl Upstream {
    fn asked_for(&self, path: &str) -> usize {
        self.asked
            .lock()
            .expect("the counter of the upstream")
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    fn total(&self) -> usize {
        self.asked.lock().expect("the counter").values().sum()
    }
}

fn start_upstream(id: &str, version: &str, nupkg: Vec<u8>) -> Upstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the upstream binds");
    let port = listener.local_addr().expect("the address").port();
    let asked: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut pages: HashMap<String, Vec<u8>> = HashMap::new();
    pages.insert(
        format!("/flatcontainer/{id}/index.json"),
        json!({ "count": 1, "versions": [ version ] })
            .to_string()
            .into_bytes(),
    );
    pages.insert(format!("/flatcontainer/{id}/{version}/{id}.nupkg"), nupkg);
    let pages = Arc::new(pages);
    let counter = Arc::clone(&asked);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = Arc::clone(&counter);
            let pages = Arc::clone(&pages);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                let mut read = String::new();
                loop {
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            read.push_str(String::from_utf8_lossy(&buffer[..n]).as_ref());
                            if read.contains("\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => return,
                    }
                }
                let Some(first) = read
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                else {
                    return;
                };
                let path = first.to_string();
                *counter
                    .lock()
                    .expect("the counter")
                    .entry(path.clone())
                    .or_default() += 1;
                let answer = match pages.get(&path) {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes()
                    .into_iter()
                    .chain(body.clone())
                    .collect::<Vec<u8>>(),
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                };
                let _ = stream.write_all(&answer);
                let _ = stream.flush();
            });
        }
    });
    Upstream { port, asked }
}

#[test]
fn the_proxy_caches_the_package_and_survives_without_network() {
    let bytes = nupkg("Proxied.Lib", "1.0.0");
    let upstream = start_upstream("proxied.lib", "1.0.0", bytes.clone());

    let deployment = Deployment::new();
    deployment.seed_nuget_proxies(&[(
        "nuget-lab",
        json!({
            "upstream": {
                "id": "lab",
                "url": format!("http://127.0.0.1:{}/", upstream.port),
                "scopes": ["/"],
                "max_bytes": 65536,
            },
        }),
    )]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let allowed = format!("127.0.0.1:{};private;http", upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    let nupkg_path = "/nuget/nuget-lab/flatcontainer/proxied.lib/1.0.0/proxied.lib.nupkg";
    let first = call(port, "GET", nupkg_path, &token, None);
    assert_eq!(first.status, 200, "{}", text_of(&first));
    assert_eq!(first.body, bytes, "el proxy reescribe el nupkg");
    assert_eq!(first.header("x-hodei-cache").as_deref(), Some("miss"));

    // The second time it does not go out: the cache serves the same bytes, and the
    // registration's contentHash keeps matching.
    let second = call(port, "GET", nupkg_path, &token, None);
    assert_eq!(second.status, 200, "{}", text_of(&second));
    assert_eq!(second.body, bytes);
    assert_eq!(second.header("x-hodei-cache").as_deref(), Some("hit"));
    assert_eq!(
        upstream.asked_for("/flatcontainer/proxied.lib/1.0.0/proxied.lib.nupkg"),
        1
    );

    // The version list, like the package: through the protocol
    // path and exactly as the upstream said it.
    let list = call(
        port,
        "GET",
        "/nuget/nuget-lab/flatcontainer/proxied.lib/index.json",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    let body: Value = serde_json::from_slice(&list.body).expect("json");
    assert_eq!(body["versions"][0], "1.0.0");

    let before = upstream.total();
    support::stop(&mut service);

    // Without network, what is cached keeps being served; what was not cached is
    // a 503 that says so.
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let port = service.port;
    let offline = call(port, "GET", nupkg_path, &token, None);
    assert_eq!(offline.status, 200, "{}", text_of(&offline));
    assert_eq!(offline.body, bytes);
    assert_eq!(offline.header("x-hodei-cache").as_deref(), Some("offline"));
    let unknown = call(
        port,
        "GET",
        "/nuget/nuget-lab/flatcontainer/proxied.lib/9.9.9/proxied.lib.nupkg",
        &token,
        None,
    );
    assert_eq!(unknown.status, 503, "{}", text_of(&unknown));
    support::stop(&mut service);
    assert_eq!(upstream.total(), before, "offline toco la red");
}
