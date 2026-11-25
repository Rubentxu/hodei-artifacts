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

> **Herramienta retirada (HSP-314 F4).** El binario `hodei-recovery`, su
> módulo `backup` y la suite `hsp115_recovery_e2e` ya no existen: operaban
> sobre el motor embebido (SurrealDB sobre RocksDB), que se retiró del producto.
> Esta sección queda como registro de lo que se midió entonces y **no** describe
> una superficie disponible. En el perfil de grafo, la copia es la de Dgraph
> (`deploy/dgraph/README.md`) y la del bucket S3; el scrub sigue disponible por
> HTTP (`GET /api/v1/scrub`).

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

La suite `hsp115_recovery_e2e` (ya borrada) ejecutaba el binario
`hodei-recovery` (ya retirado) y el servicio **reales** como procesos, con `curl` y `sha256sum` como clientes, y
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

Ya no hay e2e de recuperación: la suite `hsp115_recovery_e2e` se borró.
Lo que sigue vivo son las pruebas unitarias del módulo `recovery`:

```bash
cargo test -p hodei-artifacts-api --no-default-features --lib recovery
```

---

# SEARCH — la búsqueda y el detalle de un paquete

**Tarea:** HSP-113 (HAR-19) · **Servicio medido:** `hodei-artifacts 0.2.0`

## S1. Qué afirma

Afirma que se puede **encontrar** lo que hay en el registro sin preguntar a
cada proveedor externo, y que lo que se encuentra es lo que hay. Las
reglas, una por una:

- **El índice es el catálogo.** No hay un motor de búsqueda aparte: las
  filas de `catalog_asset` son lo que se busca, y los índices de la base
  son lo que las hace encontrables. Un segundo índice obligaría a
  sincronizarlo con el catálogo en cada escritura, y la sincronía es
  donde un índice empieza a mentir.
- **La búsqueda pregunta repositorio por repositorio.** Cada consulta
  lleva `account` y `repository`, que son las dos primeras columnas del
  índice `(account, repository, asset_path)`, y las páginas de cada
  repositorio se unen en el adaptador. No es una forma de escribir la
  consulta: es lo que evita las dos trampas del motor que se midieron
  antes de escribir el adaptador (ver S2).
- **La cuenta sale del principal verificado.** Nunca de un campo de la
  petición ni de la consulta. Una búsqueda de otra cuenta no devuelve ni
  nombres ni totales: la cuenta ajena no llega a la consulta.
- **El permiso va por delante.** Cedar decide repositorio por repositorio
  cuáles puede leer el principal, y lo que no puede leer **no sale ni
  cuenta**: un total que contara lo que no se puede leer diría que hay
  algo donde no hay nada que ver. La acción que se pide es
  `DownloadAsset`, la misma que los otros listados.
- **El orden es el del índice que sirve la consulta, y es total.** El
  recorrido y el filtro por repositorio salen en el orden de la ruta; un
  filtro con índice propio —el paquete, el digest, la etiqueta— sale en el
  orden de ese índice, que es el orden que pidió el cliente. La versión
  sola no es uno de ellos: el índice de las coordenadas pregunta antes por
  el nombre del paquete, así que una consulta que sólo da la versión la
  sirve el índice de la ruta. Todos esos índices acaban en la ruta, y
  dentro de un repositorio no hay dos assets con la misma ruta: el orden
  es total y la página no repite ni pierde un resultado entre páginas. En
  el canal `trusted` las filas se ponen también en ese orden: el orden de
  las concesiones es el orden de las decisiones, no el que ve el cliente.
- **El total se cuenta hasta un tope, o no se cuenta.** El motor
  materializa cada fila que cuenta, así que contar una cuenta grande
  cuesta lo que cuesta recorrerla. El total es exacto mientras quepa en
  256 coincidencias —256 caben, 257 no—; cuando no cabe, la respuesta
  **no dice total** y dice `more: true`. Un total que no se ha contado no
  se inventa. El tope se respeta también cuando la página empieza lejos:
  un total por encima del tope no se cuenta.
- **La confianza sale de la concesión, no de la referencia.** El estado que
  la búsqueda muestra se lee de `trust_grant`, que es donde vive la
  decisión y su revocación. Una promoción o una revocación se ven en la
  petición siguiente, sin reescribir nada: no hay un índice que
  actualizar porque no hay un segundo estado.
- **El canal de confianza también es una frontera.** Un repositorio
  servido por el canal `trusted` sólo muestra lo que tiene concesión
  viva, y lo que no la tiene no sale ni se cuenta. Un listado que lo
  mostrara diría que el contenido existe donde la descarga responde que
  no existe. Y una concesión es una por **digest**: dos decisiones pueden
  conceder el mismo contenido, y el contenido se sirve una vez y se
  cuenta una vez, porque una página que lo muestra dos veces dice que hay
  dos contenidos donde hay uno.
- **Un autorizador que no puede decidir no es un autorizador que deniega.**
  Una denegación es una decisión: lo que no se puede leer no sale ni se
  cuenta. Una decisión que no se ha podido tomar no es nada, y la
  búsqueda responde `500` —no una página vacía, que afirmaría que no hay
  nada donde no se ha podido preguntar— y el detalle responde `500`
  también, no un `403`, que diría que el contenido existe.
- **La ausencia de hallazgos no es una confianza.** Un contenido del que
  nadie ha escrito nada está en cuarentena, no es de fiar. Lo que no se
  conoce no se concede, y lo que no se concede no se sirve por el canal de
  confianza.

## S2. Qué se ha medido

| Prueba | Qué comprueba |
| --- | --- |
| `crates/hodei-catalog/src/search.rs` (unitarias) | La consulta pura: filtros, paginación, orden, validación del límite y del desplazamiento, y la lectura del digest y del atributo. |
| `crates/hodei-catalog-storage/tests/hsp113_search_contract.rs` | El puerto contra SurrealDB **real**: por coordenadas, acotada a la cuenta, sin ver un repositorio que el principal no puede leer, página ordenada que no repite ni pierde (también cuando el repositorio es más grande que la ventana, y también cuando el filtro del paquete es el que manda en el orden), la misma búsqueda 25 veces con la misma página, los filtros de atributo, el de etiqueta, el de texto, el de versión sin paquete, que la página diga si hay más, que el total sea exacto en la frontera del tope (255, 256 y 257) y que no se cuente por encima, que el tope se respete con una página que empieza lejos, un repositorio vacío, un texto que no existe, una página del tamaño máximo, y que el canal `trusted` sólo muestre lo que tiene concesión y la revocación lo vuelva a quitar. Y el **orden**, con datos en los que el orden de la ruta y el de las coordenadas no son el mismo: el recorrido sale en el orden de la ruta, el filtro del paquete en el de las coordenadas, el de la versión sola en el de la ruta, y las filas del canal `trusted` en el orden de la consulta y no en el de las concesiones. Y que dos decisiones que conceden el mismo digest sirvan el contenido **una** vez y lo cuenten **una** vez. |
| `tests/hsp113_search_e2e.rs` | La superficie HTTP contra el router de verdad, con dos cuentas: la búsqueda de una cuenta no devuelve ni el nombre ni el total de la otra, un repositorio sin permiso no sale ni cuenta, la paginación y el `more` en la superficie, los filtros de atributo, digest y etiqueta, que un total que no cabe **no** se diga, la vista de detalle enlaza blob, metadatos, lineage, cápsula y decisión, la concesión es lo que dice que un contenido es de fiar, y el canal `trusted` no muestra lo que no tiene concesión ni responde con su detalle. Y que un autorizador que **no puede decidir** responda `500` en las dos superficies, y no una página vacía. |

### Lo que se midió del motor antes de escribir el adaptador

Todo esto se midió contra SurrealDB 2.7 con RocksDB, sobre la misma base,
con `EXPLAIN` al lado de los tiempos. No son suspicions: son medidas, y
son las que decidieron cómo está escrito el adaptador.

| Lo medido | Lo que sale | Qué se hizo |
| --- | --- | --- |
| `campo IN <array>` en el filtro exterior | Devuelve **cero filas de forma intermitente**: 28 de 100 rondas entrelazadas sobre la misma base vinieron vacías. | No se usa. Cada consulta lleva un repositorio, y las concesiones de la página entran por un `OR` de trios (repositorio, algoritmo, digest), que además es la forma que mejor aprovecha el índice `(account, repository, digest_algorithm, digest)`. |
| `repository IN (SELECT VALUE ...)` en el filtro exterior | El plan dice `Fallback: Unsupported subquery`, y el motor lo resuelve **por fila**: contar 2 000 filas tardó 2,9 s. | No se usa. Los repositorios permitidos se preguntan uno por uno y las páginas se unen en el adaptador. |
| `ORDER BY` sobre las filas que coinciden | El plan dice `MemoryOrderedLimit`: trae **todas** las filas que coinciden antes de devolver las veinte primeras. 500 filas de un índice costaron 170 ms solo en ordenar. | La consulta **no lleva `ORDER BY`**. Las filas salen en el orden del índice, y ese es el orden de la página. |
| `count()` con dos campos de igualdad | El motor **materializa cada fila que cuenta**: 0,3 ms por fila, frente a 0,004 ms por fila de un conteo que no toca las filas. | El total se cuenta hasta un tope (256 coincidencias) y, cuando no cabe, no se cuenta. |
| Índice de texto completo | El que ofrece SurrealDB 2.7 no lo usa el planificador (`Iterate Table`), y construirlo costó 18 s para 2 000 filas. | La búsqueda por texto es un recorrido, y el UAT lo dice. |
| Filtro por versión sin el nombre del paquete | El planificador elige el índice del `subject` —que no tiene la versión— y sale un plan `Iterate Index` sobre `(account, repository)`: las filas salen en el orden de ese índice, que no es el de la ruta. | La consulta **lleva la pista del camino** (`asset_path > ''`) cuando ningún filtro tiene índice propio, y el planificador elige entonces el índice `(account, repository, asset_path)`: el plan dice `Iterate Index` con el prefijo y el rango, y las filas salen en el orden de la ruta, que es total. |

El `LIMIT` va como número y no atado, porque el motor no sabe evaluar un
`LIMIT` atado (`Found NONE but the LIMIT clause must evaluate to a positive
integer`). El desplazamiento **no** va al motor: la ventana entera se pide
y el corte se hace en el adaptador, que es donde está el orden.

Los índices de la búsqueda (versión 10 del esquema) son tres: el de las
coordenadas `(account, repository, package_name, package_version, asset_path)`,
el del digest `(account, repository, digest_algorithm, digest, asset_path)` y
el de la etiqueta `(account, tag_name)`. Los dos primeros **acaban en la
ruta**, y esa es la razón de que el orden que sirven sea total: dentro de
un repositorio no hay dos assets con la misma ruta. La versión es la que
lo dice, porque el esquema se declara con `IF NOT EXISTS` y un índice que
ya existe no cambia de forma: una base que aplicó la versión 9, que
creó esos dos índices sin la ruta al final, los vuelve a definir al
arrancar.

### La medida a escala

`crates/hodei-catalog-storage/src/bin/search_benchmark.rs` siembra cien mil
assets (27,7 s de siembra) y mide las búsquedas de verdad contra el motor
real, con los índices y sin ellos. La máquina es la de siempre: seis
núcleos con `nice` e `ionice`.

**Con los índices** (5 rondas cada una, se queda la mediana):

| Búsqueda | p50 | Resultados |
| --- | --- | --- |
| El recorrido | 26,47 ms | 20 |
| Por el repositorio | 18,28 ms | 20 |
| Por el nombre del paquete | 2,47 ms | 4 |
| Por la versión | 1,83 ms | 1 |
| Por el digest | 1,92 ms | 1 |
| Por la etiqueta | 3,00 ms | 4 |
| Por el texto (que aparece muchas veces) | 113,87 ms | 20 |
| Por un texto que no existe | **13 502,34 ms** | 0 |
| La página del desplazamiento 100 | 66,46 ms | 20 |

**Sin los índices de la búsqueda** (2 rondas cada una):

| Búsqueda | Sin índice | Con el índice | Cuánto vale el índice |
| --- | --- | --- | --- |
| El recorrido | 53,70 ms | 26,47 ms | x2 |
| Por el repositorio | 33,55 ms | 18,28 ms | x2 |
| Por el nombre del paquete | 10 248,81 ms | 2,47 ms | **x4 100** |
| Por la versión | 10 959,32 ms | 1,83 ms | **x6 000** |
| Por el digest | 11 730,46 ms | 1,92 ms | **x6 100** |
| Por la etiqueta | 10 820,82 ms | 3,00 ms | **x3 600** |
| Por el texto | 119,07 ms | 113,87 ms | x1 |
| La página del desplazamiento 100 | 102,75 ms | 66,46 ms | x1,5 |

Lo que dicen estas cifras, y son la razón de ser de esta sección:

- **Las búsquedas que un índice puede servir son de milisegundos.** Con
  cien mil assets, encontrar un paquete por su nombre cuesta 2,47 ms, y
  encontrar un digest, 1,92 ms.
- **Los índices valen miles de veces.** Sin ellos, una búsqueda por paquete
  recorre la cuenta entera y tarda diez segundos. El recorrido y la
  búsqueda por repositorio sólo se duplican, porque el `LIMIT` corta el
  recorrido antes; lo que se paga sin índice es la **búsqueda que filtra**.
- **La búsqueda por texto no se quita con índices.** Es la misma con y
  sin ellos (114 ms y 119 ms), porque el filtro no es indexable: el motor
  lo evalúa fila a fila. Y cuando el texto **no** existe, la búsqueda
  recorre las cien mil filas y tarda **13,5 s**: ese es el precio de no
  tener índice de texto completo, y el motivo de que la búsqueda por texto
  sea un recorrido y no una búsqueda.
- **Una página profunda cuesta lo que tiene que recorrer.** La página del
  desplazamiento 100 tarda el doble que la primera, porque el motor tiene
  que saltarse las cien filas anteriores.
- **El total con tope.** En el recorrido de cien mil assets el total no
  cabe en las 256 coincidencias que se cuentan: la respuesta trae
  `total: null` y `more: true`, y tarda lo mismo que si el total se
  contara entero.

## S3. La superficie

| Ruta | Qué hace |
| --- | --- |
| `GET /api/v1/search` | La página de resultados. Los filtros van en la consulta: `repository`, `package`, `version`, `digest` (`algoritmo:hexadecimal`), `tag`, `attribute` (`format=pypi`, `mode=hosted`, `channel=trusted`), `text`, `limit` (1 a 100) y `offset`. `total` es el número de coincidencias, o `null` cuando hay más de las que se cuentan; `more` dice si hay más resultados detrás de la página. La página viene en el orden del índice que sirve la consulta: la ruta en el recorrido, y el del filtro propio cuando el filtro tiene índice. |
| `GET /api/v1/packages/{repository}/{*path}` | El detalle: el blob, los metadatos originales, la línea (a qué cuelga y qué cuelga de él), la Trust Capsule y la decisión. |

La búsqueda **exige** credencial (`401` sin ella) y Cedar decide
repositorio por repositorio. La acción que se pide es `DownloadAsset`,
la misma que piden los otros listados del servicio: un listado dice qué
hay dentro, y un despliegue que quiera que un principal sepa que un
repositorio existe sin saber qué hay dentro no le da esa. Un filtro que
no existe, un filtro repetido, un límite que no cabe, un digest mal
formado o una consulta que no se puede leer son un `400`: una consulta
que no dice lo que dice no se responde con resultados que el cliente no
pidió. El detalle de un repositorio ajeno es un `404` —el mismo que si no
existiera— y el de un repositorio sin permiso, un `403`.

Dos cosas que **no** son una respuesta de permiso:

- **Si el autorizador falla, la búsqueda responde `500`** y no una página
  vacía. Una página vacía afirmaría que no hay nada donde no se ha podido
  preguntar.
- **El canal `trusted` es una frontera de la respuesta, no sólo del
  contenido.** Lo que no tiene concesión viva no sale de la búsqueda ni
  se cuenta, y su detalle responde `404` —no un `403`, que diría que el
  contenido existe—. Si el libro de concesiones no está o falla, la
  respuesta es `503`: no saber no es permiso. Lo mismo vale para la
  búsqueda: sin libro, un repositorio del canal `trusted` es un
  repositorio cuyas concesiones no se pueden saber, y la respuesta es
  `503`, no una página vacía.

## S4. Lo que no se promete

- **La búsqueda por texto es un recorrido.** No hay índice de texto
  completo: el filtro compara el nombre del paquete y la ruta del asset de
  los assets de la cuenta, y el motor lo evalúa fila a fila. Con cien mil
  assets, un texto que no existe tarda 13,5 s, y su coste crece con el
  tamaño de la cuenta (ver S2).
- **El total puede no estar.** Por encima de 256 coincidencias la
  respuesta trae `total: null` y `more: true`: la búsqueda dice que hay
  más y no dice cuántos. Es preferible a un número que no se ha contado.
  En un repositorio del canal `trusted` el total cuenta **sólo lo
  concedido**: lo que no tiene concesión viva no sale ni se cuenta. Y
  tampoco es exacto cuando las concesiones no caben en la ventana: la
  búsqueda deja de preguntar y **la respuesta no dice total**, ni el de
  ese repositorio ni el de la cuenta entera.
- **No hay ordenación por relevancia.** El orden es el del índice que
  sirve la consulta, que es estable y total, no el que "encuentra" el
  cliente.
- **Una revocación mueve la página.** El orden es estable mientras el
  catálogo no cambia. Una revocación no cambia el catálogo, pero cambia
  lo que se sirve: la página de un repositorio del canal `trusted` se
  mueve cuando se revoca una concesión, porque el contenido concedido
  deja de servirse.
- **No hay búsqueda por metadatos de formato.** Los atributos que se
  pueden filtrar son los que el catálogo guarda: formato, modo y canal.
  Los metadatos de un paquete (las etiquetas de un OCI, los classifiers de
  un wheel) todavía no son filas del catálogo.
- **El filtro de etiqueta acota por sus coordenadas.** Una etiqueta que
  no existe es una búsqueda sin resultados, no un error; y una etiqueta
  compartida por más de 64 repositorios se rechaza, porque esa pregunta ya
  no es una búsqueda.
- **El total es el de lo que se puede leer.** No es el total del
  registro, y no debe usarse para saber cuánto hay en la cuenta ajena.
- **Una búsqueda sin filtros devuelve una página de todo lo que se puede
  leer.** Es un recorrido, y quien lo pide lo sabe.
- **La vista de detalle no trae los bytes.** Es una vista: el contenido se
  descarga por la superficie de contenido (raw u OCI), que es la que
  decide el canal de confianza.

## S5. Cómo se ejecuta

```bash
cargo test -p hodei-catalog-storage --test hsp113_search_contract
cargo test --test hsp113_search_e2e
cargo run --release -p hodei-catalog-storage --bin search_benchmark
```

La última orden es la medición a escala, y se toma **a mano**: sembrar cien
mil assets lleva minutos, y la pipeline no paga eso en cada push. Su
tamaño se puede cambiar con `HODEI_SEARCH_ASSETS`.

# CONSOLE — la consola del catálogo

**Tarea:** HSP-114 (HAR-20) · **Servicio medido:** `hodei-artifacts 0.2.0` ·
**Frontend medido:** `frontend 0.0.0`

## C1. Qué afirma

Afirma que existe una **consola** que habla con el catálogo de verdad, y
que lo que enseña es lo que el servicio dice. Las reglas, una por una:

- **La consola no tiene su propia tabla de formatos.** Los formatos y las
  modalidades que se ofrecen salen de `GET /api/v1/capabilities`, que es
  el servicio diciendo qué sirve. Una tabla propia sería una segunda
  verdad, y la segunda verdad se desincroniza: el error se vería al
  mandar el formulario, cuando ya no tiene arreglo.
- **Una modalidad que el formato no sirve no se ofrece.** El OCI sirve
  `hosted` y nada más; el raw, el Maven 2, el npm y el PyPI sirven las
  tres. La API lo rechaza con un `400` antes de escribir nada, y la
  consola no llega a esa situación porque no lo ofrece.
- **La lista es la de la cuenta, y es la que se puede leer.** Cedar decide
  repositorio por repositorio, y lo que no se puede leer no sale ni
  cuenta. Un repositorio de otra cuenta no está en la lista y su nombre
  no se adivina: decir que existe es decir que es de alguien.
- **Las cuatro respuestas que se parecen se distinguen.** Un permiso
  denegado, una ausencia, lo que está guardado y no se puede leer, y lo
  que no se ha podido decidir son cuatro cosas distintas, y la consola
  las dice de cuatro maneras. La API las distingue en el cuerpo
  (`{"error": "...", "message": "..."}`), y el cuerpo es lo que la
  consola lee antes de mirar el estado.
- **La ausencia no dice de quién era.** El detalle de un activo que es de
  otra cuenta es el mismo `404` que el de uno que no existe, y la consola
  no lo convierte en otra cosa: dice "no hay nada aquí", que es lo único
  que se puede decir sin contar de más.
- **La confianza sale de la concesión.** Un activo sin concesión está en
  cuarentena, y la consola lo dice en cuarentena. La decisión que se
  muestra es la **viva**: una concesión retirada no es una concesión, y
  el activo vuelve a estar en cuarentena.
- **Las acciones que cambian lo que se sirve piden antes.** Promover un
  activo al canal de confianza y retirar una concesión cambian lo que ve
  todo el que lee el repositorio, y ninguna de las dos se deshace desde
  lejos. La consola pregunta, dice qué va a pasar, y la retirada pide el
  motivo, que se guarda con ella.
- **El perfil de confianza es una lectura, no un veredicto.** Las
  dimensiones se muestran una por una con su estado. Una dimensión sin
  evidencia **no** es una dimensión satisfecha, y la consola no la
  enseña como si lo fuera.
- **La descarga va a la superficie de contenido con la credencial de la
  persona.** Los bytes no salen por la API, y el enlace no lleva el token
  dentro: el token en una URL acaba en el historial de un navegador.
- **El total que no se ha contado no se dice.** Cuando la búsqueda
  responde `total: null` y `more: true`, la consola dice que hay más, y
  no dice cuántos. Un "0 resultados" sobre una página llena sería una
  mentira.

## C2. Qué se ha medido

| Prueba | Qué comprueba |
| --- | --- |
| `tests/hsp114_console_contract.rs` | La superficie del backend: la lista nombra sólo lo que el principal puede leer, dice el formato, la modalidad y el canal, la cuenta ajena no sale, una decisión que no se puede tomar responde `500` con `error: "indeterminate"` y no una lista vacía, la creación se hace en la modalidad que el formato sirve, el formato que no la sirve se rechaza con `400`, un proxy sin upstream y un grupo sin miembros se rechazan, la creación sin permiso responde `403`, y las capacidades dicen qué se puede crear. |
| `frontend/src/features/console/__tests__/api/errors.test.ts` | El traductor de fallos: la corrupción y la indeterminación se distinguen aunque las dos sean un `500`, el cuerpo manda sobre el estado, el estado es el plan B, y el mensaje nunca nombra un repositorio, una cuenta o un principal. |
| `frontend/src/features/console/__tests__/components/RepositoryCreateDialog.test.tsx` | El formulario guiado por las capacidades: sólo ofrece los formatos que el servicio sirve, las modalidades son las del formato elegido, el proxy pide su upstream, y lo que se manda es lo que la API acepta. |
| `frontend/src/features/console/__tests__/components/BrowseView.test.tsx` | El navegador: un total que no se ha contado se dice como "más de N" y nunca como cero, y los cuatro fallos se dicen como son. |
| `frontend/src/features/console/__tests__/components/PackageDetailView.test.tsx` | El detalle: el digest, el tamaño, la versión y el origen se enseñan; la promoción no llama a la API hasta que se confirma, y la retirada pide un motivo antes de poder continuar. |
| `frontend/tests/e2e/console.spec.ts` | La consola contra el **servicio de verdad**: el mismo binario, el mismo bundle de Cedar y el mismo catálogo que un despliegue, sembrado por la propia API. La lista, las modalidades ofrecidas, la búsqueda, el detalle, la confirmación de la retirada y la descarga de los bytes. |
| `frontend/tests/e2e/accessibility.spec.ts` | El teclado y los nombres: cada campo de la búsqueda tiene nombre, la búsqueda se puede pedir sin ratón, el diálogo toma el foco y se cierra con escape, y los repositorios se alcanzan con el teclado. |

### El fixture

El fixture de la consola (`frontend/tests/e2e/fixture/`) levanta el
servicio de verdad con el bundle de ejemplo del repositorio
(`config/examples/`), con un almacén y un catálogo de usar y tirar, y
**siembra los datos por la API**: crea el repositorio, publica el activo
por la superficie de contenido y lo promueve. Sembrar por la API y no
escribiendo en la base es lo que hace que la prueba mida la consola y no
la siembra.

El fixture anuncia su disponibilidad cuando el catálogo está sembrado, y
no cuando el proceso está en pie: una prueba que empieza contra un
catálogo a medio sembrar mide la siembra.

## C3. La superficie

| Ruta | Qué hace |
| --- | --- |
| `GET /api/v1/catalog/repositories` | Los repositorios de la cuenta que el principal puede leer, con su formato, su modalidad y su canal. |
| `POST /api/v1/catalog/repositories` | La creación de un repositorio del catálogo. El nombre, el formato y la modalidad son obligatorios; los ajustes son los de la modalidad (el upstream de un proxy, los miembros de un grupo). |
| `GET /api/v1/capabilities` | Los formatos que el servicio sirve y las modalidades de cada uno. No hace falta credencial de contenido, pero sí la credencial de la superficie. |
| `GET /api/v1/search` | La página de resultados (ver S3). |
| `GET /api/v1/packages/{repository}/{*path}` | El detalle de un activo (ver S3). |
| `POST /api/v1/promotions` | La promoción de un activo al canal de confianza. |
| `POST /api/v1/promotions/revoke` | La retirada de una concesión, por digest o por decisión, con su motivo. |
| `GET /repository/{repository}/{*path}` | Los bytes. La consola los pide con la credencial de la persona y se los da al navegador. |

La consola vive en tres rutas del propio frontend: `/console/repositories`,
`/console/browse` y `/console/packages/{repository}/{*path}`.

## C4. Lo que no se promete

- **La consola no es una superficie de administración.** No hay gestión de
  usuarios, de políticas ni de tokens: la consola es del catálogo, y lo
  que no es del catálogo no está.
- **La consola no crea lo que la API no sirve.** Y la lista de formatos
  que ofrece es la del despliegue: si el servicio no sirve el npm, la
  consola no lo ofrece, y no se puede forzar por el navegador.
- **La consola no dice más de lo que la API dice.** No cuenta por su
  cuenta, no ordena por relevancia y no inventa un total. Lo que no se
  sabe, no se dice.
- **La consola no sustituye a la CLI.** Los clientes de Maven, npm, pip y
  OCI siguen hablando con sus superficies; la consola es para las personas.
- **El perfil de confianza se lee como está.** Las dimensiones que no
  tienen evidencia se enseñan con su estado y su motivo, y la consola no
  los resume en una nota.
- **Lo que la consola no puede leer, no lo enseña.** Y lo que no puede
  leer no lo enseña tampoco como un hueco: si la decisión no se pudo
  tomar, la consola lo dice, porque un hueco y una ausencia son cosas
  distintas.

## C5. Cómo se ejecuta

```bash
cargo test --test hsp114_console_contract
cd frontend && npm test
cd frontend && npm run type-check
cd frontend && npx playwright test
```

La prueba de extremo a extremo levanta el servicio de verdad y el servidor
de desarrollo, y necesita el binario construido (`cargo build --bin
hodei-artifacts-api`) y el navegador (`npx playwright install chromium`).
El tipo se comprueba sobre el programa completo del frontend
(`tsconfig.app.json`), que incluye los ficheros de prueba: el type-check
que deja fuera a los tests es un type-check que no comprueba. La
configuración del tooling (`vite`, `vitest` y `playwright`) se comprueba
también, en su propio programa (`tsconfig.node.json`).

# INVESTIGACIÓN — la consulta de investigación de un artefacto

## I1. Qué afirma

Afirma que existe una **consulta de investigación** que responde a una
pregunta sobre un artefacto con lo que el servicio sabe de él, y que el
informe se puede comprobar. Las reglas, una por una:

- **La investigación es una lectura.** No promueve, no cambia una política,
  no aprueba una excepción y no escribe una decisión. La superficie tiene
  un método, y ese método solo lee.
- **El orden de las puertas es el contrato.** El principal se verifica, la
  cuenta sale del principal verificado, y Cedar decide sobre el sujeto con
  su propia acción (`Artifacts::Action::"Investigate"`). Lo que se rechaza
  en cualquiera de esos pasos **no lee nada**: no se abre un blob, no se
  consulta el grafo.
- **El sujeto de otra cuenta no está.** Se esconde tras el mismo `404` que
  el sujeto que no existe, y no se evalúa.
- **El informe dice lo que sostiene cada afirmación.** Cada hallazgo lleva
  sus citas, su clase y su estado de verificación, y el manifiesto de las
  lecturas lleva el digest de lo que se leyó, no su contenido.
- **Sin modelo, la pregunta se declara sin responder.** Los hallazgos son
  los que calculan las herramientas, y lo que no se sabe se dice que no se
  sabe. No hay ninguna API comercial en el camino de la petición.
- **Lo que no cabe en el presupuesto se declara omitido.** La cobertura
  dice cuántos hechos se quedaron fuera, y la petición solo puede acortar
  el límite del despliegue, nunca ampliarlo.
- **El tenant del grafo lo declara quien escribe la evidencia.** Es una
  convención del productor, y por eso la plantilla
  (`investigation.graph_tenant_template`) es configuración. Un grafo
  escrito bajo otro nombre no tiene relaciones que mostrar, y el informe
  lo dice en vez de inventarlas.
- **Un informe puede caducar, y se dice.** Un informe es una afirmación con
  fecha: dice qué leyó y en qué versión del mundo. La comprobación es **bajo
  demanda** --el cliente trae de vuelta el manifiesto-- y su respuesta es de
  **tres**: `current` (el mundo no se movió bajo lo que el informe dice),
  `stale` (se movió, y la respuesta dice **qué referencia** se movió) o
  `unknown` (una fuente no se pudo consultar).
- **Lo que no se sabe no se dice vigente.** Un almacén que no contesta no
  dice que el mundo no se movió, y un informe que no dice de qué mundo habló
  no puede comprobarse: se dice que no se sabe, nunca que sigue vigente.
- **Caduca lo que el informe sostiene, y solo eso.** El apoyo de la relación
  que leyó y la evidencia con el digest que leyó. Un cambio ajeno al
  manifiesto **no** invalida el informe: lo que no se leyó no puede caducar
  lo que se leyó.

## I2. Qué se ha medido

| Prueba | Qué comprueba |
| --- | --- |
| `tests/hsp88_investigation_query.rs` | La consulta autorizada devuelve el informe con su sujeto, sus hallazgos, su cobertura y su manifiesto; lo que no está autenticado, lo que es de otra cuenta, lo que no es un artefacto, lo que no es un HRN y lo que no tiene permiso **no llega a leer nada**; una pregunta más larga que el límite se rechaza; la petición solo acorta el presupuesto; la superficie no tiene otros métodos; un despliegue que no la declara no la monta; el informe cita la evidencia con su digest; lo que no cabe se declara omitido; y sin modelo la pregunta se declara sin responder. |
| `crates/hodei-investigator/tests/deterministic_contract.rs` | El dominio sin modelo: los hechos que calculan las herramientas, las incertidumbres que se declaran, el manifiesto que lleva el informe y la traza que dice que no hubo modelo. |
| `tests/hsp89_investigation_freshness.rs` | La comprobación de la frescura: un informe cuyo mundo no se movió sigue vigente; el apoyo que se retiró lo deja obsoleto, diciendo qué referencia se movió; un cambio ajeno al informe no lo invalida; un informe sin versión **no** dice que sigue vigente; y lo que no está autenticado, es de otra cuenta o no tiene permiso **no lee nada**. |
| `crates/hodei-investigator/tests/freshness_contract.rs` | El verificador puro: la respuesta de tres, el cambio ajeno que no invalida, el apoyo ajeno de la misma relación que no invalida, la fuente que no se puede consultar (que es `unknown`, no `current`) y el manifiesto sin versión. |
| `crates/hodei-investigator-storage/tests/freshness_storage.rs` | El comprobador sobre el almacenamiento real: la versión del tenant, los deltas que el grafo escribió, la retirada de un apoyo y el aislamiento de la cuenta. |

## I3. La superficie

| Ruta | Qué hace |
| --- | --- |
| `POST /api/v1/investigations` | La investigación de un artefacto. El cuerpo nombra el sujeto (un HRN canónico de artefacto) y, si se quiere, la pregunta; la respuesta es el informe versionado del investigador. |
| `POST /api/v1/investigations/freshness` | La comprobación de la frescura de un informe. El cuerpo nombra el sujeto y trae de vuelta el manifiesto; la respuesta dice `current`, `stale` (con las referencias que se movieron) o `unknown` (con el motivo). |

La superficie está **cerrada por defecto**: se abre con
`security.investigation_api_enabled = true`, y un bundle que no declara
`Artifacts::Action::"Investigate"` impide arrancar el despliegue. El
límite de la pregunta y el presupuesto de la lectura son los del
despliegue (`investigation.max_question_length`, `investigation.max_evidence`,
`investigation.max_relations`, `investigation.max_bytes`).

## I4. Lo que no se promete

- **La investigación no decide nada.** El informe describe; la confianza
  se sigue decidiendo en la política de confianza y en el motor de
  políticas, no aquí.
- **La investigación no sustituye a la cápsula ni al detalle del
  activo.** Son tres lecturas de la misma evidencia con tres preguntas
  distintas, y ninguna sustituye a las otras.
- **Sin modelo no hay respuesta a la pregunta.** Hay hallazgos y hay
  incertidumbres; la pregunta, sin responder.
- **Un despliegue sin evidencia no inventa evidencia.** El informe dice
  que no hay nada, con la cobertura que lo declara.
- **La frescura no invalida nada por su cuenta.** No hay motor de
  suscripciones, ni cola, ni TTL: la comprobación es bajo demanda, y lo que
  responde es lo que el mundo dice ahora, no una decisión sobre el informe.
- **La frescura no relee el contexto.** Compara el manifiesto con la
  posición del tenant y con los deltas posteriores a la versión leída; no
  vuelve a recuperar la evidencia ni regenera el informe.

## I5. Cómo se ejecuta

```bash
cargo test --test hsp88_investigation_query
cargo test -p hodei-investigator --test deterministic_contract
cargo test --test hsp89_investigation_freshness
cargo test -p hodei-investigator --test freshness_contract
cargo test -p hodei-investigator-storage --test freshness_storage
```
