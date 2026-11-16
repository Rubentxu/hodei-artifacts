# Operación del almacén de contenido — recolección y recuperación

**Tareas:** HSP-104 (HAR-10, recolección) y HSP-115 (HAR-21, recuperación) · **Servicio medido:** `hodei-artifacts 0.2.0`

Este contrato tiene dos partes: la **recolección** (secciones 1 a 5) y la
**recuperación** (sección RECOVERY, al final).

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

---

# RECOVERY — copia, restauración, scrub y reparación

**Tarea:** HSP-115 (HAR-21)

## R1. Qué afirma

Afirma que un operador puede **copiar** el registro con el servicio parado,
**restaurarlo** en una instalación limpia y comprobar que sirve los mismos
bytes, y que puede **detectar** el daño del almacén sin que nada lo repare a
escondidas. Las reglas:

- **Nunca se inventan bytes.** Una reparación solo copia bytes de una copia
  que se ha verificado entera, con el digest comprobado antes de publicarlos.
  Un blob que no está en la copia queda como `not_in_backup`; no se
  reconstruye ni se sustituye.
- **El orden es metadatos, luego bytes.** La copia exporta el catálogo, filtra
  el export y copia cada blob verificando su hash; el manifiesto
  (`manifest.json`) lista cada blob con su digest, su tamaño y el hash de los
  metadatos. La copia se escribe en un directorio temporal hermano y se
  publica con un renombrado atómico: una copia a medias nunca tiene el nombre
  definitivo.
- **Un blob dañado no entra en la copia.** Si al copiar el hash no coincide, el
  blob se excluye y se declara (`excluded`); la orden termina con código `1`.
- **Una copia dañada o de un formato futuro se rechaza entera**, antes de
  escribir nada en el destino.
- **La restauración solo entra en una instalación limpia.** Exige base y
  almacén vacíos; sobre datos existentes se niega y los deja intactos. Al
  terminar hace un scrub final y lo informa.
- **El scrub distingue tres daños:** `corrupt` (el contenido no tiene el
  digest que dice su nombre), `broken_reference` (una raíz nombra un blob que
  no existe) y `orphan` (un blob que nadie nombra). Reutiliza el censo de la
  recolección y reconfirma cada raíz sin blob antes de acusarla. A diferencia
  de la recolección, que ante un manifiesto OCI ilegible no actúa, el scrub
  sigue: un manifiesto ausente o corrupto es justo lo que informa
  (`broken_reference` o `corrupt`), y una prueba de servicio lo comprueba.
- **La reparación es explícita y acotada.** `repair --from` restaura desde la
  copia los blobs corruptos o ausentes, deja el corrupto en `quarantine/` y
  informa de `restored`, `quarantined`, `not_in_backup` y `backup_damaged`. Un
  huérfano no se repara: lo retira la recolección, con su gracia.

## R2. Qué se ha medido

`tests/hsp115_recovery_e2e.rs` ejecuta el binario `hodei-recovery` y el
servicio **reales** como procesos, con `curl` y `sha256sum` como clientes, y
una base RocksDB real:

| Caso | Qué comprueba |
| --- | --- |
| REC-01 | Copia, `verify`, restauración en instalación limpia y un servicio nuevo que sirve los tres contenidos con el mismo sha256. |
| REC-02 | El scrub encuentra, a la vez, un blob corrupto, una referencia rota y un huérfano, y el informe los nombra uno a uno. |
| REC-03 | La reparación restaura el corrupto desde la copia y lo deja en cuarentena; un blob publicado **después** de la copia sale `not_in_backup` y sus bytes no se inventan. |
| REC-04 | Una copia de un formato futuro y una copia con un blob alterado se rechazan antes de escribir nada en el almacén de destino, y `repair` no repara desde una copia dañada. |
| REC-05 | Una restauración sobre datos existentes se niega y los deja intactos. |
| REC-06 | `kill -9` en mitad de una subida: el scrub queda sano, la copia no contiene el blob a medias, y tras restaurar se sirve lo publicado y se responde `404` a lo cortado. |
| HTTP | `GET /api/v1/scrub` sobre el servicio vivo informa del daño y no cambia nada. |

Las pruebas unitarias cubren el manifiesto (`manifest.rs`), el veredicto puro
del scrub (`scrub.rs`), la copia verificada (`backup.rs`) y el filtro del
export (`export.rs`).

### Un defecto del export de SurrealDB 2.7, corregido

El `db.export` de SurrealDB 2.7.0 emite, para un campo `array<T>`, la
definición del campo y la de su elemento implícito (`campo[*]`), y su propio
`import` falla después con «already exists». Una restauración real lo
destapó con `gc_run.candidates`. La copia lo corrige en `src/recovery/export.rs`:
retira la definición implícita solo si la del campo padre la implica y
conserva byte a byte el resto (también los saltos `\r\n`). Como el filtro solo
conoce las formas que conoce, la copia **ensaya el import** del export ya
filtrado en una base en memoria que se descarta: si no se importa, la copia
no se publica. El hash del manifiesto es el del fichero ya filtrado.

### RPO y RTO medidos (UAT local)

Medidos con el e2e (`--nocapture`), con tres blobs de 20 a 26 KB, base
RocksDB y el binario en modo debug, sobre una máquina de desarrollo:

| Métrica | Valor medido | Qué incluye |
| --- | --- | --- |
| Copia | ~1,8 s | Proceso entero: abrir la base, exportar, filtrar, ensayar el import en memoria, copiar y verificar tres blobs, fsync y renombrado. |
| RTO (restauración) | 313 ms en la orden; ~2,0 s con el proceso | Verificar la copia, importar, copiar los blobs y el scrub final. No incluye arrancar el servicio. |
| RPO | el intervalo entre copias | La copia es un instante: lo escrito después no está en ella. REC-03 lo demuestra con un blob posterior. |

**Cómo leerlas.** Es un fixture pequeño: sirve para fijar el orden de
magnitud del coste fijo (abrir la base, ~1,5 s) y para detectar regresiones,
no para prometer un tiempo de restauración de un despliegue real. El coste
real crece con los bytes del almacén (se leen y escriben una vez y se
verifican con un hash) y con el tamaño del catálogo: **hay que medirlo en
el despliegue**, con `hodei-recovery restore`, que imprime `elapsed_ms`.

## R3. La superficie

| Orden | Qué hace |
| --- | --- |
| `hodei-recovery backup --to DIR` | Copia coherente: catálogo y bytes. Con `--json` emite el informe y los tiempos. |
| `hodei-recovery verify DIR` | Comprueba el manifiesto, los metadatos y cada blob de la copia. No escribe. |
| `hodei-recovery restore --from DIR` | Restaura en una instalación limpia y hace un scrub final. |
| `hodei-recovery scrub` | Informa de blobs corruptos, referencias rotas y huérfanos. No repara. |
| `hodei-recovery repair --from DIR` | Verifica la copia, hace scrub, restaura los blobs reparables y vuelve a hacer scrub. |
| `GET /api/v1/scrub` | El mismo scrub sobre el servicio vivo; ruta administrativa propia. `501` sin scrub, `409` si ya hay uno en curso, `500` genérico en error (el detalle va al log). |

Los valores por defecto (`--db`, `--cas`, `--namespace`, `--database`,
`--grace-secs`) salen de la configuración del servicio. Código de salida:
`0` sano, `1` daño encontrado o blobs excluidos, `2` error.

## R4. Lo que no se promete

- **No hay copia en caliente.** `backup`, `restore` y `repair` abren la base
  y RocksDB la bloquea: **el servicio debe estar parado**. La orden lo dice
  si no puede abrirla. `scrub` por HTTP sí corre con el servicio vivo.
- **No hay alta disponibilidad ni replicación.** Lo que hay es recuperación
  desde una copia; el RPO es el intervalo entre copias y el RTO es el de
  parar, restaurar y arrancar. La replicación se estudia aparte.
- **La restauración solo entra en una instalación limpia.** No hay fusión con
  datos existentes ni restauración parcial de un repositorio o una cuenta.
- **Una copia por directorio, sin rotación ni cifrado.** Dónde se guarda, con
  qué retención y con qué cifrado lo decide el operador.
- **El scrub completo lee todo el almacén, y cada blob entero en memoria.** La
  verificación de un blob lo lee de una vez: un blob de varios GB puede
  agotar la memoria del servicio vivo. En el servicio solo corre un scrub a
  la vez y no se programa solo; para almacenes con blobs muy grandes, use la
  orden `hodei-recovery scrub` con el servicio parado y memoria suficiente.
- **El ensayo del import cuesta memoria** en proporción al catálogo.
- **Una restauración interrumpida no se reintenta sobre lo que dejó.** El
  import de metadatos va antes que la copia de blobs; si falla una copia por
  E/S, la base ya tiene datos y el reintento se niega (`not clean`). El
  operador borra el destino parcial y repite; la copia de origen no cambia.
- **La copia no anota lo que ya estaba mal en el origen.** Un blob que
  desaparece mientras se copia no se lista, y una referencia del catálogo sin
  blob no consta en el manifiesto: la detecta el scrub (haga `scrub` antes de
  confiar en una copia, y el scrub final de `restore` lo repite).
- **El hash del manifiesto da integridad, no autenticidad.** Una copia
  manipulada con su hash recalculado se acepta, y el import ejecuta el
  SurrealQL del fichero. Guarde las copias donde solo escribe el operador.
- **Un huérfano no es un daño que se repare.** El scrub lo informa; lo retira
  la recolección (con su gracia y su permiso explícito).
- **Una referencia rota solo se repara si la copia tiene el blob.** Si no lo
  tiene, queda informada: no hay forma honesta de recuperar esos bytes.
- **La restauración cruzada de versiones** se limita al `format` del manifiesto
  (hoy `1`): una copia con un formato mayor se rechaza; no hay migración de
  esquema dentro de la restauración.
- **El filtro del export es específico de SurrealDB 2.7.** Si una versión
  futura corrige o cambia el export, el filtro falla alto en vez de adivinar;
  entonces hay que revisarlo.

## R5. Cómo se ejecuta

```bash
cargo test --test hsp115_recovery_e2e -- --nocapture
cargo test -p hodei-artifacts-api --lib recovery
```

La pipeline registra la suite `hsp115_recovery_e2e` en la etapa de
infraestructura (`pipeline.kts`), junto al proceso real de la recolección.
