//! HSP-231: NuGet v3 `RepositoryMode::Group` composition, precedence, and
//! protocol conformance (`dotnet restore` against hosted + `api.nuget.org`,
//! collision determinism, offline mode, negative cache, account scopes,
//! Cedar authz, and reserved namespace judgement).

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn require_dotnet() -> PathBuf {
    if let Some(found) = std::env::var_os("DOTNET")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return found;
    }
    if let Some(paths) = std::env::var_os("PATH")
        && let Some(found) = std::env::split_paths(&paths)
            .map(|dir| dir.join("dotnet"))
            .find(|candidate| candidate.exists())
    {
        return found;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).expect("HOME");
    let installed = home.join(".local/share/dotnet/dotnet");
    assert!(
        installed.exists(),
        "the .NET SDK is not installed: set DOTNET or put dotnet in PATH"
    );
    installed
}

fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn sha512_base64(bytes: &[u8]) -> String {
    let digest = Sha512::digest(bytes);
    base64::engine::general_purpose::STANDARD.encode(digest)
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

fn next_tmp(prefix: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("hsp231-{prefix}-{}-{seq}", std::process::id()))
}

fn call(port: u16, method: &str, path: &str, token: &str, body: Option<&[u8]>) -> Answer {
    let sent = body.map(|bytes| {
        let path = next_tmp("sent");
        std::fs::write(&path, bytes).expect("request body written");
        path
    });
    let into = next_tmp("answer");
    let answer = Curl::call(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        sent.as_ref().map(|_| "application/octet-stream"),
        Some(&into),
    )
    .run();
    if let Some(path) = sent {
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_file(&into);
    answer
}

fn push_with_api_key(
    port: u16,
    repo: &str,
    token: &str,
    id: &str,
    version: &str,
    body: &[u8],
) -> Answer {
    let sent = next_tmp("push");
    std::fs::write(&sent, body).expect("nupkg written");
    let into = next_tmp("push-answer");
    let answer = Curl::call_with_headers(
        port,
        "PUT",
        &format!("/nuget/{repo}/package/{id}/{version}"),
        "not-a-token",
        Some(&sent),
        &[
            ("content-type", "application/octet-stream"),
            ("x-nuget-apikey", token),
        ],
        Some(&into),
    )
    .run();
    let _ = std::fs::remove_file(&sent);
    let _ = std::fs::remove_file(&into);
    answer
}

fn nupkg(id: &str, version: &str, marker: &str) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(format!("{id}.nuspec"), options)
            .expect("nuspec entry");
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <package xmlns=\"http://schemas.microsoft.com/packaging/2013/05/nuspec.xsd\">\
               <metadata>\
                 <id>{id}</id>\
                 <version>{version}</version>\
                 <authors>{marker}</authors>\
                 <description>Test package from {marker}</description>\
               </metadata>\
             </package>"
        );
        zip.write_all(xml.as_bytes()).expect("nuspec xml");
        zip.start_file("marker.txt", options).expect("marker entry");
        zip.write_all(marker.as_bytes()).expect("marker bytes");
        zip.finish().expect("zip finishes");
    }
    buffer.into_inner()
}

fn glob_nupkg(directory: &Path) -> PathBuf {
    std::fs::read_dir(directory)
        .expect("release output exists")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("nupkg"))
        .expect("pack produced a .nupkg")
}

struct Client {
    dotnet: PathBuf,
    root: PathBuf,
}

impl Client {
    fn run(&self, work: &Path, args: &[&str]) -> std::process::Output {
        let output = Command::new(&self.dotnet)
            .current_dir(work)
            .env("HOME", &self.root)
            .env("DOTNET_CLI_HOME", &self.root)
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_NOLOGO", "1")
            .env("DOTNET_CLI_DO_NOT_USE_MSBUILD_SERVER", "1")
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("UseSharedCompilation", "false")
            .env("NUGET_PACKAGES", self.root.join("packages"))
            .args(args)
            .output()
            .expect("dotnet runs");
        assert!(
            output.status.success(),
            "dotnet {}: stdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
}

fn write_nuget_config(dir: &Path, port: u16, repo: &str, token: &str) {
    std::fs::write(
        dir.join("NuGet.config"),
        format!(
            r#"<configuration>
  <packageSources>
    <clear />
    <add key="hodei" value="http://127.0.0.1:{port}/nuget/{repo}/v3/index.json" allowInsecureConnections="true" />
  </packageSources>
  <packageSourceCredentials>
    <hodei>
      <add key="Username" value="hodei" />
      <add key="ClearTextPassword" value="{token}" />
    </hodei>
  </packageSourceCredentials>
</configuration>
"#
        ),
    )
    .expect("NuGet.config written");
}

struct MockUpstream {
    port: u16,
    asked: Arc<Mutex<HashMap<String, usize>>>,
}

impl MockUpstream {
    fn asked_for(&self, path: &str) -> usize {
        self.asked
            .lock()
            .expect("asked lock")
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    fn total(&self) -> usize {
        self.asked.lock().expect("asked lock").values().sum()
    }
}

fn start_mock_upstream(pages: HashMap<String, Vec<u8>>) -> MockUpstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("mock upstream binds");
    let port = listener.local_addr().expect("mock addr").port();
    let asked: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
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
                let Some(path) = read
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .map(str::to_string)
                else {
                    return;
                };
                *counter
                    .lock()
                    .expect("counter lock")
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

    MockUpstream { port, asked }
}

fn build_reg_doc(id: &str, items: Vec<(&str, &str)>) -> Vec<u8> {
    let entries: Vec<Value> = items
        .into_iter()
        .map(|(version, author)| {
            json!({
                "@id": format!("https://upstream.example/packages/{id}/{version}"),
                "packageContent": format!("https://upstream.example/flatcontainer/{id}/{version}/{id}.{version}.nupkg"),
                "catalogEntry": {
                    "id": id,
                    "version": version,
                    "authors": author,
                    "packageStatus": "validated"
                }
            })
        })
        .collect();
    json!({
        "@id": format!("https://upstream.example/registrations/{id}/index.json"),
        "items": entries
    })
    .to_string()
    .into_bytes()
}

#[test]
fn dotnet_restore_resolves_hosted_and_nuget_org_via_group_with_identical_hashes() {
    let dotnet = require_dotnet();
    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    deployment.seed_nuget_proxies(&[(
        "nuget-org",
        json!({
            "upstream": {
                "id": "nuget-org",
                "url": "https://api.nuget.org/v3/index.json",
                "scopes": ["/"],
                "max_bytes": 10_485_760
            }
        }),
    )]);
    deployment.seed_nuget_group("nuget-group", &["nuget-hosted", "nuget-org"]);

    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", "api.nuget.org")]);
    let port = service.port;

    // Group service index must only advertise the group's own URLs.
    let index = call(
        port,
        "GET",
        "/nuget/nuget-group/v3/index.json",
        &token,
        None,
    );
    assert_eq!(index.status, 200, "{}", text_of(&index));
    let index_text = text_of(&index);
    assert!(
        !index_text.contains("api.nuget.org"),
        "service index leaked upstream origin: {index_text}"
    );
    assert!(index_text.contains("/nuget/nuget-group/flatcontainer"));

    let root = deployment.directory.path().join("dotnet-group");
    std::fs::create_dir_all(&root).expect("client root");
    let client = Client { dotnet, root };

    // 1. Pack & push a hosted package into nuget-hosted.
    let proj = client.root.join("src");
    std::fs::create_dir_all(&proj).expect("src dir");
    client.run(&proj, &["new", "classlib", "-n", "GroupHostPkg", "--force"]);
    let csproj = proj.join("GroupHostPkg").join("GroupHostPkg.csproj");
    let edited = std::fs::read_to_string(&csproj)
        .expect("csproj")
        .replace(
            "</PropertyGroup>",
            "    <PackageId>Sample.Group.Hosted</PackageId>\n    <Version>1.0.0</Version>\n    <Authors>Hodei</Authors>\n  </PropertyGroup>",
        );
    std::fs::write(&csproj, edited).expect("csproj updated");
    client.run(
        &proj.join("GroupHostPkg"),
        &["pack", "-c", "Release", "-p:UseSharedCompilation=false"],
    );
    let hosted_nupkg = glob_nupkg(&proj.join("GroupHostPkg").join("bin/Release"));
    let hosted_bytes = std::fs::read(&hosted_nupkg).expect("packed nupkg");

    write_nuget_config(&proj, port, "nuget-hosted", &token);
    client.run(
        &proj,
        &[
            "nuget",
            "push",
            hosted_nupkg.to_str().expect("nupkg path"),
            "--source",
            "hodei",
            "--api-key",
            &token,
        ],
    );

    // 2. Restore a consumer project referencing BOTH the hosted package and Newtonsoft.Json from api.nuget.org.
    let consumer = client.root.join("consumer");
    std::fs::create_dir_all(&consumer).expect("consumer dir");
    client.run(&consumer, &["new", "console", "-n", "Consumer", "--force"]);
    let consumer_dir = consumer.join("Consumer");
    let consumer_csproj = consumer_dir.join("Consumer.csproj");
    let edited = std::fs::read_to_string(&consumer_csproj)
        .expect("consumer csproj")
        .replace(
            "</Project>",
            "  <ItemGroup>\n    <PackageReference Include=\"Sample.Group.Hosted\" Version=\"1.0.0\" />\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.3\" />\n  </ItemGroup>\n</Project>",
        );
    std::fs::write(&consumer_csproj, edited).expect("consumer csproj updated");
    write_nuget_config(&consumer_dir, port, "nuget-group", &token);
    client.run(&consumer_dir, &["restore"]);
    client.run(&consumer_dir, &["build", "--no-restore"]);

    // 3. Verify hosted package bytes & sha512 in NUGET_PACKAGES.
    let hosted_cached = client
        .root
        .join("packages/sample.group.hosted/1.0.0/sample.group.hosted.1.0.0.nupkg");
    assert!(hosted_cached.exists(), "hosted package not restored");
    let hosted_cached_bytes = std::fs::read(&hosted_cached).expect("hosted cached bytes");
    assert_eq!(sha256_hex(&hosted_cached_bytes), sha256_hex(&hosted_bytes));
    let hosted_sha512 = std::fs::read_to_string(
        client
            .root
            .join("packages/sample.group.hosted/1.0.0/sample.group.hosted.1.0.0.nupkg.sha512"),
    )
    .expect("hosted sha512");
    assert_eq!(hosted_sha512.trim(), sha512_base64(&hosted_bytes));

    // 4. Verify Newtonsoft.Json 13.0.3 bytes & contentHash (.sha512) match the group response.
    let remote_cached = client
        .root
        .join("packages/newtonsoft.json/13.0.3/newtonsoft.json.13.0.3.nupkg");
    assert!(remote_cached.exists(), "nuget.org package not restored");
    let remote_cached_bytes = std::fs::read(&remote_cached).expect("remote cached bytes");
    let group_remote = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/newtonsoft.json/13.0.3/newtonsoft.json.13.0.3.nupkg",
        &token,
        None,
    );
    assert_eq!(group_remote.status, 200, "{}", text_of(&group_remote));
    assert_eq!(
        sha256_hex(&remote_cached_bytes),
        sha256_hex(&group_remote.body)
    );
    let remote_sha512 = std::fs::read_to_string(
        client
            .root
            .join("packages/newtonsoft.json/13.0.3/newtonsoft.json.13.0.3.nupkg.sha512"),
    )
    .expect("remote sha512");
    assert_eq!(remote_sha512.trim(), sha512_base64(&group_remote.body));
}

#[test]
fn collision_across_members_is_deterministic_and_never_mixes_metadata_and_bytes() {
    let hosted_100 = nupkg("Acme.Lib", "1.0.0", "hosted");
    let proxy_a_100 = nupkg("Acme.Lib", "1.0.0", "proxy-a");
    let proxy_a_150 = nupkg("Acme.Lib", "1.5.0", "proxy-a");
    let proxy_b_150 = nupkg("Acme.Lib", "1.5.0", "proxy-b");
    let proxy_b_200 = nupkg("Acme.Lib", "2.0.0", "proxy-b");

    let mut pages_a = HashMap::new();
    pages_a.insert(
        "/flatcontainer/acme.lib/index.json".to_string(),
        json!({ "versions": ["1.0.0", "1.5.0"] })
            .to_string()
            .into_bytes(),
    );
    pages_a.insert(
        "/flatcontainer/acme.lib/1.0.0/acme.lib.1.0.0.nupkg".to_string(),
        proxy_a_100,
    );
    pages_a.insert(
        "/flatcontainer/acme.lib/1.0.0/acme.lib.1.0.0.nupkg.sha512".to_string(),
        b"SHA512-PROXY-A-100".to_vec(),
    );
    pages_a.insert(
        "/flatcontainer/acme.lib/1.5.0/acme.lib.1.5.0.nupkg".to_string(),
        proxy_a_150.clone(),
    );
    pages_a.insert(
        "/flatcontainer/acme.lib/1.5.0/acme.lib.1.5.0.nupkg.sha512".to_string(),
        b"SHA512-PROXY-A-150".to_vec(),
    );
    pages_a.insert(
        "/registrations/acme.lib/index.json".to_string(),
        build_reg_doc("acme.lib", vec![("1.0.0", "ProxyA"), ("1.5.0", "ProxyA")]),
    );
    let upstream_a = start_mock_upstream(pages_a);

    let mut pages_b = HashMap::new();
    pages_b.insert(
        "/flatcontainer/acme.lib/index.json".to_string(),
        json!({ "versions": ["1.5.0", "2.0.0"] })
            .to_string()
            .into_bytes(),
    );
    pages_b.insert(
        "/flatcontainer/acme.lib/1.5.0/acme.lib.1.5.0.nupkg".to_string(),
        proxy_b_150,
    );
    pages_b.insert(
        "/flatcontainer/acme.lib/1.5.0/acme.lib.1.5.0.nupkg.sha512".to_string(),
        b"SHA512-PROXY-B-150".to_vec(),
    );
    pages_b.insert(
        "/flatcontainer/acme.lib/2.0.0/acme.lib.2.0.0.nupkg".to_string(),
        proxy_b_200.clone(),
    );
    pages_b.insert(
        "/flatcontainer/acme.lib/2.0.0/acme.lib.2.0.0.nupkg.sha512".to_string(),
        b"SHA512-PROXY-B-200".to_vec(),
    );
    pages_b.insert(
        "/registrations/acme.lib/index.json".to_string(),
        build_reg_doc("acme.lib", vec![("1.5.0", "ProxyB"), ("2.0.0", "ProxyB")]),
    );
    let upstream_b = start_mock_upstream(pages_b);

    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    deployment.seed_nuget_proxies(&[
        (
            "proxy-a",
            json!({
                "upstream": {
                    "id": "up-a",
                    "url": format!("http://127.0.0.1:{}/", upstream_a.port),
                    "scopes": ["/"],
                    "max_bytes": 1_048_576
                }
            }),
        ),
        (
            "proxy-b",
            json!({
                "upstream": {
                    "id": "up-b",
                    "url": format!("http://127.0.0.1:{}/", upstream_b.port),
                    "scopes": ["/"],
                    "max_bytes": 1_048_576
                }
            }),
        ),
    ]);
    deployment.seed_nuget_group("g-ab", &["nuget-hosted", "proxy-a", "proxy-b"]);
    deployment.seed_nuget_group("g-ba", &["proxy-b", "proxy-a"]);

    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let allowed = format!(
        "127.0.0.1:{};private;http,127.0.0.1:{};private;http",
        upstream_a.port, upstream_b.port
    );
    let service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    let push = push_with_api_key(
        port,
        "nuget-hosted",
        &token,
        "Acme.Lib",
        "1.0.0",
        &hosted_100,
    );
    assert_eq!(push.status, 201, "{}", text_of(&push));

    // Union of versions in NuGet sort order.
    let list = call(
        port,
        "GET",
        "/nuget/g-ab/flatcontainer/acme.lib/index.json",
        &token,
        None,
    );
    assert_eq!(list.status, 200, "{}", text_of(&list));
    let list_json: Value = serde_json::from_slice(&list.body).expect("versions json");
    assert_eq!(list_json["versions"], json!(["1.0.0", "1.5.0", "2.0.0"]));

    // 1.0.0 won by hosted: nupkg is hosted_100, and .sha512 does NOT fall back to proxy-a.
    let pkg_100 = call(
        port,
        "GET",
        "/nuget/g-ab/flatcontainer/acme.lib/1.0.0/acme.lib.1.0.0.nupkg",
        &token,
        None,
    );
    assert_eq!(pkg_100.status, 200, "{}", text_of(&pkg_100));
    assert_eq!(pkg_100.body, hosted_100);
    let sha_100 = call(
        port,
        "GET",
        "/nuget/g-ab/flatcontainer/acme.lib/1.0.0/acme.lib.1.0.0.nupkg.sha512",
        &token,
        None,
    );
    assert_eq!(sha_100.status, 404, "{}", text_of(&sha_100));

    // 1.5.0 won by proxy-a in g-ab: both nupkg and .sha512 come from proxy-a.
    let pkg_150 = call(
        port,
        "GET",
        "/nuget/g-ab/flatcontainer/acme.lib/1.5.0/acme.lib.1.5.0.nupkg",
        &token,
        None,
    );
    assert_eq!(pkg_150.status, 200, "{}", text_of(&pkg_150));
    assert_eq!(pkg_150.body, proxy_a_150);
    let sha_150 = call(
        port,
        "GET",
        "/nuget/g-ab/flatcontainer/acme.lib/1.5.0/acme.lib.1.5.0.nupkg.sha512",
        &token,
        None,
    );
    assert_eq!(sha_150.status, 200, "{}", text_of(&sha_150));
    assert_eq!(sha_150.body, b"SHA512-PROXY-A-150");

    // 2.0.0 won by proxy-b.
    let pkg_200 = call(
        port,
        "GET",
        "/nuget/g-ab/flatcontainer/acme.lib/2.0.0/acme.lib.2.0.0.nupkg",
        &token,
        None,
    );
    assert_eq!(pkg_200.status, 200, "{}", text_of(&pkg_200));
    assert_eq!(pkg_200.body, proxy_b_200);

    // Registration leaf reflects the exact winning origin per version and never leaks upstream URLs.
    let reg = call(
        port,
        "GET",
        "/nuget/g-ab/registrations/acme.lib/index.json",
        &token,
        None,
    );
    assert_eq!(reg.status, 200, "{}", text_of(&reg));
    let reg_text = text_of(&reg);
    assert!(
        !reg_text.contains("upstream.example"),
        "registration leaked upstream origin: {reg_text}"
    );
    let reg_json: Value = serde_json::from_slice(&reg.body).expect("reg json");
    let items = reg_json["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["catalogEntry"]["version"], "1.0.0");
    assert_eq!(items[0]["catalogEntry"]["packageStatus"], "validated");
    assert_eq!(items[1]["catalogEntry"]["version"], "1.5.0");
    assert_eq!(items[1]["catalogEntry"]["authors"], "ProxyA");
    assert_eq!(items[2]["catalogEntry"]["version"], "2.0.0");
    assert_eq!(items[2]["catalogEntry"]["authors"], "ProxyB");

    // Reversed precedence g-ba picks proxy-b for 1.5.0.
    let sha_150_ba = call(
        port,
        "GET",
        "/nuget/g-ba/flatcontainer/acme.lib/1.5.0/acme.lib.1.5.0.nupkg.sha512",
        &token,
        None,
    );
    assert_eq!(sha_150_ba.status, 200, "{}", text_of(&sha_150_ba));
    assert_eq!(sha_150_ba.body, b"SHA512-PROXY-B-150");
}

#[test]
fn offline_negative_cache_account_scopes_authz_and_reserved_namespace_hold() {
    let remote_100 = nupkg("Public.Pkg", "1.0.0", "public");
    let internal_proxy = nupkg("Internal.Corp", "1.0.0", "public-spoof");
    let internal_hosted = nupkg("Internal.Corp", "1.0.0", "hosted-real");

    let mut pages = HashMap::new();
    pages.insert(
        "/flatcontainer/public.pkg/index.json".to_string(),
        json!({ "versions": ["1.0.0"] }).to_string().into_bytes(),
    );
    pages.insert(
        "/flatcontainer/public.pkg/1.0.0/public.pkg.1.0.0.nupkg".to_string(),
        remote_100.clone(),
    );
    pages.insert(
        "/flatcontainer/internal.corp/index.json".to_string(),
        json!({ "versions": ["1.0.0"] }).to_string().into_bytes(),
    );
    pages.insert(
        "/flatcontainer/internal.corp/1.0.0/internal.corp.1.0.0.nupkg".to_string(),
        internal_proxy,
    );
    let upstream = start_mock_upstream(pages);

    let deployment = Deployment::new();
    deployment.seed_nuget(&[("nuget-hosted", Value::Null)]);
    deployment.seed_nuget_proxies(&[(
        "nuget-proxy",
        json!({
            "upstream": {
                "id": "up",
                "url": format!("http://127.0.0.1:{}/", upstream.port),
                "scopes": ["/"],
                "max_bytes": 1_048_576
            },
            "cache": {
                "negative_secs": 60
            }
        }),
    )]);
    deployment.seed_groups_raw(&[json!({
        "key": "nuget-group",
        "format": "nuget",
        "members": ["nuget-hosted", "nuget-proxy"],
        "namespaces": {
            "reserved": ["Internal.Corp"]
        }
    })]);

    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let other_account_token = support::token(support::SECOND_ACCOUNT, "bob");
    let allowed = format!("127.0.0.1:{};private;http", upstream.port);
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let port = service.port;

    // 1. Account scope: second account cannot see first account's group.
    let cross_account = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/public.pkg/index.json",
        &other_account_token,
        None,
    );
    assert_eq!(cross_account.status, 404, "{}", text_of(&cross_account));

    // 2. Reserved namespace: before hosted has internal.corp, group blocks proxy and returns 403.
    let reserved_missing = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/internal.corp/index.json",
        &token,
        None,
    );
    assert_eq!(
        reserved_missing.status,
        403,
        "{}",
        text_of(&reserved_missing)
    );
    assert_eq!(
        upstream.asked_for("/flatcontainer/internal.corp/index.json"),
        0,
        "reserved namespace must never contact public proxy upstream"
    );

    // Once hosted publishes internal.corp, the group serves it from hosted without touching proxy.
    let push_internal = push_with_api_key(
        port,
        "nuget-hosted",
        &token,
        "Internal.Corp",
        "1.0.0",
        &internal_hosted,
    );
    assert_eq!(push_internal.status, 201, "{}", text_of(&push_internal));
    let reserved_hosted = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/internal.corp/1.0.0/internal.corp.1.0.0.nupkg",
        &token,
        None,
    );
    assert_eq!(reserved_hosted.status, 200, "{}", text_of(&reserved_hosted));
    assert_eq!(reserved_hosted.body, internal_hosted);
    assert_eq!(
        upstream.asked_for("/flatcontainer/internal.corp/index.json"),
        0
    );

    // 3. Negative cache: missing.pkg hits upstream only once within negative_secs.
    let miss1 = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/missing.pkg/index.json",
        &token,
        None,
    );
    assert_eq!(miss1.status, 404, "{}", text_of(&miss1));
    let miss2 = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/missing.pkg/index.json",
        &token,
        None,
    );
    assert_eq!(miss2.status, 404, "{}", text_of(&miss2));
    assert_eq!(
        upstream.asked_for("/flatcontainer/missing.pkg/index.json"),
        1,
        "negative cache should prevent second upstream hit"
    );

    // Warm public.pkg into proxy cache before switching to offline.
    let warm = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/public.pkg/1.0.0/public.pkg.1.0.0.nupkg",
        &token,
        None,
    );
    assert_eq!(warm.status, 200, "{}", text_of(&warm));
    assert_eq!(warm.body, remote_100);
    let calls_before_offline = upstream.total();
    support::stop(&mut service);

    // 4. Offline mode: cached proxy + hosted still work; uncached returns 503.
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str()),
        ("HODEI_PROXY__OFFLINE", "true"),
    ]);
    let port = service.port;

    let cached_offline = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/public.pkg/1.0.0/public.pkg.1.0.0.nupkg",
        &token,
        None,
    );
    assert_eq!(cached_offline.status, 200, "{}", text_of(&cached_offline));
    assert_eq!(cached_offline.body, remote_100);

    let uncached_offline = call(
        port,
        "GET",
        "/nuget/nuget-group/flatcontainer/never.cached/index.json",
        &token,
        None,
    );
    assert_eq!(
        uncached_offline.status,
        503,
        "{}",
        text_of(&uncached_offline)
    );
    assert_eq!(upstream.total(), calls_before_offline);
    support::stop(&mut service);

    // 5. Cedar authz: denying download_asset denies reading from the NuGet group.
    deployment.write_policy("");
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())]);
    let denied = call(
        service.port,
        "GET",
        "/nuget/nuget-group/flatcontainer/internal.corp/index.json",
        &token,
        None,
    );
    assert_eq!(denied.status, 403, "{}", text_of(&denied));
    support::stop(&mut service);
}
