//! La política de sustitución del contenido de una dirección.
//!
//! Un repositorio crudo **hosted** guarda el contenido en una dirección.
//! Publicar en una dirección que ya tiene contenido **sustituye** lo
//! que hay, y sustituir sin más es una decisión que no puede tomar el
//! cliente: dos clientes que publican en la misma dirección se pisan,
//! y el que pierde no se entera.
//!
//! Por eso la decisión no es del cliente: es del **repositorio**, y vive
//! en su configuración **versionada**. Por defecto una dirección publica
//! **una vez**: publicar el mismo contenido otra vez es idempotente, y
//! publicar otro contenido es un conflicto.
//!
//! # Donde vive la decisión
//!
//! En los ajustes del repositorio, bajo la clave `raw.replace`:
//!
//! ```json
//! { "raw": { "replace": "allow" } }
//! ```
//!
//! `allow` reponer. `deny` --o la ausencia de la clave-- no. La ausencia
//! es una decisión: un repositorio que no dice nada es un repositorio que
//! no ha dicho que se pueda reponer, y lo que no se ha dicho no se
//! permite.

use serde_json::Value;

/// Si el contenido de una dirección puede ser sustituido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacePolicy {
    /// No: la dirección publica una vez.
    Deny,
    /// Si: la dirección se puede reponer.
    Allow,
}

/// La clave de la política dentro de los ajustes del repositorio.
const REPLACE_KEY: &str = "replace";

/// El valor que autoriza a reponer.
const ALLOW: &str = "allow";

/// La política de sustitución que dice la configuración del repositorio.
///
/// Lo que no es `allow` es `deny`: un valor desconocido no es un permiso,
/// y un permiso que no se nombra no se concede.
pub fn the_replace_policy(settings: &Value) -> ReplacePolicy {
    let allowed = settings
        .get("raw")
        .and_then(|raw| raw.get(REPLACE_KEY))
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case(ALLOW));
    if allowed {
        ReplacePolicy::Allow
    } else {
        ReplacePolicy::Deny
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_absence_of_the_policy_is_a_refusal() {
        assert_eq!(the_replace_policy(&json!({})), ReplacePolicy::Deny);
        assert_eq!(
            the_replace_policy(&json!({ "raw": {} })),
            ReplacePolicy::Deny
        );
    }

    #[test]
    fn only_allow_replaces() {
        assert_eq!(
            the_replace_policy(&json!({ "raw": { "replace": "allow" } })),
            ReplacePolicy::Allow
        );
        assert_eq!(
            the_replace_policy(&json!({ "raw": { "replace": "deny" } })),
            ReplacePolicy::Deny
        );
    }

    #[test]
    fn an_unknown_value_is_not_a_permission() {
        assert_eq!(
            the_replace_policy(&json!({ "raw": { "replace": "true" } })),
            ReplacePolicy::Deny
        );
        assert_eq!(
            the_replace_policy(&json!({ "raw": { "replace": 1 } })),
            ReplacePolicy::Deny
        );
        assert_eq!(
            the_replace_policy(&json!({ "raw": "allow" })),
            ReplacePolicy::Deny
        );
    }
}
