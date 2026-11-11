# Eventos de auditoría de las mutaciones

El borde de efecto de creación emite un evento estructurado con identidad, acción, recurso, resultado y correlación. El evento **nunca** contiene el bearer ni credenciales.

Quedan auditadas dos superficies: la creación de repositorios, que audita desde el principio, y las mutaciones de la configuración de autorización —alta, modificación y baja de políticas IAM—, que **no** se auditaban y dejaron de dejar rastro. Una mutación de la configuración de autorización es una mutación crítica por definición: decide quién puede hacer qué, y sin rastro no hay forma de reconstruir quien la hizo.

## Evento

`AuditEvent`:

| Campo | Ejemplo |
| --- | --- |
| `event_id` | UUID del evento |
| `correlation_id` | `x-request-id` validado o UUID generado |
| `timestamp` | UTC |
| `actor` | HRN del subject verificado |
| `action` | `Artifacts::Action::"CreateRepository"`, `CreatePolicy`, `UpdatePolicy`, `DeletePolicy` |
| `resource` | HRN canónico del repositorio |
| `result` | `allowed` / `denied` / `conflict` / `failed` |

- `allowed`: se persistió una entidad nueva.
- `denied`: la autorización devolvió `Deny` explícito; no se escribió.
- `conflict`: ya existía; no se reemplazó.
- `failed`: `Indeterminate`, error del motor de autorización o fallo del writer; no se completó.
- Los errores de validación del body (400) ocurren antes del borde de efecto y no emiten evento; el 401 no tiene actor verificado.
- Sin token no hay actor verificado y no se emite evento de operación (el `401` se rechaza antes).

## Mutaciones de la configuración de autorización

Las tres mutaciones de políticas IAM emiten su evento con la misma forma:

| Operación | `action` | `resource` |
| --- | --- | --- |
| Alta | `CreatePolicy` | HRN canónico de la política |
| Modificación | `UpdatePolicy` | HRN canónico de la política |
| Baja | `DeletePolicy` | HRN canónico de la política |

Los resultados se distinguen igual que en el resto:

- `allowed`: la política quedó creada, modificada o borrada.
- `denied`: la operación se rechazó —un principal adjunto de otra cuenta, o una política de otra cuenta— y no se escribió nada.
- `conflict`: la política ya existía.
- `failed`: la validación o el almacenamiento fallaron.

La denegación se registra **antes** de responder, y el evento nombra la política de la que se trata aunque la operación no llegue a escribirse: un rechazo que no deja rastro es indistinguible de una petición que nunca ocurrió.

## Redacción

El token se consume en `verify_principal` y no se pasa al evento ni a `record_audit`. El evento solo contiene HRNs y el resultado. Los tests serializan el evento y comprueban que no aparece `Bearer` ni el token.

## Sink y durabilidad

- `AuditSink` es el puerto; `TracingAuditSink` es el sink por defecto y escribe campos estructurados en `tracing`. Es síncrono: un adaptador durable (Audit Trail) debe aportar su propio buffering/async/outbox; este slice no lo hace.
- **Garantía real:** el sink por defecto es **best-effort**, no durable. Si `record` falla, la operación no se revierte; se emite un `warn` con la correlación.
- **CreateRepository es una mutación crítica** y necesitará entrega durable (outbox o equivalente en el almacén real) cuando se introduzca la persistencia SurrealDB. Hasta entonces, la garantía es “evento emitido en el proceso, sin outbox”.
- Una futura integración con Audit Trail implementa `AuditSink` y se inyecta con `AppState::with_audit_sink`; no cambia reglas de autorización, validación ni creación. La durabilidad/retry es responsabilidad de ese adaptador.
- `InMemoryAuditSink` es para tests/verificación local; `FailingAuditSink` prueba que un fallo de auditoría no altera la decisión (pero deja el evento sin persistir).

## Pruebas

`tests/iam_audit.rs` (3 tests) cubre las mutaciones de la configuración de autorización:

1. las tres mutaciones emiten su evento con actor, acción, recurso y resultado, y con la correlación de la petición;
2. una mutación rechazada emite `denied` y no `failed`, porque un rechazo y un fallo no son lo mismo;
3. el evento no contiene el token ni el contenido de la política.

`tests/audit_events.rs` (7 tests):

1. create permitido emite actor/acción/recurso/resultado/correlación y no contiene token;
2. `403` emite `denied` sin escribir;
3. `201` + `409` emiten `allowed` + `conflict` con una sola entidad;
4. sin token no hay evento de operación;
5. un sink que falla no convierte el `201` en error y registra el intento;
6. `failed` se emite cuando el writer falla;
7. la correlación se devuelve en `x-request-id` y coincide con el evento.

## Límites

- No hay outbox ni almacén de auditoría todavía; la durabilidad de la mutación crítica es una deuda explícita. Medido: el workspace no tiene outbox, y el sink por defecto escribe campos estructurados en `tracing`. Las mutaciones de la configuración de autorización tienen la misma limite que la creación de repositorios: el evento se emite en el proceso, y un sink caído lo pierde sin cambiar la respuesta.
- No se envía a Audit Trail remoto; no se implementa E2E de esa plataforma.
- El `x-request-id` es una correlación de traza; se valida y se devuelve en la respuesta para poder unir log/evento. No es una identidad.
- La desviación frente a ADR-009 (evento atómico/outbox en mutaciones críticas) es explícita: este slice es best-effort.
