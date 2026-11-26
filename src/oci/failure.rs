//! The error dialect of OCI distribution.
//!
//! An OCI client does not read a bare `404`: it reads an **envelope** with a
//! dialect code (`BLOB_UNKNOWN`, `MANIFEST_UNKNOWN`,
//! `BLOB_UPLOAD_INVALID`, …) and a message. The code is what the client
//! understands, and the HTTP status is what the transport understands.
//!
//! And there is a rule the dialect imposes that tends to get lost: a
//! resource of **another account** does not exist for this one, and its error is the same
//! as that of a resource that does not exist. A `403` or a `404` with a message
//! that tells them apart leaks the existence of what belongs to others.
//!
//! That is why the constructors here do not accept the name of the foreign
//! account: the message says what was asked for, never whose it is.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

/// Why an OCI request cannot be served, with its OCI code.
#[derive(Debug, Clone)]
pub struct OciFailure {
    /// The error code in the distribution dialect.
    code: &'static str,
    /// What happened, stated without credentials or disk paths.
    message: String,
    /// The HTTP status the OCI code carries.
    status: StatusCode,
    /// The range that exists now, for the responses that state it.
    ///
    /// It is what lets a client recover the offset of an upload
    /// that has gone out of sync: the error says how much there is, and the client
    /// continues from there.
    range: Option<String>,
    /// The authentication challenge, for `401` responses.
    ///
    /// A `401` without a challenge does not tell the client how to authenticate: it says
    /// that it cannot, and leaves it there. The challenge is what turns a
    /// "no" into a "here is how it is done".
    challenge: Option<String>,
    /// The seconds of `Retry-After`, for upload saturation.
    ///
    /// The account is doing nothing wrong: it is doing too much at once. The status
    /// without the advice would be a "no" with no way back.
    retry_after: Option<u64>,
}

impl OciFailure {
    /// The blob is not there, or the repository does not reference it.
    pub fn blob_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "BLOB_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The manifest is not there, or the repository does not reference it.
    pub fn manifest_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "MANIFEST_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The repository does not exist for this account.
    pub fn name_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "NAME_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The repository name is not valid.
    pub fn name_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "NAME_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The digest is not a digest, or is not that of the content.
    pub fn digest_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "DIGEST_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The upload is not valid: the range does not fit, or the state does not fit.
    pub fn blob_upload_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "BLOB_UPLOAD_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The upload does not fit the offset the client states.
    ///
    /// The dialect code is the same as for an invalid upload --the
    /// chunk that arrives does not fit--, but the status is the `416` the
    /// protocol specifies for a range that cannot be satisfied: a
    /// client whose chunk did not fit needs to know that its offset
    /// is out of sync, and not a `400` that could be anything.
    pub fn blob_upload_range_mismatch(message: impl Into<String>) -> Self {
        Self {
            code: "BLOB_UPLOAD_INVALID",
            message: message.into(),
            status: StatusCode::RANGE_NOT_SATISFIABLE,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The upload session does not exist, or does not belong to this account.
    pub fn blob_upload_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "BLOB_UPLOAD_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The manifest cannot be read or is not of the supported subset.
    pub fn manifest_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "MANIFEST_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The manifest references a blob the repository does not have.
    pub fn manifest_blob_unknown(message: impl Into<String>) -> Self {
        Self {
            code: "MANIFEST_BLOB_UNKNOWN",
            message: message.into(),
            status: StatusCode::NOT_FOUND,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The declared length is not that of the content.
    pub fn size_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "SIZE_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The requested number of results is not a number.
    ///
    /// It is a **client** error, and that is why it is a 400: the `n`
    /// parameter of the tag list is an integer, and an integer that is
    /// not one is not a request the registry cannot serve, it is a
    /// badly written request. The code is the one the reference registry
    /// uses, so that a client that looks at the code sees the same
    /// as it would see on any other.
    pub fn pagination_number_invalid(message: impl Into<String>) -> Self {
        Self {
            code: "PAGINATION_NUMBER_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The operation does not exist on this surface.
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: "UNSUPPORTED",
            message: message.into(),
            status: StatusCode::METHOD_NOT_ALLOWED,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The decision is not permit.
    pub fn denied(message: impl Into<String>) -> Self {
        Self {
            code: "DENIED",
            message: message.into(),
            status: StatusCode::FORBIDDEN,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// There is no credential, or it is not valid.
    pub fn unauthorized() -> Self {
        Self {
            code: "UNAUTHORIZED",
            message: "authentication required".into(),
            status: StatusCode::UNAUTHORIZED,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// Something failed internally and it is not the client's fault.
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "UNAVAILABLE",
            message: message.into(),
            status: StatusCode::SERVICE_UNAVAILABLE,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The account's storage quota does not allow for more.
    ///
    /// The dialect code is `SIZE_INVALID`: the client sends more
    /// bytes than the account can hold, and it is the closest the
    /// protocol has to a `413`. Resending the same later changes
    /// nothing, and that is why it carries no `Retry-After`.
    pub fn storage_over_quota(message: impl Into<String>) -> Self {
        Self {
            code: "SIZE_INVALID",
            message: message.into(),
            status: StatusCode::PAYLOAD_TOO_LARGE,
            range: None,
            challenge: None,
            retry_after: None,
        }
    }

    /// The account already has as many uploads in progress as it can.
    ///
    /// `TOO_MANY_REQUESTS` is the code the protocol itself reserves
    /// for this, and the `Retry-After` is the honest half of the "no": the
    /// same bytes are fine in a moment.
    pub fn too_many_requests(message: impl Into<String>, retry_after_secs: u64) -> Self {
        Self {
            code: "TOO_MANY_REQUESTS",
            message: message.into(),
            status: StatusCode::TOO_MANY_REQUESTS,
            range: None,
            challenge: None,
            retry_after: Some(retry_after_secs),
        }
    }

    /// The error code, as it goes in the envelope.
    pub fn the_code(&self) -> &'static str {
        self.code
    }

    /// The HTTP status of the error.
    pub fn the_status(&self) -> StatusCode {
        self.status
    }

    /// The error with the range that exists now.
    ///
    /// It is what lets a client recover the offset of an upload
    /// that has gone out of sync: the error says how much there is, and the client
    /// continues from there.
    pub fn with_range(mut self, range: impl Into<String>) -> Self {
        self.range = Some(range.into());
        self
    }

    /// The error with the authentication challenge.
    ///
    /// The challenge says where the token endpoint is and which service it
    /// is about, and is what a Docker client reads to know how to
    /// authenticate. Without it, the client stays at the `401`.
    pub fn with_challenge(mut self, realm: impl Into<String>) -> Self {
        self.challenge = Some(realm.into());
        self
    }

    /// The error with the challenge, if it carries one.
    ///
    /// The challenge belongs to `401` responses and only to them: a `404` with
    /// a challenge would be a response asking for a credential that will not
    /// help. An error that already carries its challenge keeps it.
    pub fn with_the_realm(mut self, realm: impl Into<String>) -> Self {
        if self.status == StatusCode::UNAUTHORIZED && self.challenge.is_none() {
            self.challenge = Some(realm.into());
        }
        self
    }
}

/// The error response, in the distribution dialect.
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
            // The challenge: where the token endpoint is and which service
            // it is about. It is what tells a Docker client how to
            // authenticate, and without it the client has nowhere to
            // start.
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
            // The range that exists now: a client whose chunk does not fit
            // needs to know where to continue.
            response.headers_mut().insert(header::RANGE, value);
        }
        if let Some(seconds) = self.retry_after
            && let Ok(value) = axum::http::HeaderValue::from_str(&seconds.to_string())
        {
            // When to come back: saturation is temporary, and a 429 without
            // advice is a no with no way back.
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each dialect code carries the status the protocol specifies.
    ///
    /// The status is not an implementation detail: it is part of the
    /// contract with the client, and a code with a status that does not
    /// match it makes the client take a wrong decision.
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
                OciFailure::storage_over_quota("x"),
                StatusCode::PAYLOAD_TOO_LARGE,
                "SIZE_INVALID",
            ),
            (
                OciFailure::too_many_requests("x", 1),
                StatusCode::TOO_MANY_REQUESTS,
                "TOO_MANY_REQUESTS",
            ),
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

    /// The envelope carries the dialect code, which is what the client
    /// reads to decide what to do.
    #[test]
    fn the_envelope_carries_the_code() {
        let response = OciFailure::blob_upload_unknown("no such upload").into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// Saturation says when to come back: a `429` without `Retry-After` is
    /// a no with no way back, and the advice is half of the no.
    #[test]
    fn the_saturation_carries_the_advice_to_come_back() {
        let response = OciFailure::too_many_requests("x", 7).into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response
                .headers()
                .get(header::RETRY_AFTER)
                .map(|value| value.as_bytes()),
            Some(b"7".as_slice())
        );
        // And the quota 413 does not carry it: coming back later does not make the
        // account bigger.
        let over = OciFailure::storage_over_quota("x").into_response();
        assert_eq!(over.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(over.headers().get(header::RETRY_AFTER).is_none());
    }
}
