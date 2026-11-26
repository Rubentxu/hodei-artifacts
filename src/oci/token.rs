//! The token endpoint: the door through which Docker clients enter.
//!
//! The Docker protocol does not send the platform token: it sends some
//! credentials and receives a token. When a real client (`podman`,
//! `oras`, the CLI `docker`) reaches a registry, the first thing it does
//! is **ask how to authenticate**: a `GET /v2/` without a credential,
//! and read the `WWW-Authenticate` of the response. If the response says
//! `Bearer realm="..."`, the client goes to that `realm` with its
//! credentials and receives a token that it uses in the requests that
//! follow.
//!
//! Without that endpoint, a Docker client does not know how to talk to
//! the registry: it is not that the registry does not accept the token,
//! it is that the client never finds out that it has to send it.
//!
//! # What the returned token is and is not
//!
//! The registry **does not issue** tokens: what it returns is **the
//! platform credential**, verified at the door. The client presents it
//! as `Bearer` in each request, and it is the same credential the
//! client already had.
//!
//! And what decides what the client can do **is not the token**: it is
//! Cedar, on every request and on the specific resource. A token from
//! this registry carries no permissions inside: it carries an identity,
//! and permissions are decided with the bundle, as in the rest of the
//! platform. Pretending otherwise --issuing tokens with permissions
//! inside-- would mean having two authorities, and the one that is not
//! consulted is the one that lies.
//!
//! # The secret
//!
//! The credential travels as the **password** of basic credentials,
//! which is how the Docker protocol carries them. The user is
//! informative: what is verified is the password.

use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;
use crate::public_url::TheOrigin;

/// The service name in the authentication challenge.
///
/// The protocol uses it so the client knows which registry it is
/// talking to. It is the product name, and says nothing about who asks.
pub const SERVICE_NAME: &str = "hodei";

/// The path of the token endpoint.
///
/// It is the protocol convention, and that is why it is used: a client
/// that ignores the `realm` the registry announces finds it anyway.
pub const TOKEN_PATH: &str = "/token";

/// How long we tell the client to use the token before asking for
/// another.
///
/// It is a **cache hint**, not an expiry: the returned token is the
/// platform credential, and its expiry is whatever it has. What the
/// client is told is how often it is advisable to renew it, and
/// renewing it means going back to this endpoint with the same credentials.
const REFRESH_SECONDS: i64 = 300;

/// The token endpoint.
///
/// Credentials arrive in the `Authorization` header in basic form:
/// the user is informative and the password is the platform
/// credential. Without a credential there is no token, and what is
/// returned is the same challenge the client already knows.
pub async fn token(
    State(state): State<AppState>,
    origin: TheOrigin,
    headers: HeaderMap,
) -> Result<Response, OciFailure> {
    let Some(credentials) = the_basic_credentials(&headers) else {
        return Err(OciFailure::unauthorized().with_challenge(the_realm(&origin)));
    };

    // The password is the platform credential, and it is verified the
    // same way as in any other request: with the same authenticator and
    // the same path.
    if hodei_authz::verify_principal(
        state.authenticator.as_ref(),
        &hodei_authz::Credentials::bearer(credentials.password.clone()),
    )
    .await
    .is_err()
    {
        return Err(OciFailure::unauthorized().with_challenge(the_realm(&origin)));
    }

    let body = serde_json::json!({
        "token": credentials.password,
        "expires_in": REFRESH_SECONDS,
        "issued_at": chrono::Utc::now().to_rfc3339(),
        // The scope the client asks for, and the scope it has. The
        // registry does not trim it: what decides what the client can do
        // is Cedar, on every request.
        "scope": "repository:*:pull,push",
    });

    let mut response = (axum::http::StatusCode::OK, body.to_string()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );
    Ok(response)
}

/// The basic credentials of the request.
///
/// The Docker protocol sends them in the `Authorization` header. The
/// password is the platform credential; the user is informative, and
/// therefore decides nothing.
struct BasicCredentials {
    /// The user name the client sends.
    #[allow(dead_code)]
    user: String,
    /// The platform credential.
    password: String,
}

/// The basic credentials from the header, if there are any.
fn the_basic_credentials(headers: &HeaderMap) -> Option<BasicCredentials> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, encoded) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (user, password) = decoded.split_once(':')?;
    if password.is_empty() {
        return None;
    }
    Some(BasicCredentials {
        user: user.to_string(),
        password: password.to_string(),
    })
}

/// The realm of the token endpoint, as the client sees it.
///
/// It is the **public** address of the deployment, the one its
/// configuration decides: the one the client used to arrive and the only
/// one it can use. The request does not decide it, because the `Host` and
/// `X-Forwarded-Proto` of a request can be written by anyone, and a
/// challenge pointing elsewhere is a credential asked for somewhere
/// else.
///
/// When the deployment does not say where it lives and the request does
/// not carry a valid authority, the challenge says `localhost`: it is
/// the address the service itself declares when it knows of no other.
pub fn the_realm(origin: &TheOrigin) -> String {
    origin.url_or(TOKEN_PATH, "http://localhost/token")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The challenge says where the token endpoint is and which service
    /// it is about.
    ///
    /// The realm is the address the deployment declares, not the one the
    /// request writes: a hostile `Host` does not reach the challenge.
    #[test]
    fn the_challenge_names_the_realm_and_the_service() {
        let origin = TheOrigin::default();
        assert_eq!(the_realm(&origin), "http://localhost/token");

        let public = crate::public_url::PublicUrl::of(Some("https://artifacts.example"), &[])
            .expect("a public base URL");
        let origin = TheOrigin::of(&public, None, &HeaderMap::new());
        assert_eq!(the_realm(&origin), "https://artifacts.example/token");
    }

    /// Basic credentials are read, and what is not them is not read.
    #[test]
    fn the_basic_credentials_are_read_when_they_are_there() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Basic YWxpY2U6c2VjcmV0".parse().expect("a header"),
        );
        let credentials = the_basic_credentials(&headers).expect("the credentials");
        assert_eq!(credentials.user, "alice");
        assert_eq!(credentials.password, "secret");

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer the-token".parse().expect("a header"),
        );
        assert!(the_basic_credentials(&headers).is_none());
    }
}
