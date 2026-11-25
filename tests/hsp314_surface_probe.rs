//! HSP-314 phase 0: which surfaces answer what on the graph profile.
//!
//! A certification probe, not a product contract: it starts the real binary
//! (built with the features of this target) on the graph profile against the
//! Dgraph named by `HODEI_GRAPH_IT_ENDPOINT` and prints the status of the
//! surfaces that only the embedded profile wires (investigation, reactive
//! CVE cycle). It asserts nothing about them: it records. Guarded by
//! `HSP314_SURFACE_PROBE=1`, otherwise it prints SKIPPED.

mod support;

use std::process::Command;

use support::{Deployment, token};

fn call(port: u16, method: &str, path: &str, bearer: &str, body: &str) -> (u16, String) {
    let out = Command::new("curl")
        .args([
            "-s",
            "-m",
            "20",
            "-X",
            method,
            "-H",
            &format!("Authorization: Bearer {bearer}"),
            "-H",
            "Content-Type: application/json",
            "-d",
            body,
            "-w",
            "\n%{http_code}",
            &format!("http://127.0.0.1:{port}{path}"),
        ])
        .output()
        .expect("curl runs");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", "0"));
    (code.trim().parse().unwrap_or(0), body.to_string())
}

#[test]
fn surfaces_of_the_graph_profile() {
    if std::env::var("HSP314_SURFACE_PROBE").as_deref() != Ok("1") {
        eprintln!("SKIPPED: set HSP314_SURFACE_PROBE=1 and HODEI_GRAPH_IT_ENDPOINT");
        return;
    }
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").expect("the Dgraph endpoint");
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_else(|_| "groot".into());
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_else(|_| "password".into());
    let deployment = Deployment::new();
    // A policy that permits everything: the probe measures what is wired,
    // not what Cedar allows.
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        "permit(principal, action, resource);\n",
    )
    .expect("the policy");
    let environment: Vec<(String, String)> = vec![
        ("RUN_MODE".into(), "dgraph".into()),
        ("HODEI_LOGGING__FORMAT".into(), "pretty".into()),
        ("HODEI_GRAPH__ENDPOINT".into(), endpoint),
        ("HODEI_GRAPH__TLS__ENABLED".into(), "false".into()),
        ("HODEI_GRAPH__TLS__CA_CERT_PATH".into(), String::new()),
        ("HODEI_GRAPH__TLS__SERVER_NAME".into(), String::new()),
        ("HODEI_GRAPH__NAMESPACE".into(), "0".into()),
        ("HODEI_GRAPH__CREDENTIALS__USERID".into(), user),
        ("HODEI_GRAPH__CREDENTIALS__PASSWORD".into(), password),
        ("HODEI_SECURITY__ADMIN_API_ENABLED".into(), "true".into()),
        (
            "HODEI_SECURITY__INVESTIGATION_API_ENABLED".into(),
            "true".into(),
        ),
    ];
    let refs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let service = deployment.start(&refs);
    let bearer = token("acme", "alice");
    let digest = format!("sha256:{}", "ab".repeat(32));
    let subject = "hrn:hodei:artifacts:eu-west-1:acme:artifact/releases/sha256-abababababababab";

    let probes = [
        (
            "POST /api/v1/advisories/accept",
            call(
                service.port,
                "POST",
                "/api/v1/advisories/accept",
                &bearer,
                &format!(
                    r#"{{"repository":"releases","digest":"{digest}","advisory":{{"id":"GHSA-0000-0000-0000","modified":"2026-01-01T00:00:00Z","affected":[]}}}}"#
                ),
            ),
        ),
        (
            "POST /api/v1/investigations",
            call(
                service.port,
                "POST",
                "/api/v1/investigations",
                &bearer,
                &format!(r#"{{"subject":"{subject}"}}"#),
            ),
        ),
        (
            "POST /api/v1/investigations/freshness",
            call(
                service.port,
                "POST",
                "/api/v1/investigations/freshness",
                &bearer,
                &format!(r#"{{"subject":"{subject}"}}"#),
            ),
        ),
        (
            "POST /api/v1/promotions",
            call(
                service.port,
                "POST",
                "/api/v1/promotions",
                &bearer,
                &format!(r#"{{"repository":"releases","subject":"{subject}","digest":"{digest}"}}"#),
            ),
        ),
    ];
    for (name, (status, body)) in probes {
        let body: String = body.chars().take(240).collect();
        eprintln!("PROBE {name} -> {status} {body}");
    }
    let log = deployment.the_log();
    for line in log.lines().filter(|l| {
        l.contains("not wired") || l.contains("not available") || l.contains("graph profile")
    }) {
        eprintln!("LOG {}", support::without_the_colors(line));
    }
}
