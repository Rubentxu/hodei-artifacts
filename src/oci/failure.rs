//! El dialecto de error de la distribución OCI.
//!
//! Un cliente de OCI no lee un `404` a pelo: lee un **envelope** con un
//! código del dialecto (`BLOB_UNKNOWN`, `MANIFEST_UNKNOWN`,
//! `BLOB_UPLOAD_INVALID`, …) y un mensaje. El código es lo que el cliente
//! entiende, y el estado HTTP es lo que el transporte entiende.
//!
//! Y hay una regla que el dialecto impone y que se suele perder: un
//! recurso de **otra cuenta** no existe para esta, y su error es el mismo
//! que el de un recurso que no existe. Un `403` o un `404` con un mensaje
//! que lo distinga filtra la existencia de lo ajeno.
//!
//! Por eso los constructores de aquí no admiten el nombre de la cuenta
//! ajena: el mensaje dice qué se pidió, nunca de quién es.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

/// Por qué una petición OCI no se puede servir, con su código OCI.
#[derive(Debug, Clone)]
pub struct OciFailure {
    /// El código del error en el dialecto de la distribución.
    code: &'static str,
    /// Lo que pasó, dicho sin credenciales ni rutas de disco.
    message: String,
    /// El estado HTTP que el código OCI lleva.
    status: StatusCode,
    /// El rango que hay ahora, para las respuestas que lo dicen.
    ///
    /// Es lo que permite a un cliente recuperar el offset de una subida
    /// que se ha descolocado: el error dice cuanto hay, y el cliente
    /// sigue desde ahi.
    range: Option<String>,
    /// El reto de autenticacion, para las respuestas `401`.
    ///
    /// Un `401` sin reto no dice al cliente como autenticarse: le dice
    /// que no puede, y se queda ahi. El reto es lo que convierte un
    /// "no" en un "asi se hace".
    challenge: Option<String>,
}

impl OciFailure {
    /// El blob no está, o el repositorio no lo referencia.
    pub fn blob_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "BLOB_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
        }
    }

    /// El manifiesto no está, o el repositorio no lo referencia.
    pub fn manifest_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "MANIFEST_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
        }
    }

    /// El repositorio no existe para esta cuenta.
    pub fn name_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "NAME_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
        }
    }

    /// El nombre del repositorio no es valido.
    pub fn name_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "NAME_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
        }
    }

    /// El digest no es un digest, o no es el del contenido.
    pub fn digest_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "DIGEST_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
        }
    }

    /// La subida no es valida: el rango no encaja, o el estado no encaja.
    pub fn blob_upload_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "BLOB_UPLOAD_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
        }
    }

    /// La subida no encaja con el offset que el cliente dice.
    ///
    /// El codigo del dialecto es el mismo de una subida invalida --el
    /// trozo que llega no encaja--, pero el estado es el `416` que el
    /// protocolo dice para un rango que no se puede satisfacer: un
    /// cliente cuyo trozo no ha encajado necesita saber que su offset
    /// esta desfasado, y no un `400` que podria ser cualquier cosa.
    pub fn blob_upload_range_mismatch(message: impl Into<String>) -> Self {
        Self {
            code: "BLOB_UPLOAD_INVALID",
            message: message.into(),
            status: StatusCode::RANGE_NOT_SATISFIABLE,
            range: None,
            challenge: None,
        }
    }

    /// La sesion de subida no existe, o no es de esta cuenta.
    pub fn blob_upload_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "BLOB_UPLOAD_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
        }
    }

    /// El manifiesto no se puede leer o no es del subconjunto soportado.
    pub fn manifest_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "MANIFEST_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
        }
    }

    /// El manifiesto referencia un blob que el repositorio no tiene.
    pub fn manifest_blob_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "MANIFEST_BLOB_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
        }
    }

    /// La longitud declarada no es la del contenido.
    pub fn size_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "SIZE_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
        }
    }

    /// El numero de resultados que se pide no es un numero.
    ///
    /// Es un error **del cliente**, y por eso es un 400: el parametro
    /// `n` de la lista de etiquetas es un entero, y un entero que no lo
    /// es no es una peticion que el registro no sepa atender, es una
    /// peticion mal escrita. El codigo es el que usa el registro de
    /// referencia, para que un cliente que mire el codigo vea lo mismo
    /// que veria en cualquier otro.
    pub fn pagination_number_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "PAGINATION_NUMBER_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
        }
    }

    /// La operacion no existe en esta superficie.
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: "UNSUPPORTED",
            message: message.into(),
            status: StatusCode::METHOD_NOT_ALLOWED,
            range: None,
            challenge: None,
        }
    }

    /// La decision no es permiso.
    pub fn denied(message: impl Into<String>) -> Self {
        Self {
            code: "DENIED",
            message: message.into(),
            status: StatusCode::FORBIDDEN,
            range: None,
            challenge: None,
        }
    }

    /// No hay credencial, o no es valida.
    pub fn unauthorized() -> Self {
        Self {
            code: "UNAUTHORIZED",
            message: "authentication required".into(),
            status: StatusCode::UNAUTHORIZED,
            range: None,
            challenge: None,
        }
    }

    /// Algo fallo por dentro y no es culpa del cliente.
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "UNAVAILABLE",
            message: message.into(),
            status: StatusCode::SERVICE_UNAVAILABLE,
            range: None,
            challenge: None,
        }
    }

    /// El codigo del error, tal y como va en el envelope.
    pub fn the_code(&self) -> &'static str {
        self.code
    }

    /// El estado HTTP del error.
    pub fn the_status(&self) -> StatusCode {
        self.status
    }

    /// El error con el rango que hay ahora.
    ///
    /// Es lo que permite a un cliente recuperar el offset de una subida
    /// que se ha descolocado: el error dice cuanto hay, y el cliente
    /// sigue desde ahi.
    pub fn with_range(mut self, range: impl Into<String>) -> Self {
        self.range = Some(range.into());
        self
    }

    /// El error con el reto de autenticacion.
    ///
    /// El reto dice donde esta el endpoint de token y de que servicio se
    /// trata, y es lo que un cliente de Docker lee para saber como
    /// autenticarse. Sin el, el cliente se queda en el `401`.
    pub fn with_challenge(mut self, realm: impl Into<String>) -> Self {
        self.challenge = Some(realm.into());
        self
    }
}

/// La respuesta de error, en el dialecto de la distribucion.
impl IntoResponse for OciFailure {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "errors": [{
                "code": self.code,
                "message": self.message,
            }]
        });
        let mut response = (
            self.status,
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            // El reto: donde esta el endpoint de token y de que servicio
            // se trata. Es lo que le dice a un cliente de Docker como
            // autenticarse, y sin el el cliente no tiene por donde
            // empezar.
            let challenge = match &self.challenge {
                Some(realm) => format!(
                    "Bearer realm=\"{realm}\",service=\"{}\"",
                    crate::oci::token::SERVICE_NAME
                ),
                None => "Bearer".to_string(),
            };
            if let Ok(value) = axum::http::HeaderValue::from_str(&challenge) {
                response
                    .headers_mut()
                    .insert(header::WWW_AUTHENTICATE, value);
            }
        }
        if let Some(range) = &self.range
            && let Ok(value) = axum::http::HeaderValue::from_str(range)
        {
            // El rango que hay ahora: un cliente cuyo trozo no encaja
            // necesita saber por donde seguir.
            response.headers_mut().insert(header::RANGE, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada codigo del dialecto lleva el estado que el protocolo dice.
    ///
    /// El estado no es un detalle de la implementacion: es parte del
    /// contrato con el cliente, y un codigo con un estado que no le
    /// corresponde hace que el cliente tome una decision equivocada.
    #[test]
    fn the_codes_carry_the_status_the_protocol_says() {
        let cases: Vec<(OciFailure, StatusCode, &str)> = vec![
            (
                OciFailure::blob_unknown("x"),
                StatusCode::NOT_FOUND,
                "BLOB_UNKNOWN",
            ),
            (
                OciFailure::manifest_unknown("x"),
                StatusCode::NOT_FOUND,
                "MANIFEST_UNKNOWN",
            ),
            (
                OciFailure::name_unknown("x"),
                StatusCode::NOT_FOUND,
                "NAME_UNKNOWN",
            ),
            (
                OciFailure::name_invalid("x"),
                StatusCode::BAD_REQUEST,
                "NAME_INVALID",
            ),
            (
                OciFailure::digest_invalid("x"),
                StatusCode::BAD_REQUEST,
                "DIGEST_INVALID",
            ),
            (
                OciFailure::blob_upload_invalid("x"),
                StatusCode::BAD_REQUEST,
                "BLOB_UPLOAD_INVALID",
            ),
            (
                OciFailure::blob_upload_range_mismatch("x"),
                StatusCode::RANGE_NOT_SATISFIABLE,
                "BLOB_UPLOAD_INVALID",
            ),
            (
                OciFailure::blob_upload_unknown("x"),
                StatusCode::NOT_FOUND,
                "BLOB_UPLOAD_UNKNOWN",
            ),
            (
                OciFailure::manifest_invalid("x"),
                StatusCode::BAD_REQUEST,
                "MANIFEST_INVALID",
            ),
            (
                OciFailure::manifest_blob_unknown("x"),
                StatusCode::NOT_FOUND,
                "MANIFEST_BLOB_UNKNOWN",
            ),
            (
                OciFailure::size_invalid("x"),
                StatusCode::BAD_REQUEST,
                "SIZE_INVALID",
            ),
            (
                OciFailure::unsupported("x"),
                StatusCode::METHOD_NOT_ALLOWED,
                "UNSUPPORTED",
            ),
            (OciFailure::denied("x"), StatusCode::FORBIDDEN, "DENIED"),
            (
                OciFailure::unauthorized(),
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
            ),
        ];
        for (failure, status, code) in cases {
            assert_eq!(failure.the_status(), status, "the status of {code}");
            assert_eq!(failure.the_code(), code, "the code");
        }
    }

    /// El envelope lleva el codigo del dialecto, que es lo que el cliente
    /// lee para decidir que hacer.
    #[test]
    fn the_envelope_carries_the_code() {
        let response = OciFailure::blob_upload_unknown("no such upload").into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
