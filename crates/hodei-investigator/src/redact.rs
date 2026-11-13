//! La redaccion de datos sensibles ANTES del modelo.
//!
//! El texto de la evidencia es de un tercero, y un tercero puede meter
//! un secreto en un README o en la descripcion de un SBOM. Lo que sale
//! hacia el modelo **se redacta**, y lo redactado NO entra en la traza:
//! la traza lleva identificadores, no contenido.

/// Las formas de secreto que se redactan, con su prueba.
///
/// La lista es deliberadamente corta y de senal alta: cada patron que
/// se anade es un patron que puede redactar de mas, y redactar de mas
/// tambien es esconder.
const PATTERNS: &[( &str, &str )] = &[
    // Claves de acceso de AWS: AKIA seguido de 16 mayusculas/digitos.
    ("AKIA[0-9A-Z]{16}", "aws-access-key"),
    // Tokens de GitHub: ghp_/gho_/ghs_ seguidos de 36 caracteres.
    ("gh[pos]_[A-Za-z0-9]{36}", "github-token"),
    // Un token Bearer en cabecera.
    ("(?i)bearer[ \\t]+[A-Za-z0-9\\-\\._~\\+/]+=*", "bearer-token"),
    // Un JWT: tres segmentos base64url separados por punto.
    ("eyJ[A-Za-z0-9_\\-]{10,}\\.[A-Za-z0-9_\\-]{10,}\\.[A-Za-z0-9_\\-]{10,}", "jwt"),
    // Una cadena hexadecimal larga (>= 32): firma, resumen o secreto.
    ("\\b[0-9a-fA-F]{32,}\\b", "hex-secret"),
];

/// Redacta lo que parezca un secreto en el texto de una evidencia.
///
/// Lo que sale de aqui es lo que ve el modelo, y lo mismo tendria que
/// ver la traza si la traja llevara contenido -- y no lo lleva: la traza
/// lleva identificadores de evidencia, no texto.
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
