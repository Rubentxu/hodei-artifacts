//! El endpoint de token: la puerta por la que entran los clientes Docker.
//!
//! El protocolo de Docker no manda el token de la plataforma: manda unas
//! credenciales y recibe un token. Cuando un cliente real (`podman`,
//! `oras`, el `docker` de la CLI) llega a un registro, lo primero que
//! hace es **preguntar como autenticarse**: un `GET /v2/` sin
//! credencial, y leer el `WWW-Authenticate` de la respuesta. Si la
//! respuesta dice `Bearer realm="..."`, el cliente va a ese `realm` con
//! sus credenciales y recibe un token que usa en las peticiones que
//! siguen.
//!
//! Sin ese endpoint, un cliente de Docker no sabe como hablar con el
//! registro: no es que el registro no acepte el token, es que el cliente
//! nunca se entera de que hay que mandarlo.
//!
//! # Que es y que no es el token que devuelve
//!
//! El registro **no emite** tokens: lo que devuelve es **la credencial de
//! la plataforma**, verificada en la puerta. El cliente la presenta como
//! `Bearer` en cada peticion, y es la misma credencial que la que el
//! cliente ya tenia.
//!
//! Y lo que decide que puede hacer el cliente **no es el token**: es
//! Cedar, en cada peticion y sobre el recurso concreto. Un token de este
//! registro no lleva permisos dentro: lleva una identidad, y los
//! permisos se deciden con el bundle, como en el resto de la plataforma.
//! Pretender lo contrario --emitir tokens con permisos dentro-- seria
//! tener dos autoridades, y la que no se consulta es la que miente.
//!
//! # El secreto
//!
//! La credencial viaja como la **contrasena** de unas credenciales basicas,
//! que es como el protocolo Docker las lleva. El usuario es informativo:
//! lo que se verifica es la contrasena.

use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;

use crate::app_state::AppState;
use crate::oci::failure::OciFailure;

/// El nombre del servicio en el reto de autenticacion.
///
/// El protocolo lo usa para que el cliente sepa a que registro esta
/// hablando. Es el nombre del producto, y no dice nada de quien pregunta.
pub const SERVICE_NAME: &str = "hodei";

/// La ruta del endpoint de token.
///
/// Es la convencion del protocolo, y por eso se usa: un cliente que
/// ignore el `realm` que anuncia el registro la encuentra igual.
pub const TOKEN_PATH: &str = "/token";

/// Cuanto tiempo le decimos al cliente que use el token antes de pedir
/// otro.
///
/// Es una **pista de cache**, no una caducidad: el token que se devuelve
/// es la credencial de la plataforma, y su caducidad es la que ella
/// tenga. Lo que se le dice al cliente es cada cuanto conviene renovarlo,
/// y renovarlo es volver a este endpoint con las mismas credenciales.
const REFRESH_SECONDS: i64 = 300;

/// El endpoint de token.
///
/// Las credenciales llegan en la cabecera `Authorization` en forma basica:
/// el usuario es informativo y la contrasena es la credencial de la
/// plataforma. Sin credencial no hay token, y lo que se responde es el
/// mismo reto que el cliente ya conoce.
pub async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, OciFailure> {
    let Some(credentials) = the_basic_credentials(&headers) else {
        return Err(OciFailure::unauthorized().with_challenge(the_realm(&headers)));
    };

    // La contrasena es la credencial de la plataforma, y se verifica
    // como se verifica en cualquier otra peticion: con el mismo
    // autenticador y el mismo camino.
    if hodei_authz::verify_principal(
        state.authenticator.as_ref(),
        &hodei_authz::Credentials::bearer(credentials.password.clone()),
    )
    .await
    .is_err()
    {
        return Err(OciFailure::unauthorized().with_challenge(the_realm(&headers)));
    }

    let body = serde_json::json!({
        "token": credentials.password,
        "expires_in": REFRESH_SECONDS,
        "issued_at": chrono::Utc::now().to_rfc3339(),
        // El alcance que el cliente pide, y el alcance que tiene. El
        // registro no lo recorta: lo que decide que puede hacer el
        // cliente es Cedar, en cada peticion.
        "scope": "repository:*:pull,push",
    });

    let mut response = (axum::http::StatusCode::OK, body.to_string()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );
    Ok(response)
}

/// Las credenciales basicas de la peticion.
///
/// El protocolo Docker las manda en la cabecera `Authorization`. La
/// contrasena es la credencial de la plataforma; el usuario es
/// informativo, y por eso no decide nada.
struct BasicCredentials {
    /// El nombre de usuario que manda el cliente.
    #[allow(dead_code)]
    user: String,
    /// La credencial de la plataforma.
    password: String,
}

/// Las credenciales basicas de la cabecera, si las hay.
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

/// El realm del endpoint de token, tal y como lo ve el cliente.
///
/// Se construye con la cabecera `Host` de la peticion: es la direccion
/// por la que el cliente ha llegado, y es la unica que el cliente puede
/// usar. Un despliegue con TLS delante lo dice con `X-Forwarded-Proto`.
pub fn the_realm(headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .filter(|host| is_a_host(host))
        .unwrap_or("localhost");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .filter(|scheme| *scheme == "http" || *scheme == "https")
        .unwrap_or("http");
    format!("{scheme}://{host}{TOKEN_PATH}")
}

/// Si el valor de la cabecera `Host` sirve para una URL.
///
/// Un `Host` con espacios, barras o controles no es un host: es una
/// intento de escribir la cabecera de otra manera, y una cabecera de
/// reto es una cabecera que se lee en un cliente.
fn is_a_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El reto dice donde esta el endpoint de token y de que servicio
    /// se trata.
    #[test]
    fn the_challenge_names_the_realm_and_the_service() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:5000".parse().expect("a host"));
        assert_eq!(the_realm(&headers), "http://127.0.0.1:5000/token");

        headers.insert("x-forwarded-proto", "https".parse().expect("a scheme"));
        assert_eq!(the_realm(&headers), "https://127.0.0.1:5000/token");
    }

    /// Un `Host` que no es un host no se copia a la cabecera del reto.
    ///
    /// Un `Host` con un salto de linea no puede ni construirse: el
    /// analizador de cabeceras lo rechaza antes de que la peticion
    /// llegue al handler. El caso que si puede llegar es el que trae un
    /// camino o un espacio, y ese tampoco es un host.
    #[test]
    fn a_host_that_is_not_a_host_does_not_reach_the_challenge() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "evil.example/path".parse().expect("a header"));
        assert_eq!(the_realm(&headers), "http://localhost/token");

        headers.insert(header::HOST, "evil example".parse().expect("a header"));
        assert_eq!(the_realm(&headers), "http://localhost/token");
    }

    /// Las credenciales basicas se leen, y lo que no son no se leen.
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
