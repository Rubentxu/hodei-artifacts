# Operación del almacén de contenido — contrato de recolección

**Tarea:** HSP-104 (HAR-10) · **Servicio medido:** `hodei-artifacts 0.2.0`

## 1. Qué afirma este contrato

Afirma que el servicio sabe **qué bytes del almacén de contenido nadie
nombra** y que puede borrarlos sin tocar los que alguien nombra, y dice
también lo que **no** hace. La recolección es la única superficie que borra
bytes, y por eso es la más acotada: no tiene contadores de referencias
propios, no decide por su cuenta y no borra si no está seguro.

Las reglas, una por una:

- **Las raíces salen de las fuentes reales.** Cada recolección lee el
  catálogo (assets, versiones, referencias y referrers por
  cuenta/repositorio) y los manifiestos del propio almacén (de cada
  manifiesto se leen la configuración y las capas, o los hijos de un
  índice; el `subject` es una raíz propia del catálogo). No existe
  un contador aparte que pueda desviarse de ellas.
- **Un estado desconocido no borra.** Si el catálogo no responde, si el
  almacén no se puede listar o si los bytes de un manifiesto no se pueden
  leer, la respuesta es `503` y no se borra nada. Cada pasada hace dos
  censos: el segundo decide, y lo que ya no es reclamable se omite.
  Un manifiesto es el asset `manifests/<su digest>`; si sus bytes no se
  entienden como manifiesto, no aporta aristas (se avisa en el log) y su
  propio contenido sigue protegido por la raíz que lo nombra, de modo que
  un fichero crudo con esa ruta no puede dejar el servicio en `503`.
- **El plan es la configuración por defecto.** `GET /api/v1/gc/plan` y
  `POST /api/v1/gc/sweep` sin `"execute": true` son la simulación: explican,
  blob a blob, qué se conserva y por qué, y no escriben.
- **Gracia y barrera.** Un blob sin referencias solo es reclamable cuando
  lleva más de `content.gc_grace_secs` sin tocarse (por defecto 86 400 s;
  variable `HODEI_CONTENT__GC_GRACE_SECS`). Una subida que deduplica contra
  un blob existente lo «toca», y el borrado vuelve a mirar la fecha bajo el
  mismo cerrojo que la subida: una subida concurrente y un borrado se
  ordenan, no se pisan.
- **Marca y barrido durables.** La pasada se registra en el catálogo
  (marca, luego barrido) y es reanudable e idempotente: tras un `kill` se
  continúa la corrida marcada, y un blob ya borrado no se borra dos veces.
- **Retenciones.** Una retención (`hold`) protege un digest, un repositorio
  o una cuenta, con fecha de caducidad opcional, y el plan nombra la
  retención que protege cada blob.
- **Traza.** Cada borrado deja un tombstone por digest y un evento de
  auditoría con identificador determinista (`gc-delete-<corrida>-<hex>`).

## 2. Qué se ha medido

Las pruebas que respaldan el contrato, y lo que cada una comprueba:

| Prueba | Qué comprueba |
| --- | --- |
| `crates/hodei-hub/tests/hsp104_reclaim_contract.rs` | El almacén de bytes: listado, fecha de contenido, borrado condicionado por la gracia bajo el cerrojo, y la subida que toca el blob existente. |
| `crates/hodei-catalog-storage/tests/hsp104_gc_ledger_contract.rs` | El libro de la recolección sobre el motor real: corridas, tombstones (intención y confirmación) y retenciones con caducidad. |
| `src/gc/plan.rs` (pruebas unitarias) | El plan puro: veredictos, razones, orden, y que lo desconocido no se borra. |
| `tests/hsp104_gc_service.rs` | El servicio con el almacén real y un libro de recolección simulado en memoria (para provocar los cortes): dos capas y dos repositorios sobre un mismo blob, retenciones, gracia, corte entre marca y barrido, reanudación y barrido único. |
| `tests/hsp104_gc_e2e.rs` | El proceso real por HTTP, con tokens y la política administrativa: plan, retención, barrido, permisos por ruta y `kill`/reinicio real. |

### GC-01 — un blob compartido no se borra

Un mismo contenido es un fichero de un repositorio, un fichero de otro y
la capa de una imagen OCI. Mientras exista alguna de esas raíces, el plan
lo marca `retained` con **todas** sus razones, el barrido no lo toca y el
manifiesto de la imagen sigue sirviéndose después de barrer.

### GC-02 — subida en curso, retención y versión fijada no se reclaman

- Un blob subido hace menos de la gracia está `in_grace`; el plan lo dice.
- Un blob con una retención está `retained` y la razón es la retención, con
  su identificador y su motivo.
- Una retención de repositorio o de cuenta protege lo que ese ámbito
  referencia.

### GC-03 — el huérfano se elimina una sola vez, con permiso explícito

Un blob que nadie nombra y que ha pasado la gracia sale `reclaimable` en el
plan, con su tamaño. `POST /api/v1/gc/sweep` con `{"execute": true}` lo
borra y lo audita; una segunda pasada responde `deleted: 0`. La
autorización es **por ruta**: una identidad que puede leer el plan puede no
poder barrer, y la prueba lo comprueba.

### GC-04 — corte entre marca y barrido

A nivel de servicio se fabrican los estados de un corte entre la marca y el
barrido (no se inyecta un fallo en pleno flujo); al volver, la corrida marcada se reanuda y no se pierde nada ni se
borra dos veces. En el proceso real se mata con `SIGKILL` después de un
barrido: tras reiniciar, las retenciones siguen, el plan sigue explicando y
una nueva pasada no repite lo hecho.

## 3. La superficie

| Ruta | Qué hace |
| --- | --- |
| `GET /api/v1/gc/plan` | La simulación: veredicto y razones por blob. No escribe. |
| `POST /api/v1/gc/sweep` | Sin `"execute": true`, la misma simulación; con él, borra. |
| `GET /api/v1/gc/holds` | Las retenciones registradas, activas o caducadas. |
| `POST /api/v1/gc/holds` | Pone una: `scope` (`digest`, `repository` o `account`), `reason` y `expires_at` opcional. |
| `POST /api/v1/gc/holds/release` | Suelta una por su identificador. |

Cada ruta es un recurso distinto para la política administrativa, así que
una política puede conceder el plan sin conceder el barrido. Con el
servicio sin catálogo duradero o con un almacén que no sabe reclamar, las
rutas responden `501`; un estado desconocido, `503`; una pasada ya en
curso, `409`.

## 4. Lo que no se promete

Un límite que no está escrito es un límite que nadie ha decidido.

- **Hoy lo reclamable son huérfanos.** El registro no tiene borrado de
  assets ni de manifiestos (responde `405`), así que no se liberan raíces:
  lo que se reclama son los bytes que un fallo dejó sin nombre (por ejemplo,
  un corte entre escribir el contenido y confirmar el catálogo). Las
  retenciones de repositorio o de cuenta importarán cuando existan los
  retiros de referencias.
- **La gracia debe superar la latencia de publicación.** Es la barrera
  contra una publicación que ha escrito los bytes y aún no ha confirmado el
  catálogo. Un valor de segundos solo es para pruebas.
- **El montaje de blob no «toca».** El `mount` de OCI dentro de una cuenta
  reutiliza el blob sin tocarlo. Hoy es seguro porque el origen es una raíz
  y el registro no borra assets; quien añada retiros de referencias deberá
  tocar el blob al montar.
- **Ámbito de digest por cuenta.** `AccountPartitioned` no tiene todavía
  ubicación física y la recolección no lo recorre.
- **Un solo tombstone por digest.** Guarda el último borrado. Una carrera
  entre un blob que vuelve a aparecer y su borrado puede dejar una
  intención sin confirmar: está registrada, es visible y no borra nada.
- **Una retención puesta entre el censo y el borrado no se ve.** La
  ventana es la de una pasada, y la retención vale desde la siguiente.
- **La auditoría es de mejor esfuerzo.** Va por el outbox; el tombstone es
  la traza duradera de lo borrado.
- **La recolección es de plataforma.** Las retenciones no se restringen por
  cuenta; quién puede planificar, barrer o retener lo decide la política
  administrativa, ruta a ruta. El plan y el listado de retenciones nombran
  cuentas, repositorios y rutas de **todas** las cuentas, y la compuerta
  evalúa el recurso con la cuenta de quien llama: conceder estas rutas a un
  administrador de una sola cuenta le enseña lo de las demás. Hay que
  concederlas solo a operadores de la plataforma.
- **Un inquilino puede fijar bytes.** Un fichero crudo en
  `manifests/<su propio digest>` que sea un JSON de manifiesto válido hace
  que los digests que nombra no se reclamen. Falla del lado seguro (retiene)
  y no es reparable hasta que exista el borrado de assets.
- **Una pasada larga puede superar el tiempo de la petición.** El plan y el
  barrido recorren todo el almacén y el router corta las peticiones a los
  30 s. El barrido corre en su propia tarea: el corte de la conexión no lo
  cancela ni suelta el cerrojo, y la auditoría se escribe igualmente; el
  cliente solo pierde la respuesta y puede ver el resultado en el plan.
- **La gracia debe cubrir la inactividad de una subida por trozos.** Al
  publicar, el blob recibe la fecha del commit, así que la gracia empieza
  ahí y no en el último trozo.
- **El corte entre marca y barrido, en el proceso real, es un `kill` entre
  pasadas.** El corte exacto a mitad de pasada se prueba con un fallo
  inyectado a nivel de servicio, no en el proceso.

## 5. Cómo se ejecuta

```bash
cargo test --test hsp104_reclaim_contract --test hsp104_gc_ledger_contract \
  --test hsp104_gc_service --test hsp104_gc_e2e -- --test-threads=1
```

La pipeline registra las cuatro suites (`pipeline.kts`): el contrato del
almacén en la etapa rápida, las dos de catálogo y servicio en la de base
de datos, y el proceso real en la de infraestructura.
