//! El evento de auditoría: lo que se dice de una operación.
//!
//! El evento es una afirmación sobre una operación, y por eso lleva lo que
//! la hace comprobable: **quién** actuó, **en qué cuenta**, **qué** hizo,
//! **sobre qué** recurso, **con qué contenido**, **cómo acabó** y **con
//! qué correlación**. Lo que nunca lleva es una credencial: un evento que
//! dice quién actuó no necesita decir con qué se autenticó, y si lo
//! dijera, el rastro se convertiría en un secreto más que hay que
//! proteger.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// La cota de un campo de texto del evento.
///
/// Los campos de texto del evento los pone quien llama, y van tanto a una
/// fila **durable** como a una línea de log. Sin cota, un principal al que
/// se le deniega una publicación puede escribir filas arbitrarias e
/// ilimitadas en el outbox, y un salto de línea en el `resource` es una
/// inyección en el log.
pub const MAX_FIELD_LENGTH: usize = 256;

/// Limpia un texto que va al evento o al outbox.
///
/// Quita los caracteres de control —un salto de línea en un log es una
/// inyección— y corta lo que se pasa de la cota. La cuenta de caracteres
/// es de caracteres, no de bytes, así que el texto no queda partido por la
/// mitad de un UTF-8.
///
/// **Esto no es un saneamiento de secretos.** El evento no lleva
/// credenciales porque el código no las mete, no porque aquí se limpien.
pub fn sanitize_text(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_FIELD_LENGTH)
        .collect()
}

/// Cómo acabó la operación que el evento describe.
///
/// Los cuatro resultados son cuatro hechos distintos y no se confunden: un
/// rechazo no es un fallo, y un conflicto no es un rechazo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    /// Se permitió y la operación se completó.
    Allowed,
    /// La autorización lo denegó y no se escribió nada.
    Denied,
    /// Ya había algo con otro contenido y no se sobrescribió.
    Conflict,
    /// Algo falló y la operación no se completó.
    Failed,
}

impl std::fmt::Display for AuditResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Conflict => "conflict",
            Self::Failed => "failed",
        })
    }
}

/// Quién actuó, en qué cuenta y con qué correlación.
///
/// Las tres cosas viajan juntas porque siempre viajan juntas: la cuenta es
/// el **tenant verificado**, y no se deduce de la petición sino del
/// principal que la máquina ya verificó. Una cuenta que se deduce es una
/// cuenta que se puede repartir.
///
/// El **actor** es el HRN de quien actuó. En la capa HTTP sale del
/// principal verificado, así que está verificado. En el catálogo lo declara
/// quien publica, porque el catálogo verifica la **cuenta** contra el
/// autorizador y no el subject: por eso la palabra "verificado" no se usa
/// aquí, y quien publique tiene que poner el actor que de verdad actuó.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditContext {
    /// El actor: el HRN del subject.
    pub actor: String,
    /// La cuenta verificada del actor: el tenant.
    pub tenant: String,
    /// La correlación de la petición.
    pub correlation_id: String,
}

impl AuditContext {
    /// El contexto de una operación.
    pub fn new(
        actor: impl Into<String>,
        tenant: impl Into<String>,
        correlation_id: impl Into<String>,
    ) -> Self {
        Self {
            actor: sanitize_text(&actor.into()),
            tenant: sanitize_text(&tenant.into()),
            correlation_id: sanitize_text(&correlation_id.into()),
        }
    }
}

/// Un evento de auditoría.
///
/// Nunca lleva un bearer token ni una credencial: solo identidades, hechos
/// y el resultado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// El identificador del evento.
    ///
    /// Es la **clave de idempotencia**: reentregar este evento es entregar
    /// el mismo evento, y un sink que lo trate así no duplica el efecto.
    pub event_id: String,
    /// Quién actuó, en qué cuenta y con qué correlación.
    pub context: AuditContext,
    /// Cuándo ocurrió.
    pub timestamp: DateTime<Utc>,
    /// Qué se hizo: `publish`, `create_repository`, `move_tag`…
    pub action: String,
    /// Sobre qué se hizo: el recurso canónico.
    pub resource: String,
    /// El digest del contenido, cuando la operación trata de contenido.
    ///
    /// Va como texto (`algoritmo:valor`) para que el evento no dependa del
    /// modelo de ningún dominio: el evento dice qué contenido se movió, no
    /// cómo lo representa el catálogo.
    pub digest: Option<String>,
    /// Cómo acabó.
    pub result: AuditResult,
}

impl AuditEvent {
    /// El evento, con el identificador y el momento tomados de las costuras
    /// que los producen.
    ///
    /// El momento y el identificador son **hechos de la máquina**, no de la
    /// operación: tomarlos de una costura es lo que permite fijarlos en una
    /// prueba y comparar dos ejecuciones de la misma operación.
    ///
    /// Los textos van **limitados**: los pone quien llama, y van a una fila
    /// durable y a una línea de log. Ver [`sanitize_text`].
    pub fn new(
        event_id: impl Into<String>,
        context: AuditContext,
        timestamp: DateTime<Utc>,
        action: impl Into<String>,
        resource: impl Into<String>,
        digest: Option<String>,
        result: AuditResult,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            context,
            timestamp,
            action: sanitize_text(&action.into()),
            resource: sanitize_text(&resource.into()),
            digest: digest.map(|value| sanitize_text(&value)),
            result,
        }
    }

    /// El actor que actuó.
    pub fn actor(&self) -> &str {
        &self.context.actor
    }

    /// La cuenta verificada en la que actuó.
    pub fn tenant(&self) -> &str {
        &self.context.tenant
    }

    /// La correlación de la operación.
    pub fn correlation_id(&self) -> &str {
        &self.context.correlation_id
    }

    /// Si dos eventos dicen el mismo hecho.
    ///
    /// El instante **no** se compara: un reloj no conserva necesariamente la
    /// misma precisión al pasar por un almacén, y dos expresiones del mismo
    /// hecho dejarían de serlo sólo por eso. Lo que decide es quién, en qué
    /// cuenta, qué hizo, sobre qué, con qué contenido y cómo acabó.
    pub fn same_fact(&self, other: &AuditEvent) -> bool {
        self.context == other.context
            && self.action == other.action
            && self.resource == other.resource
            && self.digest == other.digest
            && self.result == other.result
    }
}
