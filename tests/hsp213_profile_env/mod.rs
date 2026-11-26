//! Los fixtures del despliegue antiguo para el contrato de HSP-213.
//!
//! Lo usa `hsp213_profile_rejection`: el binario productivo rechaza el
//! perfil antiguo sin `[graph]`. El binario es
//! `CARGO_BIN_EXE_hodei-artifacts-api`: el que Cargo acaba de construir,
//! y el unico arbol que existe (sin motor embebido).

use std::path::Path;

/// The binary under test, as built by Cargo for this integration test.
pub const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");

/// An ephemeral port, released before the binary binds it.
pub fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("the address").port();
    drop(listener);
    port
}

/// El esquema Cedar del despliegue, con las acciones que el servicio
/// autoriza: un bundle que no las declara muere en la validacion de
/// seguridad antes de llegar al despacho del perfil, y la prueba mediria
/// otra cosa. Es el mismo que sirve el arnes de `tests/support` (hsp138).
pub const SCHEMA: &str = r#"{
  "": { "entityTypes": {}, "actions": {} },
  "Iam": { "entityTypes": { "User": { "shape": { "type": "Record", "attributes": {} } } }, "actions": {} },
  "Artifacts": {
    "entityTypes": {
      "Repository": { "memberOfTypes": ["Artifacts::Repository"], "shape": { "type": "Record", "attributes": {} } },
      "Artifact": { "shape": { "type": "Record", "attributes": {} } }
    },
    "actions": {
      "CreateRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ReadRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "PromoteAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "RevokePromotion": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ReadCapsule": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Artifact"] } },
      "UploadAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "DownloadAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } }
    }
  }
}"#;

/// The environment an old embedded deployment still carries: no `[graph]`,
/// the retired local store path, and the production bundle the validator
/// demands.
pub fn retired_profile_env(tmp: &Path, port: u16) -> Vec<(String, String)> {
    let policy = tmp.join("policies.cedar");
    let schema = tmp.join("schema.json");
    std::fs::write(&policy, "permit(principal, action, resource);\n").expect("the policy");
    std::fs::write(&schema, SCHEMA).expect("the schema");
    vec![
        ("HODEI_SERVER__HOST".to_string(), "127.0.0.1".to_string()),
        ("HODEI_SERVER__PORT".to_string(), port.to_string()),
        (
            "HODEI_ROCKSDB__PATH".to_string(),
            tmp.join("db").to_string_lossy().into_owned(),
        ),
        (
            "HODEI_CONTENT__CAS_PATH".to_string(),
            tmp.join("cas").to_string_lossy().into_owned(),
        ),
        (
            "HODEI_SECURITY__RAW_API_ENABLED".to_string(),
            "true".to_string(),
        ),
        (
            "HODEI_SECURITY__OCI_API_ENABLED".to_string(),
            "true".to_string(),
        ),
        (
            "HODEI_SECURITY__AUTH_JWT_SECRET".to_string(),
            "0123456789abcdef0123456789abcdef".to_string(),
        ),
        (
            "HODEI_SECURITY__AUTH_ISSUER".to_string(),
            "hodei-tests".to_string(),
        ),
        (
            "HODEI_SECURITY__AUTH_AUDIENCE".to_string(),
            "hodei".to_string(),
        ),
        (
            "HODEI_SECURITY__AUTHORIZATION_POLICY_PATH".to_string(),
            policy.to_string_lossy().into_owned(),
        ),
        (
            "HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH".to_string(),
            schema.to_string_lossy().into_owned(),
        ),
        ("RUST_LOG".to_string(), "info".to_string()),
    ]
}
