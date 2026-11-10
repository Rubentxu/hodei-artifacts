# Eventos de auditoría de CreateRepository

El borde de efecto de creación emite un evento estructurado con identidad, acción, recurso, resultado y correlación. El evento **nunca** contiene el bearer ni credenciales.

## Evento

`AuditEvent`:

| Campo | Ejemplo |
| --- | --- |
| `event_id` | UUID del evento |
| `correlation_id` | `x-request-id` validado o UUID generado |
| `timestamp` | UTC |
| `actor` | HRN del subject verificado |
| `action` | `Artifacts::Action::"CreateRepository"` |
| `resource` | HRN canónico del repositorio |
| `result` | `allowed` / `denied` / `conflict` / `failed` |

- `allowed`: se persistió una entidad nueva.
- `denied`: la autorización devolvió `Deny` explícito; no se escribió.
- `conflict`: ya existía; no se reemplazó.
- `failed`: `Indeterminate`, error del motor de autorización o fallo del writer; no se completó.
- Los errores de validación del body (400) ocurren antes del borde de efecto y no emiten evento; el 401 no tiene actor verificado.
- Sin token no hay actor verificado y no se emite evento de operación (el `401` se rechaza antes).

## Redacción

El token se consume en `verify_principal` y no se pasa al evento ni a `record_audit`. El evento solo contiene HRNs y el resultado. Los tests serializan el evento y comprueban que no aparece `Bearer` ni el token.

## Sink y durabilidad

- `AuditSink` es el puerto; `TracingAuditSink` es el sink por defecto y escribe campos estructurados en `tracing`. Es síncrono: un adaptador durable (Audit Trail) debe aportar su propio buffering/async/outbox; este slice no lo hace.
- **Garantía real:** el sink por defecto es **best-effort**, no durable. Si `record` falla, la operación no se revierte; se emite un `warn` con la correlación.
- **CreateRepository es una mutación crítica** y necesitará entrega durable (outbox o equivalente en el almacén real) cuando se introduzca la persistencia SurrealDB. Hasta entonces, la garantía es “evento emitido en el proceso, sin outbox”.
- Una futura integración con Audit Trail implementa `AuditSink` y se inyecta con `AppState::with_audit_sink`; no cambia reglas de autorización, validación ni creación. La durabilidad/retry es responsabilidad de ese adaptador.
- `InMemoryAuditSink` es para tests/verificación local; `FailingAuditSink` prueba que un fallo de auditoría no altera la decisión (pero deja el evento sin persistir).

## Pruebas

`tests/audit_events.rs` (7 tests):

1. create permitido emite actor/acción/recurso/resultado/correlación y no contiene token;
2. `403` emite `denied` sin escribir;
3. `201` + `409` emiten `allowed` + `conflict` con una sola entidad;
4. sin token no hay evento de operación;
5. un sink que falla no convierte el `201` en error y registra el intento;
6. `failed` se emite cuando el writer falla;
7. la correlación se devuelve en `x-request-id` y coincide con el evento.

## Límites

- No hay outbox ni almacén de auditoría todavía; la durabilidad de la mutación crítica es una deuda explícita.
- No se envía a Audit Trail remoto; no se implementa E2E de esa plataforma.
- El `x-request-id` es una correlación de traza; se valida y se devuelve en la respuesta para poder unir log/evento. No es una identidad.
- La desviación frente a ADR-009 (evento atómico/outbox en mutaciones críticas) es explícita: este slice es best-effort.
