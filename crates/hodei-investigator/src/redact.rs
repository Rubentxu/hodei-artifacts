//! The redaction of sensitive data BEFORE the model.
//!
//! The evidence text is a third party's, and a third party can put
//! a secret in a README or in the description of an SBOM. What goes out
//! to the model **is redacted**, and what is redacted does NOT enter the trace:
//! the trace carries identifiers, not content.

/// The forms of secret that are redacted, with their test.
///
/// The list is deliberately short and high-signal: every pattern that
/// is added is a pattern that can over-redact, and over-redacting
/// is also hiding.
const PATTERNS: &[(&str, &str)] = &[
    // AWS access keys: AKIA followed by 16 uppercase letters/digits.
    ("AKIA[0-9A-Z]{16}", "aws-access-key"),
    // GitHub tokens: ghp_/gho_/ghs_ followed by 36 characters.
    ("gh[pos]_[A-Za-z0-9]{36}", "github-token"),
    // A Bearer token in a header.
    (
        "(?i)bearer[ \\t]+[A-Za-z0-9\\-\\._~\\+/]+=*",
        "bearer-token",
    ),
    // A JWT: three base64url segments separated by a dot.
    (
        "eyJ[A-Za-z0-9_\\-]{10,}\\.[A-Za-z0-9_\\-]{10,}\\.[A-Za-z0-9_\\-]{10,}",
        "jwt",
    ),
    // A long hexadecimal string (>= 32): signature, digest or secret.
    ("\\b[0-9a-fA-F]{32,}\\b", "hex-secret"),
];

/// Redacts whatever looks like a secret in the text of a piece of evidence.
///
/// What comes out of here is what the model sees, and the same is what the trace
/// would have to see if the trace carried content -- and it does not: the trace
/// carries evidence identifiers, not text.
pub fn redact(text: &str) -> String {
    let mut redacted = text.to_string();
    for (pattern, label) in PATTERNS {
        let re = match regex::Regex::new(pattern) {
            Ok(re) => re,
            Err(_) => continue,
        };
        redacted = re
            .replace_all(&redacted, format!("[REDACTED:{label}]"))
            .into_owned();
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn an_aws_key_is_redacted() {
        let text = "the deploy uses AKIAIOSFODNN7EXAMPLE for the bucket";
        let redacted = redact(text);
        assert!(
            redacted.contains("[REDACTED:aws-access-key]") && !redacted.contains("AKIAIOSFODNN7"),
            "la clave no sale: {redacted}"
        );
    }

    #[test]
    fn a_github_token_is_redacted() {
        let text = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef123456";
        let redacted = redact(text);
        assert!(
            redacted.contains("[REDACTED:github-token]") && !redacted.contains("ghp_ABCD"),
            "el token no sale: {redacted}"
        );
    }

    #[test]
    fn a_bearer_token_and_a_jwt_are_redacted() {
        let text = "Authorization: Bearer abc.def.ghi end; jwt eyJhbGciOiJIUzI1NiIsInR5.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9Pn_9lkKudjej";
        let redacted = redact(text);
        assert!(redacted.contains("[REDACTED:bearer-token]"), "{redacted}");
        assert!(redacted.contains("[REDACTED:jwt]"), "{redacted}");
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiIsInR5"), "{redacted}");
    }

    #[test]
    fn a_long_hex_digest_is_redacted_but_a_short_one_survives() {
        let text = "digest 0123456789abcdef0123456789abcdef01234567 and short 0123abcd";
        let redacted = redact(text);
        assert!(redacted.contains("[REDACTED:hex-secret]"), "{redacted}");
        assert!(
            redacted.contains("0123abcd"),
            "lo corto no es senal de secreto: {redacted}"
        );
    }

    #[test]
    fn plain_text_is_not_touched() {
        let text = "the artifact depends on lib-1.2.3 and passes its SBOM check";
        assert_eq!(redact(text), text, "el texto normal no se toca");
    }
}
