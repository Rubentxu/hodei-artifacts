//! Maven 2 proxy y group de HSP-110 / HAR-16, contra el wire.
//!
//! El servicio es un proceso de verdad con catalogo, almacen de bytes y
//! Cedar reales; los upstreams son servidores locales que este fichero
//! controla. Lo que se mide es lo que ve un cliente Maven: que lo bajado de
//! un "Central" de laboratorio queda en cache y se resuelve despues sin red
//! a traves de un group, que los metadatos de snapshot se revalidan y los
//! ficheros inmutables no se sobrescriben, y que el group nunca mezcla un
//! fichero de un miembro con el checksum de otro.

mod support;

use sha1::{Digest, Sha1};
use support::lab::Upstream;
use support::{Curl, Deployment, FIRST_ACCOUNT};

const HOSTED: &str = "mvn-hosted";

const POM_PATH: &str = "org/up/lib/1.0/lib-1.0.pom";
const JAR_PATH: &str = "org/up/lib/1.0/lib-1.0.jar";
const METADATA_PATH: &str = "org/up/lib/maven-metadata.xml";

const JAR: &[u8] = b"jar bytes served by the upstream";
const OTHER_JAR: &[u8] = b"jar bytes that the other member has for the same coordinates";

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn pom(group: &str, artifact: &str, version: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- kept as the upstream wrote it -->\n<project>\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>{group}</groupId>\n  <artifactId>{artifact}</artifactId>\n  <version>{version}</version>\n</project>\n"
    )
    .into_bytes()
}

fn metadata(versions: &[&str]) -> Vec<u8> {
    let list: String = versions
        .iter()
        .map(|version| format!("      <version>{version}</version>\n"))
        .collect();
    let last = versions.last().copied().unwrap_or("");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<metadata>\n  <groupId>org.up</groupId>\n  <artifactId>lib</artifactId>\n  <versioning>\n    <latest>{last}</latest>\n    <release>{last}</release>\n    <versions>\n{list}    </versions>\n    <lastUpdated>20240101000000</lastUpdated>\n  </versioning>\n</metadata>\n"
    )
    .into_bytes()
}

fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Un repositorio proxy del laboratorio.
struct Proxy {
    key: &'static str,
    /// Que upstream usa: 0 es el primero, 1 el segundo.
    upstream: usize,
    cache: serde_json::Value,
    maven: serde_json::Value,
    credential_ref: Option<&'static str>,
}

impl Proxy {
    fn new(key: &'static str, upstream: usize) -> Self {
        Self {
            key,
            upstream,
            cache: serde_json::json!({ "ttl": { "metadata_secs": 3600 } }),
            maven: serde_json::Value::Null,
            credential_ref: None,
        }
    }

    fn cache(mut self, cache: serde_json::Value) -> Self {
        self.cache = cache;
        self
    }

    fn maven(mut self, maven: serde_json::Value) -> Self {
        self.maven = maven;
        self
    }

    fn credential(mut self, reference: &'static str) -> Self {
        self.credential_ref = Some(reference);
        self
    }
}

struct Lab {
    deployment: Deployment,
    upstreams: [Upstream; 2],
}

impl Lab {
    fn new(proxies: Vec<Proxy>, groups: &[(&str, &[&str])]) -> Self {
        let upstreams = [Upstream::start(), Upstream::start()];
        let deployment = Deployment::new();
        deployment.seed_maven(&[(HOSTED, serde_json::Value::Null)]);
        let seeded: Vec<(&str, serde_json::Value, serde_json::Value)> = proxies
            .iter()
            .map(|proxy| {
                let mut upstream = serde_json::json!({
                    "id": format!("lab-{}", proxy.upstream),
                    "url": format!("http://127.0.0.1:{}/", upstreams[proxy.upstream].port()),
                    "scopes": ["/"],
                    "max_bytes": 4096,
                });
                if let Some(reference) = proxy.credential_ref {
                    upstream["credential_ref"] = reference.into();
                }
                (
                    proxy.key,
                    proxy.maven.clone(),
                    serde_json::json!({ "upstream": upstream, "cache": proxy.cache }),
                )
            })
            .collect();
        if !seeded.is_empty() {
            deployment.seed_maven_proxies(&seeded);
        }
        deployment.seed_maven_groups(groups);
        Self {
            deployment,
            upstreams,
        }
    }

    fn start_with(&self, extra: &[(&str, &str)]) -> support::Service {
        let allowed = self
            .upstreams
            .iter()
            .map(|upstream| format!("127.0.0.1:{};private;http", upstream.port()))
            .collect::<Vec<_>>()
            .join(",");
        let mut environment = vec![("HODEI_PROXY__ALLOWED_HOSTS", allowed.as_str())];
        environment.extend_from_slice(extra);
        self.deployment.start(&environment)
    }

    fn start(&self) -> support::Service {
        self.start_with(&[])
    }

    fn file(&self, name: &str, body: &[u8]) -> std::path::PathBuf {
        let path = self.deployment.directory.path().join(name);
        std::fs::write(&path, body).expect("the body is written");
        path
    }

    /// Publica en el hosted y exige un 2xx.
    fn publish(&self, port: u16, path: &str, body: &[u8]) {
        let alice = support::token(FIRST_ACCOUNT, "alice");
        let file = self.file("upload.bin", body);
        let status = Curl::call(
            port,
            "PUT",
            &format!("/repository/{HOSTED}/{path}"),
            &alice,
            Some(&file),
            None,
            None,
        )
        .run()
        .status;
        assert!((200..300).contains(&status), "publishing {path}: {status}");
    }
}

/// Una respuesta: codigo, cuerpo y las cabeceras que dicen quien sirvio.
struct Reply {
    status: u16,
    body: Vec<u8>,
    resolved_from: Option<String>,
    group_version: Option<String>,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn get(lab: &Lab, port: u16, repository: &str, path: &str) -> Reply {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let into = lab.deployment.directory.path().join("response.bin");
    let _ = std::fs::remove_file(&into);
    let answer = Curl::call(
        port,
        "GET",
        &format!("/repository/{repository}/{path}"),
        &alice,
        None,
        None,
        Some(&into),
    )
    .run();
    Reply {
        status: answer.status,
        body: std::fs::read(&into).unwrap_or_default(),
        resolved_from: answer.header("x-hodei-resolved-from"),
        group_version: answer.header("x-hodei-group-version"),
    }
}

/// El codigo que responde un metodo de escritura (sin cuerpo).
fn status_of(lab: &Lab, port: u16, method: &str, repository: &str, path: &str) -> u16 {
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let file = lab.file("upload.bin", b"payload");
    Curl::call(
        port,
        method,
        &format!("/repository/{repository}/{path}"),
        &alice,
        Some(&file),
        None,
        None,
    )
    .run()
    .status
}

// ---------------------------------------------------------------------------
// AC1: Central se cachea por el proxy y despues se resuelve sin red.
// ---------------------------------------------------------------------------

#[test]
fn central_is_cached_by_the_proxy_and_then_resolved_offline_from_the_group() {
    // TTL 0: sin el modo offline, cada lectura preguntaria al upstream. Que
    // no lo haga es merito del modo offline, no de que la copia siga fresca.
    let cache = serde_json::json!({ "ttl": { "metadata_secs": 0, "content_secs": 0 } });
    let lab = Lab::new(
        vec![Proxy::new("central", 0).cache(cache)],
        &[("mvn-all", &[HOSTED, "central"])],
    );
    let up_pom = pom("org.up", "lib", "1.0");
    lab.upstreams[0].put(POM_PATH, &up_pom, None);
    lab.upstreams[0].put(JAR_PATH, JAR, None);
    lab.upstreams[0].put(METADATA_PATH, &metadata(&["1.0", "1.1"]), None);

    let mut service = lab.start();
    let port = service.port;
    // Lo propio del equipo convive con lo de Central en el mismo group.
    lab.publish(
        port,
        "org/up/lib/1.2/lib-1.2.pom",
        &pom("org.up", "lib", "1.2"),
    );
    lab.publish(port, "org/up/lib/1.2/lib-1.2.jar", b"our own build");

    let online = [
        get(&lab, port, "mvn-all", POM_PATH),
        get(&lab, port, "mvn-all", JAR_PATH),
        get(&lab, port, "mvn-all", &format!("{JAR_PATH}.sha1")),
        get(&lab, port, "mvn-all", METADATA_PATH),
        get(&lab, port, "mvn-all", &format!("{METADATA_PATH}.sha1")),
    ];
    for reply in &online {
        assert_eq!(reply.status, 200, "{}", reply.text());
    }
    // El POM llega byte a byte como lo escribio Central: ni se reescribe ni
    // se normaliza.
    assert_eq!(online[0].body, up_pom);
    assert_eq!(online[0].resolved_from.as_deref(), Some("central"));
    assert_eq!(online[1].body, JAR);
    // El checksum es el del fichero servido, aunque Central no tenga sidecar.
    assert_eq!(online[2].text(), sha1_hex(JAR));
    assert_eq!(lab.upstreams[0].hits(&format!("{JAR_PATH}.sha1")), 0);
    // Los metadatos unen lo de Central y lo publicado en el hosted.
    let merged = online[3].text();
    for version in ["1.0", "1.1", "1.2"] {
        assert!(
            merged.contains(&format!("<version>{version}</version>")),
            "{merged}"
        );
    }
    assert!(merged.contains("<latest>1.2</latest>"), "{merged}");
    assert_eq!(online[4].text(), sha1_hex(&online[3].body));
    // Los metadatos unidos dicen que miembros aportaron y la version del
    // group con la que se calcularon.
    let from = online[3].resolved_from.clone().unwrap_or_default();
    assert!(from.contains(HOSTED) && from.contains("central"), "{from}");
    assert!(online[3].group_version.is_some());
    // Un group es de lectura: escribir en el o en un proxy es un 405.
    assert_eq!(status_of(&lab, port, "PUT", "mvn-all", JAR_PATH), 405);
    assert_eq!(status_of(&lab, port, "PUT", "central", JAR_PATH), 405);
    support::stop(&mut service);

    // Sin red: el upstream no recibe ni una peticion mas y el group resuelve
    // lo mismo desde lo cacheado.
    let before = lab.upstreams[0].total();
    lab.upstreams[0].fail(true);
    let mut offline = lab.start_with(&[("HODEI_PROXY__OFFLINE", "true")]);
    let port = offline.port;
    for (path, expected) in [(POM_PATH, &up_pom[..]), (JAR_PATH, JAR)] {
        let again = get(&lab, port, "mvn-all", path);
        assert_eq!(again.status, 200, "{}", again.text());
        assert_eq!(again.body, expected);
    }
    let again = get(&lab, port, "mvn-all", METADATA_PATH);
    assert_eq!(again.status, 200, "{}", again.text());
    assert_eq!(again.body, online[3].body);
    assert_eq!(
        lab.upstreams[0].total(),
        before,
        "offline touched the network"
    );
    support::stop(&mut offline);
}

// ---------------------------------------------------------------------------
// AC2: snapshots se revalidan por TTL; las releases no se sobrescriben.
// ---------------------------------------------------------------------------

const SNAPSHOT_DIR: &str = "org/up/lib/1.0-SNAPSHOT";
const FLOATING: &str = "org/up/lib/1.0-SNAPSHOT/lib-1.0-SNAPSHOT.jar";
const TIMESTAMPED: &str = "org/up/lib/1.0-SNAPSHOT/lib-1.0-20240101.120000-1.jar";

#[test]
fn a_floating_snapshot_and_its_metadata_are_revalidated_when_the_ttl_expires() {
    let cache = serde_json::json!({ "ttl": { "metadata_secs": 0, "content_secs": 0 } });
    let lab = Lab::new(vec![Proxy::new("pxy", 0).cache(cache)], &[]);
    let metadata_path = format!("{SNAPSHOT_DIR}/maven-metadata.xml");
    lab.upstreams[0].put(FLOATING, b"snapshot build one", Some("\"s1\""));
    lab.upstreams[0].put(&metadata_path, b"<metadata>one</metadata>", Some("\"m1\""));
    let mut service = lab.start();
    let port = service.port;

    assert_eq!(get(&lab, port, "pxy", FLOATING).body, b"snapshot build one");
    assert_eq!(
        get(&lab, port, "pxy", &metadata_path).body,
        b"<metadata>one</metadata>"
    );

    // El upstream publica otro snapshot con el mismo nombre: el proxy lo
    // revalida y sirve lo nuevo en vez de retenerlo como "otros bytes".
    lab.upstreams[0].put(FLOATING, b"snapshot build two", Some("\"s2\""));
    lab.upstreams[0].put(&metadata_path, b"<metadata>two</metadata>", Some("\"m2\""));
    let floating = get(&lab, port, "pxy", FLOATING);
    assert_eq!(floating.status, 200, "{}", floating.text());
    assert_eq!(floating.body, b"snapshot build two");
    assert_eq!(
        get(&lab, port, "pxy", &metadata_path).body,
        b"<metadata>two</metadata>"
    );
    assert_eq!(lab.upstreams[0].hits(FLOATING), 2);
    support::stop(&mut service);
}

#[test]
fn an_expired_snapshot_is_revalidated_with_its_etag_and_a_304_keeps_the_copy() {
    let cache = serde_json::json!({ "ttl": { "metadata_secs": 0, "content_secs": 0 } });
    let lab = Lab::new(vec![Proxy::new("pxy", 0).cache(cache)], &[]);
    let metadata_path = format!("{SNAPSHOT_DIR}/maven-metadata.xml");
    lab.upstreams[0].put(FLOATING, b"snapshot build", Some("\"s1\""));
    lab.upstreams[0].put(&metadata_path, b"<metadata>same</metadata>", Some("\"m1\""));
    let mut service = lab.start();
    let port = service.port;
    for _ in 0..2 {
        let floating = get(&lab, port, "pxy", FLOATING);
        assert_eq!(floating.status, 200, "{}", floating.text());
        assert_eq!(floating.body, b"snapshot build");
        let meta = get(&lab, port, "pxy", &metadata_path);
        assert_eq!(meta.status, 200, "{}", meta.text());
        assert_eq!(meta.body, b"<metadata>same</metadata>");
    }
    // La segunda vez se pregunto (TTL vencido) y con el validador.
    for (path, etag) in [(FLOATING, "\"s1\""), (metadata_path.as_str(), "\"m1\"")] {
        assert_eq!(lab.upstreams[0].hits(path), 2, "{path}");
        let asked: Vec<_> = lab.upstreams[0]
            .requests()
            .into_iter()
            .filter(|seen| seen.path == path)
            .collect();
        assert_eq!(asked[0].if_none_match, None);
        assert_eq!(asked[1].if_none_match.as_deref(), Some(etag), "{path}");
    }
    support::stop(&mut service);
}

#[test]
fn snapshot_metadata_inside_its_ttl_does_not_ask_the_upstream_again() {
    let cache = serde_json::json!({ "ttl": { "metadata_secs": 3600 } });
    let lab = Lab::new(vec![Proxy::new("pxy", 0).cache(cache)], &[]);
    let metadata_path = format!("{SNAPSHOT_DIR}/maven-metadata.xml");
    lab.upstreams[0].put(&metadata_path, b"<metadata>one</metadata>", None);
    lab.upstreams[0].put(FLOATING, b"snapshot build one", None);
    let mut service = lab.start();
    let port = service.port;
    for _ in 0..3 {
        assert_eq!(get(&lab, port, "pxy", &metadata_path).status, 200);
        assert_eq!(get(&lab, port, "pxy", FLOATING).status, 200);
    }
    assert_eq!(lab.upstreams[0].hits(&metadata_path), 1);
    assert_eq!(lab.upstreams[0].hits(FLOATING), 1);
    support::stop(&mut service);
}

#[test]
fn a_release_and_a_timestamped_snapshot_are_never_silently_overwritten() {
    let cache = serde_json::json!({ "ttl": { "metadata_secs": 0, "content_secs": 0 } });
    let lab = Lab::new(vec![Proxy::new("pxy", 0).cache(cache)], &[]);
    lab.upstreams[0].put(JAR_PATH, b"release one", Some("\"r1\""));
    lab.upstreams[0].put(TIMESTAMPED, b"timestamped one", Some("\"t1\""));
    let mut service = lab.start();
    let port = service.port;
    assert_eq!(get(&lab, port, "pxy", JAR_PATH).body, b"release one");
    assert_eq!(get(&lab, port, "pxy", TIMESTAMPED).body, b"timestamped one");

    // El upstream cambia los bytes de lo que es inmutable: se retiene.
    lab.upstreams[0].put(JAR_PATH, b"release two", Some("\"r2\""));
    lab.upstreams[0].put(TIMESTAMPED, b"timestamped two", Some("\"t2\""));
    for path in [JAR_PATH, TIMESTAMPED] {
        let held = get(&lab, port, "pxy", path);
        assert_eq!(held.status, 502, "{path}: {}", held.text());
        assert!(held.text().contains("withheld"), "{}", held.text());
        // Y sigue retenido: no basta con volver a pedirlo.
        assert_eq!(get(&lab, port, "pxy", path).status, 502, "{path}");
    }
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC3: prioridad, 404/5xx, conflicto de coordenadas, credenciales, politica.
// ---------------------------------------------------------------------------

fn two_proxies_and_groups() -> Lab {
    Lab::new(
        vec![Proxy::new("pxy-a", 0), Proxy::new("pxy-b", 1)],
        &[
            ("g-ab", &["pxy-a", "pxy-b"]),
            ("g-ba", &["pxy-b", "pxy-a"]),
            ("g-hb", &[HOSTED, "pxy-b"]),
            ("g-bh", &["pxy-b", HOSTED]),
        ],
    )
}

#[test]
fn the_first_member_that_has_the_file_wins_and_a_miss_does_not_stop_the_search() {
    let lab = two_proxies_and_groups();
    lab.upstreams[0].put("org/up/lib/1.0/both.jar", b"from a", None);
    lab.upstreams[1].put("org/up/lib/1.0/both.jar", b"from b", None);
    lab.upstreams[1].put("org/up/lib/1.0/only-b.jar", b"only b", None);
    let mut service = lab.start();
    let port = service.port;

    let a_first = get(&lab, port, "g-ab", "org/up/lib/1.0/both.jar");
    assert_eq!(a_first.body, b"from a");
    assert_eq!(a_first.resolved_from.as_deref(), Some("pxy-a"));
    let b_first = get(&lab, port, "g-ba", "org/up/lib/1.0/both.jar");
    assert_eq!(b_first.body, b"from b");
    assert_eq!(b_first.resolved_from.as_deref(), Some("pxy-b"));

    // El primero no lo tiene (404): se sigue con el segundo.
    let only = get(&lab, port, "g-ab", "org/up/lib/1.0/only-b.jar");
    assert_eq!(only.status, 200);
    assert_eq!(only.resolved_from.as_deref(), Some("pxy-b"));
    assert_eq!(lab.upstreams[0].hits("org/up/lib/1.0/only-b.jar"), 1);

    // Si nadie lo tiene, un 404 limpio.
    assert_eq!(
        get(&lab, port, "g-ab", "org/up/lib/1.0/none.jar").status,
        404
    );
    support::stop(&mut service);
}

#[test]
fn an_upstream_failure_is_an_error_and_the_group_does_not_jump_to_the_next_member() {
    let lab = two_proxies_and_groups();
    lab.upstreams[1].put("org/up/lib/1.0/fresh.jar", b"from b", None);
    lab.upstreams[0].put(METADATA_PATH, &metadata(&["1.0"]), None);
    lab.upstreams[1].put(METADATA_PATH, &metadata(&["2.0"]), None);
    let mut service = lab.start();
    let port = service.port;

    lab.upstreams[0].fail(true);
    let failed = get(&lab, port, "g-ab", "org/up/lib/1.0/fresh.jar");
    assert_eq!(failed.status, 502, "{}", failed.text());
    assert_eq!(
        lab.upstreams[1].hits("org/up/lib/1.0/fresh.jar"),
        0,
        "the group jumped over a failing member"
    );
    // Los metadatos tampoco se sirven a medias: sin un miembro, no se
    // fabrica una lista de versiones incompleta.
    let partial = get(&lab, port, "g-ab", METADATA_PATH);
    assert_eq!(partial.status, 502, "{}", partial.text());

    // Cuando el miembro se recupera, todo vuelve a resolverse.
    lab.upstreams[0].fail(false);
    assert_eq!(
        get(&lab, port, "g-ab", "org/up/lib/1.0/fresh.jar").status,
        200
    );
    let merged = get(&lab, port, "g-ab", METADATA_PATH).text();
    assert!(merged.contains("<version>1.0</version>"), "{merged}");
    assert!(merged.contains("<version>2.0</version>"), "{merged}");
    support::stop(&mut service);
}

#[test]
fn the_checksum_is_always_the_one_of_the_file_that_the_group_serves() {
    let lab = two_proxies_and_groups();
    lab.upstreams[1].put(JAR_PATH, OTHER_JAR, None);
    // El upstream ofrece un sidecar que no corresponde a su fichero: el
    // group nunca lo usa.
    lab.upstreams[1].put(&format!("{JAR_PATH}.sha1"), b"deadbeef", None);
    let mut service = lab.start();
    let port = service.port;
    lab.publish(port, POM_PATH, &pom("org.up", "lib", "1.0"));
    lab.publish(port, JAR_PATH, JAR);

    // Mismas coordenadas en el hosted y en el proxy: gana el primero y su
    // checksum es el suyo, en los dos ordenes.
    let hosted_first = get(&lab, port, "g-hb", JAR_PATH);
    assert_eq!(hosted_first.body, JAR);
    assert_eq!(hosted_first.resolved_from.as_deref(), Some(HOSTED));
    assert_eq!(
        get(&lab, port, "g-hb", &format!("{JAR_PATH}.sha1")).text(),
        sha1_hex(JAR)
    );
    let proxy_first = get(&lab, port, "g-bh", JAR_PATH);
    assert_eq!(proxy_first.body, OTHER_JAR);
    assert_eq!(proxy_first.resolved_from.as_deref(), Some("pxy-b"));
    assert_eq!(
        get(&lab, port, "g-bh", &format!("{JAR_PATH}.sha1")).text(),
        sha1_hex(OTHER_JAR)
    );
    support::stop(&mut service);
}

#[test]
fn each_proxy_sends_only_its_own_credential_and_never_the_one_of_the_caller() {
    let lab = Lab::new(
        vec![
            Proxy::new("pxy-a", 0).credential("env:TOKEN_A"),
            Proxy::new("pxy-b", 1).credential("env:TOKEN_B"),
        ],
        &[("g-ab", &["pxy-a", "pxy-b"])],
    );
    lab.upstreams[1].put("org/up/lib/1.0/only-b.jar", b"only b", None);
    let mut service = lab.start_with(&[
        ("HODEI_PROXY__ALLOWED_CREDENTIALS", "TOKEN_A,TOKEN_B"),
        ("TOKEN_A", "secret-of-a"),
        ("TOKEN_B", "secret-of-b"),
    ]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");
    assert_eq!(
        get(&lab, port, "g-ab", "org/up/lib/1.0/only-b.jar").status,
        200
    );

    let seen_a = lab.upstreams[0].requests();
    let seen_b = lab.upstreams[1].requests();
    assert!(!seen_a.is_empty() && !seen_b.is_empty());
    for seen in &seen_a {
        assert_eq!(seen.authorization.as_deref(), Some("Bearer secret-of-a"));
    }
    for seen in &seen_b {
        assert_eq!(seen.authorization.as_deref(), Some("Bearer secret-of-b"));
    }
    // Ni los secretos ni la credencial de quien pregunta salen por el log.
    let log = lab.deployment.the_log();
    for secret in ["secret-of-a", "secret-of-b", alice.as_str()] {
        assert!(!log.contains(secret), "a credential reached the log");
    }
    support::stop(&mut service);
}

#[test]
fn a_proxy_without_a_credential_sends_none_not_even_the_one_of_the_caller() {
    let lab = Lab::new(
        vec![Proxy::new("pxy-open", 0)],
        &[("g-open", &["pxy-open"])],
    );
    lab.upstreams[0].put(JAR_PATH, JAR, None);
    lab.upstreams[0].put(METADATA_PATH, &metadata(&["1.0"]), None);
    let mut service = lab.start();
    let port = service.port;
    assert_eq!(get(&lab, port, "pxy-open", JAR_PATH).status, 200);
    assert_eq!(get(&lab, port, "g-open", METADATA_PATH).status, 200);
    let seen = lab.upstreams[0].requests();
    assert!(!seen.is_empty());
    for one in seen {
        assert_eq!(one.authorization, None, "{}", one.path);
    }
    support::stop(&mut service);
}

#[test]
fn a_release_only_proxy_does_not_ask_the_upstream_for_snapshots() {
    let lab = Lab::new(
        vec![Proxy::new("pxy-rel", 0).maven(serde_json::json!({ "version_policy": "release" }))],
        &[],
    );
    lab.upstreams[0].put(FLOATING, b"a snapshot", None);
    lab.upstreams[0].put(JAR_PATH, JAR, None);
    let mut service = lab.start();
    let port = service.port;
    assert_eq!(get(&lab, port, "pxy-rel", FLOATING).status, 404);
    assert_eq!(lab.upstreams[0].hits(FLOATING), 0);
    assert_eq!(get(&lab, port, "pxy-rel", JAR_PATH).body, JAR);
    support::stop(&mut service);
}

#[test]
fn metadata_that_cannot_be_read_is_a_bad_gateway_in_a_group_and_verbatim_in_the_proxy() {
    let lab = Lab::new(vec![Proxy::new("pxy-a", 0)], &[("g-a", &["pxy-a"])]);
    lab.upstreams[0].put(METADATA_PATH, b"<metadata><unclosed>", None);
    let mut service = lab.start();
    let port = service.port;
    let direct = get(&lab, port, "pxy-a", METADATA_PATH);
    assert_eq!(direct.status, 200);
    assert_eq!(direct.body, b"<metadata><unclosed>");
    let grouped = get(&lab, port, "g-a", METADATA_PATH);
    assert_eq!(grouped.status, 502, "{}", grouped.text());
    support::stop(&mut service);
}

#[test]
fn a_group_skips_a_release_only_proxy_for_snapshots_and_does_not_offer_its_snapshot_versions() {
    let lab = Lab::new(
        vec![
            Proxy::new("pxy-rel", 0).maven(serde_json::json!({ "version_policy": "release" })),
            Proxy::new("pxy-b", 1),
        ],
        &[("g-rb", &["pxy-rel", "pxy-b"])],
    );
    lab.upstreams[0].put(FLOATING, b"never asked", None);
    lab.upstreams[1].put(FLOATING, b"from b", None);
    lab.upstreams[0].put(METADATA_PATH, &metadata(&["1.0", "2.0-SNAPSHOT"]), None);
    let mut service = lab.start();
    let port = service.port;

    // El proxy de releases es un MISS para un snapshot: gana el siguiente.
    let snapshot = get(&lab, port, "g-rb", FLOATING);
    assert_eq!(snapshot.body, b"from b");
    assert_eq!(snapshot.resolved_from.as_deref(), Some("pxy-b"));
    assert_eq!(lab.upstreams[0].hits(FLOATING), 0);

    // Y no anuncia una version que luego no serviria: el cliente que pide
    // `latest` recibiria un 404.
    let merged = get(&lab, port, "g-rb", METADATA_PATH).text();
    assert!(merged.contains("<version>1.0</version>"), "{merged}");
    assert!(!merged.contains("SNAPSHOT"), "{merged}");
    assert!(merged.contains("<latest>1.0</latest>"), "{merged}");
    support::stop(&mut service);
}

#[test]
fn metadata_of_another_artifact_is_never_served_as_if_it_were_ours() {
    let lab = two_proxies_and_groups();
    let foreign = String::from_utf8(metadata(&["9.9"]))
        .expect("utf8")
        .replace("org.up", "org.evil");
    lab.upstreams[0].put(METADATA_PATH, foreign.as_bytes(), None);
    lab.upstreams[1].put(METADATA_PATH, &metadata(&["../evil"]), None);
    let mut service = lab.start();
    let port = service.port;
    // Directo, el proxy lo sirve como esta; el group no.
    assert_eq!(get(&lab, port, "pxy-a", METADATA_PATH).status, 200);
    let grouped = get(&lab, port, "g-ab", METADATA_PATH);
    assert_eq!(grouped.status, 404, "{}", grouped.text());
    support::stop(&mut service);
}

#[test]
fn plugin_metadata_without_versions_and_the_version_metadata_come_from_the_first_member() {
    let lab = two_proxies_and_groups();
    let plugins = b"<metadata><plugins><plugin><prefix>up</prefix></plugin></plugins></metadata>";
    let up_plugins = "org/up/maven-metadata.xml";
    let snapshot_metadata = format!("{SNAPSHOT_DIR}/maven-metadata.xml");
    lab.upstreams[0].put(up_plugins, plugins, None);
    lab.upstreams[0].put(&snapshot_metadata, b"<metadata>from a</metadata>", None);
    lab.upstreams[1].put(&snapshot_metadata, b"<metadata>from b</metadata>", None);
    let mut service = lab.start();
    let port = service.port;

    let served = get(&lab, port, "g-ab", up_plugins);
    assert_eq!(served.status, 200, "{}", served.text());
    assert_eq!(served.body, plugins);

    // Los de version no se fusionan: gana el primero que los tiene.
    let first = get(&lab, port, "g-ab", &snapshot_metadata);
    assert_eq!(first.body, b"<metadata>from a</metadata>");
    assert_eq!(first.resolved_from.as_deref(), Some("pxy-a"));
    let second = get(&lab, port, "g-ba", &snapshot_metadata);
    assert_eq!(second.body, b"<metadata>from b</metadata>");
    assert_eq!(second.resolved_from.as_deref(), Some("pxy-b"));
    support::stop(&mut service);
}

#[test]
fn every_checksum_algorithm_of_group_metadata_matches_the_bytes_that_are_served() {
    use md5::Md5;
    use sha2::{Sha256, Sha512};
    let lab = two_proxies_and_groups();
    lab.upstreams[0].put(METADATA_PATH, &metadata(&["1.0"]), None);
    lab.upstreams[1].put(METADATA_PATH, &metadata(&["2.0"]), None);
    let mut service = lab.start();
    let port = service.port;
    let body = get(&lab, port, "g-ab", METADATA_PATH).body;
    let hex = |bytes: &[u8]| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    let expected = [
        ("md5", hex(&Md5::digest(&body))),
        ("sha1", sha1_hex(&body)),
        ("sha256", hex(&Sha256::digest(&body))),
        ("sha512", hex(&Sha512::digest(&body))),
    ];
    for (extension, digest) in expected {
        let sidecar = get(&lab, port, "g-ab", &format!("{METADATA_PATH}.{extension}"));
        assert_eq!(sidecar.status, 200, "{extension}: {}", sidecar.text());
        assert_eq!(sidecar.text(), digest, "{extension}");
    }
    support::stop(&mut service);
}

#[test]
fn offline_a_group_with_several_proxies_serves_from_the_one_that_has_a_copy() {
    let lab = two_proxies_and_groups();
    lab.upstreams[1].put("org/up/lib/1.0/only-b.jar", b"only b", None);
    lab.upstreams[1].put(METADATA_PATH, &metadata(&["2.0"]), None);
    let mut service = lab.start();
    let port = service.port;
    // En linea: pxy-a no la tiene (404, que no se cachea) y pxy-b si.
    for path in ["org/up/lib/1.0/only-b.jar", METADATA_PATH] {
        assert_eq!(get(&lab, port, "g-ab", path).status, 200, "{path}");
    }
    support::stop(&mut service);

    let before = (lab.upstreams[0].total(), lab.upstreams[1].total());
    let mut offline = lab.start_with(&[("HODEI_PROXY__OFFLINE", "true")]);
    let port = offline.port;
    // pxy-a no tiene copia ni red: no es un error del group, es un miembro
    // que no aporta. Sirve el que si la tiene.
    let jar = get(&lab, port, "g-ab", "org/up/lib/1.0/only-b.jar");
    assert_eq!(jar.status, 200, "{}", jar.text());
    assert_eq!(jar.body, b"only b");
    assert_eq!(jar.resolved_from.as_deref(), Some("pxy-b"));
    let merged = get(&lab, port, "g-ab", METADATA_PATH);
    assert_eq!(merged.status, 200, "{}", merged.text());
    assert!(merged.text().contains("<version>2.0</version>"));
    // Lo que nadie tiene sigue siendo "sin red", no un 404 que engane.
    let unknown = get(&lab, port, "g-ab", "org/up/lib/1.0/never.jar");
    assert_eq!(unknown.status, 503, "{}", unknown.text());
    assert_eq!(
        (lab.upstreams[0].total(), lab.upstreams[1].total()),
        before,
        "offline touched the network"
    );
    support::stop(&mut offline);
}
