# Eventos de auditoría de las mutaciones

El borde de efecto de creación emite un evento estructurado con identidad, acción, recurso, resultado y correlación. El evento **nunca** contiene el bearer ni credenciales.

Quedan auditadas dos superficies: la creación de repositorios, que audita desde el principio, y las mutaciones de la configuración de autorización —alta, modificación y baja de políticas IAM—, que **no** se auditaban y dejaron de dejar rastro. Una mutación de la configuración de autorización es una mutación crítica por definición: decide quién puede hacer qué, y sin rastro no hay forma de reconstruir quien la hizo.

## Evento

`AuditEvent`:

| Campo | Ejemplo |
| --- | --- |
| `event_id` | UUID del evento; es la clave de idempotencia |
| `context.tenant` | la cuenta **verificada** del actor |
| `context.actor` | HRN del subject verificado |
| `context.correlation_id` | `x-request-id` validado o UUID generado |
| `timestamp` | UTC |
| `action` | `Artifacts::Action::"CreateRepository"`, `CreatePolicy`, `publish`, `UpdatePolicy`, `DeletePolicy` |
| `resource` | HRN canónico del repositorio, o `repositorio/ruta` en una publicación |
| `digest` | el digest del contenido, cuando la operación trata de contenido |
| `result` | `allowed` / `denied` / `conflict` / `failed` |

- `allowed`: se persistió una entidad nueva.
- `denied`: la autorización devolvió `Deny` explícito; no se escribió.
- `conflict`: ya existía; no se reemplazó.
- `failed`: `Indeterminate`, error del motor de autorización o fallo del writer; no se completó.
- Los errores de validación del body (400) ocurren antes del borde de efecto y no emiten evento; el 401 no tiene actor verificado.
- Sin token no hay actor verificado y no se emite evento de operación (el `401` se rechaza antes).

El contrato vive en el crate `hodei-audit`, que lo usan el catálogo y la aplicación sin duplicarlo: un evento con dos definiciones es un evento que puede tener dos formas. El **tenant** del evento es la cuenta verificada del principal, no la que declara el llamante.

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

El token se consume en `verify_principal` y no se pasa al evento ni a `record_audit`. El evento solo contiene identidades, hechos y el resultado: HRNs, la cuenta verificada, la acción, el recurso, el digest y la correlación. Los tests serializan el evento y comprueban que no aparece `Bearer` ni el token.

## Sink y durabilidad

- `AuditSink` es el puerto; `TracingAuditSink` es el sink por defecto y escribe campos estructurados en `tracing`. Es síncrono: un adaptador durable (Audit Trail) debe aportar su propio buffering/async.
- **El outbox de auditoría** (`catalog_audit_outbox`) es donde espera el evento. La publicación escribe la fila del evento **dentro de la misma transacción** que escribe el asset, así que el asset y su evento tienen **una sola frontera de commit**: si la fila del evento no se puede escribir, el asset no se publica. No queda un hecho sin su rastro.
- El `AuditDispatcher` reclama un evento pendiente (un compare-and-set: dos workers, uno gana), lo entrega al sink y **solo entonces** lo marca como entregado. **Solo el worker que lo tiene puede marcarlo**: un lease que expiró ya no es un permiso para decir nada de ese evento, y sin esa condición un worker lento podría devolverlo al mercado mientras el otro lo está entregando. Si el sink falla, el evento vuelve a `pending` con su contador de intentos y su último error: lo que no se entregó se puede **mirar**. Nunca se marca entregado lo que no se entregó.
- **La reentrega es el mismo evento.** El `event_id` es la clave de idempotencia y el contrato del sink exige tratarlo como tal: una caída entre la entrega y la marca vuelve a entregar el mismo evento, no crea otro. Lo que el outbox garantiza es **al menos una entrega**, no exactamente una: el **efecto lógico de un evento es del almacén de auditoría**, que es el que guarda por ese `event_id`. El outbox pone la condición para que pueda haber uno solo —la misma clave y la misma fila—; el que lo asegura es el que guarda.
- **El límite de la auditoría está escrito.** Una petición **mal formada** se rechaza antes de la frontera de efecto y **no deja evento**: un repositorio vacío, una ruta, un nombre o una versión vacíos, o un digest mal formado, no son hechos que registrar. Una petición **bien formada a la que el sistema se niega a servir sí deja evento** (`failed`): un repositorio que no existe, un error de lectura, un modo no servido o una sonda del blob que falla son hechos que ocurrieron.
- **Un `event_id` con otro contenido es un conflicto, no un silencio.** Si el mismo `event_id` llega con un hecho distinto, el outbox no conserva la fila vieja en silencio: lo dice y lo rechaza.
- **El texto del evento está acotado.** El actor, la cuenta, la correlación, la acción, el recurso y el digest pasan por `sanitize_text`, que quita los caracteres de control y corta en 256 caracteres: el que llama no decide cuánto ocupa el evento.
- **Garantía real:** el sink por defecto sigue siendo **best-effort** respecto al proceso: la fila del outbox sobrevive al reinicio, pero el `tracing` no. Si `record` falla, la operación no se revierte; se emite un `warn` con la correlación.
- Una futura integración con Audit Trail implementa `AuditSink` y se inyecta con `AppState::with_audit_sink`; no cambia reglas de autorización, validación ni creación. La durabilidad/retry es responsabilidad de ese adaptador.
- `InMemoryAuditSink` es para tests/verificación local, y deduplica por `event_id`: entregar dos veces el mismo evento es un evento. `FailingAuditSink` prueba que un fallo de auditoría no altera la decisión (pero deja el evento sin persistir).
- El ledger de Evidence tiene **otro propósito** y no se confunde con esta autoridad: el ledger es evidencia de hechos; el outbox es el registro de las mutaciones y de su entrega.

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

`crates/hodei-audit` (10 tests unitarios) cubre el contrato: el claim es un
compare-and-set, un lease que expira vuelve al mercado, un evento que nadie
reclamó no se puede marcar, un worker que perdió el lease no puede decir
nada de ese evento, y decirlo dos veces es decirlo una.

`crates/hodei-catalog-storage/tests/har03_outbox_contract.rs` (22 tests)
cubre la frontera única de commit y la entrega, contra el motor
persistente:

1. el asset y su evento se escriben en la misma transacción;
2. un asset cuyo evento no se puede escribir **no** se publica, y su blob
   queda sin referencia (reconciliable);
3. una publicación en un repositorio que no existe no deja asset y deja un
   evento `failed`;
4. una petición mal formada no deja evento;
5. el evento lleva actor, cuenta verificada, acción, digest, resultado y
   correlación;
6. el evento no lleva secretos;
7. una reentrega produce **un** efecto lógico;
8. una reentrega lleva el **mismo** `event_id`, medido con un sink que no
   deduplica, que es la única forma de que la prueba diga algo;
9. un evento que contradice a otro con el mismo `event_id` no se traga en
   silencio;
10. el texto del evento está acotado;
11. un worker que perdió el lease no produce un segundo efecto lógico;
12. un sink que falla deja el evento pendiente, observable, con su intento
    contado;
13. un evento que nadie reclamó no se puede marcar como entregado;
14. un worker cuyo lease expiró no puede marcar el evento ni devolverlo al
    mercado;
15. dos workers reclaman el mismo evento y solo uno lo entrega;
16. caída antes del commit: no queda asset, ni referencia, ni evento;
17. caída después del commit: el evento queda pendiente y se entrega una vez;
18. el replay del outbox no duplica assets ni eventos de auditoría;
19. un almacén escrito por un esquema más viejo recibe la tabla del outbox al
    migrar;
20. una publicación idempotente no escribe un segundo evento;
21. una publicación en conflicto deja un evento `conflict`;
22. una publicación denegada deja un evento `denied`.

## Límites

- **El outbox es durable; el sink por defecto no.** La fila del evento sobrevive al reinicio, pero el `tracing` se pierde con el proceso: la entrega durable necesita un `AuditSink` que escriba en el almacén de auditoría (Audit Trail), que todavía no está.
- Las mutaciones de la configuración de autorización y la creación de repositorios **siguen siendo best-effort**: su evento se emite en el proceso y un sink caído lo pierde sin cambiar la respuesta. La frontera única de commit es, de momento, la de la **publicación** del catálogo.
- Un rechazo, un conflicto o una negativa de una publicación se dice en el outbox, pero **no** comparte transacción con el catálogo: no hay mutación que atar. Por eso su evento va solo.
- **El outbox entrega al menos una vez.** El que garantiza un solo efecto lógico es el almacén de auditoría, que guarda por `event_id`; el sink por defecto (`tracing`) no deduplica, porque un log es append-only. La reentrega lleva la misma clave, que es la condición para que el almacén pueda tener un solo efecto, no la garantía en sí misma.
- Si el almacén no responde, el evento de un `failed` tampoco se puede escribir: es el mismo almacén. Se avisa con un `warn` y no se finge nada.
- **El outbox es de la plataforma, no de una cuenta.** `pending()` devuelve los eventos de todas las cuentas, porque el almacén de auditoría que los recibe es de la plataforma y el dispatcher tiene que poder verlos todos. Un sink por cuenta tendría que filtrar por `tenant`, y el puerto no lo hace: hoy no hay tal sink.
- No se envía a Audit Trail remoto; no se implementa E2E de esa plataforma.
- El `x-request-id` es una correlación de traza; se valida y se devuelve en la respuesta para poder unir log/evento. No es una identidad.
- La UAT `uat/UAT-RAW-FOUNDATION.md` (PUB-01..03) que cita la tarea **no existe todavía** en el repositorio: el contrato entregado son los cuatro criterios de aceptación, y la conformidad es HAR-05 (HSP-99).
