# Changelog

Todos los cambios relevantes de este proyecto se registran en este archivo.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y
el proyecto adopta [SemVer](https://semver.org/lang/es/).

Este proyecto está en estado **pre-GA**. Las versiones publicadas hasta la
fecha son *baselines reconciliados*: no declaran paridad con Nexus CE ni
compatibilidad de protocolo completa.

## [No publicado]

Cambios que todavia no estan en una release. El formato sigue
[Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/).

### Changed

- La **búsqueda del catálogo** y la **vista de detalle de un paquete** piden
  la acción `DownloadAsset` y no `ReadRepository`: un listado dice qué hay
  dentro, y es el mismo permiso que piden los otros listados del servicio
  (Maven, npm, PyPI, OCI). Un bundle que no declara `DownloadAsset` ya no
  arranca: las dos superficies están montadas siempre, y sin esa acción no
  podrían decidir.
- Un **autorizador que no puede decidir** ya no se confunde con uno que
  deniega: la búsqueda responde `500` y la vista de detalle también, en vez
  de una página vacía o un `403`. Una página vacía affirmaría que no hay
  nada donde no se ha podido preguntar.
- **Cambio incompatible**: la accion del playground de politicas se expresa
  ahora en la forma canonica `Namespace::Action::"nombre"`
  (`Api::Action::"read"`), que es la que usa el gate. Antes se aceptaba la
  forma `hrn:<particion>:<servicio>::<cuenta>:Action/<nombre>`, y tambien la
  grafia `Namespace::Action::nombre` sin comillas, que el parser canonico
  rechaza.
- Las respuestas de las politicas IAM (`GET`, `POST`, `PUT` y el listado)
  incluyen `created_at` y `updated_at` con las marcas que guarda la base de
  datos. Una fila escrita antes de que existieran las marcas devuelve `null`
  en esos campos, que es la manera de decir que no hay marca, y no la de
  inventar una.
- La CI se segmenta en tres suites -- rapida, de integracion y de
  infraestructura -- y cada una mide su duracion en el log y en el resumen
  del job. La suite rapida es la que responde primero en una pull request.

### Added

- La **consola del catálogo** (`frontend/src/features/console/`): las personas
  ven los repositorios de su cuenta, buscan en lo que hay y miran el detalle
  de cada activo --digest, tamaño, versiones, origen y perfil de
  confianza--, descargan los bytes y promoten o retiran una concesión. La
  consola no guarda su propia tabla de formatos: lo que se puede crear sale
  de `GET /api/v1/capabilities`, y una modalidad que el formato no sirve no
  se ofrece. Las cuatro respuestas que se parecen --permiso denegado,
  ausencia, lo que está guardado y no se puede leer, y lo que no se ha
  podido decidir-- se dicen de cuatro maneras, porque el cuerpo de la API
  las distingue y una consola que las iguala miente sobre tres. Las
  acciones que cambian lo que se sirve a todo el que lee el repositorio
  preguntan antes, y la retirada pide el motivo.
- Los **repositorios del catálogo** (`GET|POST /api/v1/catalog/repositories`)
  y las **capacidades** (`GET /api/v1/capabilities`): la lista nombra sólo
  los repositorios de la cuenta que el principal puede leer, con su formato,
  su modalidad y su canal, y la creación rechaza con un `400` la modalidad
  que el formato no sirve, un proxy sin upstream y un grupo sin miembros.
  Una decisión que no se puede tomar responde `500` con su clase, y no una
  lista vacía.
- La **búsqueda del catálogo** (`GET /api/v1/search`) y la **vista de
  detalle de un paquete** (`GET /api/v1/packages/{repository}/{*path}`): la
  primera busca por repositorio, paquete, versión, digest, etiqueta,
  atributo y texto libre, con página y desplazamiento; la segunda enlaza el
  blob, lo que se publicó, la línea, la *Trust Capsule* y la decisión.
  No hay motor de búsqueda nuevo: el índice es el propio catálogo, y por
  eso lo que promete la búsqueda es lo que hay. La página viene en el orden
  del índice que sirve la consulta, y ese orden es total, así que una
  página no repite ni pierde un resultado entre páginas. La confianza se
  lee de la concesión viva del libro y no de la referencia, de modo que una
  promoción o una revocación se ven en la petición siguiente. El canal
  `trusted` es también una frontera: lo que no tiene concesión viva no sale
  ni se cuenta, y su detalle responde `404`.
- La **conformidad de la superficie de contenido crudo**, de caja negra:
  el binario de verdad en un puerto de verdad, con `curl` de verdad y
  `sha256sum` de verdad. Lo que se mide es que lo publicado vuelve
  **intacto** —con los bytes del tramo y sin cuerpo en el `HEAD`—, que un
  proceso **muerto a mitad de una subida** no deja una ruta publicada a
  medias y que al reiniciar el servicio recoge lo caducado y respeta lo
  vivo, que las subidas **simultáneas** no se mezclan —el arnés comprueba
  que se solaparon de verdad, el mismo contenido es idempotente, las
  rutas distintas guardan lo suyo y la ruta disputada tiene un solo
  ganador— y que dos cuentas con tokens reales **no se ven** entre sí.
  La suite escribe un informe con la versión, el comando real de cada
  caso, el resultado **medido** y los límites, y dice explícitamente lo
  que **no** certifica: ni OCI, ni S3, ni disponibilidad.
- La **recuperación del staging** al arrancar el servicio: una subida
  interrumpida deja su fichero de staging en el almacén de contenido, y
  ese fichero no se publica nunca —la transacción del catálogo no llegó
  a ocurrir—, pero era un fichero que se acumulaba sin que nadie lo
  quitara. El arranque limpia el staging caducado
  (`content.staging_max_age_secs`, una hora por defecto, con un suelo de
  un minuto) y dice cuántos ha quitado. Lo que se queda es el staging
  **vivo**, y lo que lo protege no es su fecha sino el **bloqueo**:
  mientras la escritura vive, su fichero está bloqueado por el proceso
  que escribe, y un fichero bloqueado no se borra, aunque la subida
  lleve horas sin escribir. Un staging que no se puede borrar no impide
  arrancar: se avisa y se sigue. La conformidad comprueba las dos cosas,
  y el staging vivo sobrevive **caducado**, que es la única forma de que
  se vea que lo protege el bloqueo y no la fecha.
- La **superficie de contenido crudo** (`/repository/{repositorio}/{ruta…}`,
  fuera de `/api/v1`): `PUT` publica, `GET` descarga y `HEAD` describe, y
  las tres hablarían con un repositorio `raw` en modo `hosted`. La ruta se
  lee del URI **crudo**, no del decodificado, y sus reglas son estrictas:
  cada segmento se decodifica una vez y tiene que ser un `pchar` de
  RFC 3986 —ni barras, ni barras invertidas, ni el espacio, ni `#` ni
  `?`, ni ser `.` ni `..`—, de modo que un doble codificado es una forma
  que todavía lleva codificación dentro y por eso no es un nombre. Lo
  que se guarda es siempre un nombre literal. El
  contenido se escribe **por trozos** en el CAS, con el digest calculado
  mientras pasa y el commit comprobando que lo publicado es exactamente
  lo declarado. El `ETag` es el digest del contenido; `Range` es RFC 9110
  (`206` con `Content-Range`, `416` con `Content-Range: bytes
  */<longitud>` para lo que no cabe, y un rango en otra unidad se ignora
  y se sirve el contenido entero); `If-None-Match` que coincide al leer
  es `304` —con la comparación débil que manda el RFC, para que un
  cliente que revalida contra una caché reciba el `304` que espera—, y
  una condición que no se cumple al escribir es `412` **sin dejar nada
  escrito** —la condición viaja al catálogo, que la comprueba dentro de
  la transacción que escribe las filas, y no hay ventana entre el
  comprobar y el escribir—. Publicar en una dirección ocupada **no**
  sustituye en silencio: la política de reponer la decide el
  **repositorio**, en su configuración versionada (`raw.replace`), y por
  defecto la dirección publica una vez —republicar el mismo contenido es
  idempotente y publicar otro es un `409` cuya escritura se aborta, sin
  que quede ni un byte en el almacén. La autorización es de Cedar, con
  las acciones `UploadAsset` y `DownloadAsset`, y el recurso de la
  decisión es el repositorio. Un repositorio de otra cuenta es un `404`,
  no un `403`: un `403` diría que existe en algún sitio. La superficie
  está **cerrada por defecto** (`security.raw_api_enabled`), y un
  despliegue que la abre tiene que declarar las dos acciones en su bundle
  de Cedar: el servicio no arranca con un bundle que no declare lo que
  autoriza con.
- La **condición de una escritura condicional** se comprueba **dentro de
  la transacción** que escribe, y no solo en la frontera. La condición
  viaja al catálogo, que la comprueba sobre la fila que va a escribir: si
  no se cumple, la transacción no se confirma y no queda ni una fila
  escrita. Sin eso, entre el comprobar y el escribir cabía otra
  publicación, y un `If-Match` que se había cumplido podía no cumplirse
  al escribir.
- El **catálogo de artefactos** (`hodei-catalog` y `hodei-catalog-storage`):
  un contexto acotado que dice **qué hay dónde** y quién puede verlo. El
  repositorio declara cuenta, clave, HRN canónico, formato, modo y una
  configuración **versionada**; un modo que esta release no sirve (`proxy`,
  `group`) se rechaza con un error tipado en vez de crear un repositorio
  que luego no va a funcionar. El asset es cuenta + repositorio + ruta, y
  apunta a una versión inmutable y a su digest; la etiqueta mutable es una
  fila aparte, de modo que moverla no reescribe el blob que hay detrás. La
  política de ámbito del digest (`global` o `account_partitioned`) es una
  decisión del repositorio, guardada con él, y de ella sale la clave del
  blob en el CAS. Publicar son cuatro filas (versión, asset, referencia y
  etiquetas) y van en **una sola transacción**: o están las cuatro, o no
  está ninguna. Una segunda escritura incompatible recibe un conflicto y
  **no** sobrescribe en silencio; la republicación idéntica es idempotente.
  La lectura distingue tres estados que no son lo mismo: `Missing` (no hay
  nada), `Denied` (lo hay, pero este principal no puede verlo; lo decide el
  puerto de autorización antes de mirar la base, para no filtrar la
  existencia de lo ajeno) y `Corrupt` (lo hay y no cuadra). El adaptador
  escribe con identificadores deterministas, porque es lo que hace que dos
  clientes concurrentes escriban el mismo registro y el motor detecte el
  conflicto: el motor persistente lo rechaza y el motor en memoria lo
  aceptaría en silencio.
- Correcciones del **catálogo de artefactos** que salió de una revisión
  independiente: los identificadores compuestos son ahora **inyectivos**
  (cada parte se escapa por separado, así que dos coordenadas que solo se
  distinguen por el separador ya no acaban en el mismo registro); una
  escritura **no relee por el camino público** al terminar, de modo que
  escribir ya no exige un permiso de lectura y un cambio confirmado ya no
  se devuelve como `Denied`; la reconfiguración del repositorio es un
  **compare-and-set** sobre la versión que se leyó, de modo que dos
  configuraciones simultáneas no pueden compartir número; el `Denied`
  nombra **la clase de cosa** que se pidió (`repository`, `version`,
  `asset`, `tag`) y no siempre "asset"; la sonda de integridad pregunta
  por la **ubicación** del contenido (cuenta y ámbito) y no solo por su
  digest, para no dar por roto un blob particionado que está sano; crear
  el mismo repositorio dos veces con la misma configuración es idempotente;
  publicar contenido que no está en el almacén de bytes ya no se publica
  (una referencia a un blob que no existe deja un camino que no lleva a
  nada); y el HRN de un repositorio tiene que ser uno que la plataforma
  sepa leer.
- Segunda ronda de correcciones del **catálogo**, de otra revisión
  independiente: `initialize` **no declara el esquema encima de uno más
  nuevo** (si el almacén lo escribió una build más nueva, se niega a
  tocarlo con `SchemaMismatch` en vez de escribir su número encima y
  dejar que el esquema y el número no cuadren); publicar en un repositorio
  **sin nombre** es una petición inválida y no un recurso que falte; una
  republicación idempotente devuelve **el instante que hay en la base** y
  no "ahora", porque lo que no se escribe no cambia de fecha; y el test de
  reinicio ya no puede pasar en verde **sin comprobar nada** (la fase de
  escritura es un test aparte, ignorado, y el proceso que verifica no
  tiene rama que lo saltee: antes, una variable de entorno puesta por
  error en la shell hacía que el proceso que verifica escribiera y se
  saliera sin comprobar una sola cosa). Se verificó también en el motor
  que la transacción se **cancela** cuando una sentencia del lote falla y
  que el `check()` de la respuesta mira todos los resultados, no solo el
  último.
- El **outbox de auditoría** (`hodei-audit` y su adaptador
  `hodei-catalog-storage`): la publicación del catálogo escribe ahora el
  evento de auditoría **dentro de la misma transacción** que escribe el
  asset, de modo que el asset y su evento tienen **una sola frontera de
  commit**: si el evento no se puede escribir, **el asset no se publica**.
  El blob al que apunta puede quedar *staged*, y eso es reconciliable; un
  asset publicado sin evento no lo es. El evento lleva actor, cuenta
  **verificada** (la del principal, no la que declara el llamante), acción,
  recurso, digest, resultado (`allowed`, `denied`, `conflict`, `failed`) y
  correlación, y no lleva el bearer ni las credenciales. El límite de la
  auditoría está escrito: una publicación **mal formada** no deja evento,
  y una publicación **bien formada a la que el sistema se niega a servir**
  deja un evento `failed`, porque ocurrió. La entrega es un
  problema aparte: el dispatcher reclama el evento pendiente con un
  compare-and-set, lo entrega y **solo entonces** lo marca como entregado;
  solo el worker que lo tiene puede marcarlo, así que un lease que expiró
  no permite decir nada de ese evento (y un worker lento no puede devolverlo
  al mercado mientras otro lo entrega). Si el
  sink falla, el evento vuelve a `pending` con su contador de intentos y su
  último error, de modo que lo que no se entregó se puede mirar y nunca se
  afirma una entrega que no ocurrió. La reentrega entrega el **mismo**
  evento (`event_id` es la clave de idempotencia): el outbox entrega al menos
  una vez, y el efecto lógico de un evento es del almacén de auditoría, que
  es el que guarda por ese identificador. Una publicación idempotente no
  escribe un segundo evento: no hubo un segundo hecho. El contrato de auditoría vive
  en su propio crate, que el catálogo y la aplicación HTTP usan sin
  duplicarlo.
- El almacén de contenido del hub (`hodei-hub`) acepta **subidas en
  streaming**: `BlobWriteSession` recibe el contenido a trozos, con un
  fichero de staging propio por escritura, y el `commit` publica el
  contenido entero o no publica nada. El commit valida el algoritmo, la
  longitud y el digest contra lo que el cliente declara, y si no cuadra,
  el staging se elimina y no queda ninguna referencia visible. La lectura
  distingue ahora lo que no está (`NotFound`) de lo que está roto
  (`Corrupted`) de lo que no se pudo leer (`Unavailable`), y el staging
  caducado se limpia al reiniciar. La garantía central no cambia: el mismo
  contenido no se guarda dos veces y lo que sale por el digest es lo que
  entró.
- `ensure-iam-index`, comando operador que declara el indice `policy_account`
  de las politicas IAM. Viaja en la distribucion, junto al binario del
  servicio, y se ejecuta con el servicio detenido: declarar el indice desde
  el servicio retiene el bloqueo de la base de datos en el proceso.

### Fixed

- La **lista del registro de repositorios** sirve lo que el contrato
  promete. `GET /api/v1/repositories` sin `hrn` respondía `400` a todo lo
  que no nombrara un repositorio, y el contrato declaraba una lista con
  `limit` y `offset` que nadie servía. Ahora la ruta sirve la página de los
  repositorios de la cuenta del principal verificado (`total`, `items`,
  `limit`, `offset`), y con `hrn` sigue devolviendo el repositorio, con el
  mismo `404` para una cuenta ajena que para un nombre que no existe.
  Cedar decide un repositorio cada vez: lo que deniega no sale de la página
  **ni se cuenta** en el `total`, porque un total que contase lo que no se
  puede ver diría cuánta hay.
- El **cuerpo de los errores** del registro es `{"error": "<clase>",
  "message": "..."}`, como el del catálogo, y el contrato lo declara así. Un
  **autorizador que no puede decidir** ya no se confunde con uno que
  deniega: la lectura responde `500` con su clase, y no `403`.
- El contrato retiraba un camino que el servicio no sirve:
  `GET|PUT|DELETE /v1/repositories/{id}` prometía leer, actualizar y borrar
  un repositorio del registro, y no hay ni una ni otra. La promesa se
  retira del contrato en vez de dejarlo ahí, y el camino que queda es
  `/api/v1/repositories`, que es donde la API vive.
- Los **ajustes del repositorio** no se perdian en silencio. En una tabla
  `SCHEMAFULL`, un campo `TYPE object` **sin** `FLEXIBLE` declara que el
  objeto no tiene campos, así que el motor guardaba `{}` y descartaba lo
  que se le había dado: un `update_config` subía la versión de la
  configuración y no cambiaba nada. El campo es ahora `object FLEXIBLE`,
  con `OVERWRITE` para que un almacén ya declarado migre solo, y el
  esquema sube a la versión 3.
- La respuesta a un **rango que no se puede servir** lleva la longitud
  del contenido en la cabecera `Content-Range`, que es donde RFC 9110 la
  pone, y no en el cuerpo.
- El motor de evaluacion aplica el schema que gobierna las peticiones: la
  accion se resuelve contra el schema y la peticion se construye con el. Una
  peticion que no encaja con el schema se deniega con el motivo, en vez de
  responder con un error de servidor.
- Un schema que no declara ni un tipo de entidad ni una accion ya no rechaza
  las peticiones: un schema que no dice nada no puede rechazar nada.
- El **almacenamiento de la sesion** del frontend usaba una clave que no
  existe (`STORAGE_KEYS.AUTH`), de modo que el estado de la autenticacion se
  guardaba bajo la clave literal `"undefined"`. La sesion ahora se guarda
  bajo `hodei-auth-token`, que es la clave que el frontend ya declaraba,
  y el token sobrevive a una recarga.
- El **`type-check` del frontend comprobaba nada**: `tsc --noEmit` con el
  `tsconfig.json` de raíz, que no tiene ficheros, no mira un solo fichero.
  Ahora hay un programa de tipos propio para la consola
  (`frontend/tsconfig.console.json`) que comprueba la consola y lo que usa,
  y la etapa de frontend de la CI lo ejecuta. El `build` también: antes
  `tsc -b` recorría el árbol entero y ahora comprueba el programa de la
  consola, porque el frontend heredado tiene errores de tipos que nadie ha
  corregido desde que dejó de comprobarse —121 errores en 63 ficheros, que
  hacen que `tsc -b` no pase—. Corregirlos todos de golpe es un trabajo
  aparte (HSP-133), y mientras tanto el `build` vuelve a poder hacerse.
- El **`build` del frontend no compilaba**: un fichero muerto con JSX en un
  `.ts` rompia el analizador. El fichero se ha borrado --no lo importaba
  nadie-- y el build vuelve a hacer su trabajo.

## [0.2.0] - 2026-10-24

Release del roadmap **Hodei Artifacts Graph-First** tras los hitos **H1a** y
**H1b-1**, **H1b-2**, **H1b-3** y **H1b-4**. **No es GA** y **no declara
paridad con Nexus CE**.

### Añadido

- HRN v2 tipado (`partition/service/region/account/kind/opaque-id`) con
  round-trip canónico, escapes y no reciclaje. El HRN v1 se conserva como
  **alias** (región vacía): el formato en el cable no cambia para los HRN
  existentes.
- `EntityUid` de Cedar con el **scope completo** (partición/servicio/región/
  cuenta/tipo/id), construido solo con APIs Cedar tipadas. Dos homónimos de
  cuentas distintas son **entidades distintas**.
- **Snapshot inmutable por petición**: el motor no conserva `policies` ni
  `entities` mutables compartidas entre solicitudes, así que dos peticiones
  concurrentes con políticas opuestas no se contaminan.
- `POST /api/v1/policies/evaluate` **real**: invoca el puerto cableado y
  devuelve `Allow`/`Deny` con razones, políticas determinantes y
  diagnósticos. `forbid` gana a `permit` y el default es deny.
- `GET /api/v1/schemas/load` **real**: estado (`active` / `no_active_schema`),
  versión, identificador y contenido del esquema activo.
- `POST /api/v1/playground/evaluate` gobernado por el **esquema que el
  consumidor envía**: el verbo se resuelve por `id` contra las acciones
  declaradas, el **namespace lo decide el esquema** y no la petición, el
  contexto llega a la evaluación y una petición que no encaja se deniega
  nombrando el motivo.
- `POST /api/v1/iam/policies` y `POST /api/v1/iam/policies/get`: la política
  persistida **declara sus principales** (`attached_principals`) y la
  atribución sobrevive al viaje de ida y vuelta por la base de datos.
- `POST /api/v1/iam/evaluate`: la evaluación IAM **consulta de verdad** las
  políticas guardadas, y solo las que están atribuidas al principal que se
  decide.
- `tests/module_tree.rs`: guard estructural que resuelve las declaraciones
  `mod` desde las raíces del crate y **falla** si algún `.rs` de `src/` queda
  fuera de la unidad de compilación. Es un invariante, no una aserción sobre
  una ruta, y lleva dos casos negativos que demuestran su poder discriminante.
- `docs/roadmaps/hodei-artifacts-graph-first/`: perfiles de conformidad
  `H1a`/`H1b-1`/`H1b-2`/`H1b-3` en `SPECS/authorization-hrn.md`,
  `ADR/011-h1a-identity-and-decision-core.md`, y las secciones medidas
  `BASELINE.md` §7.3–§7.7 y §9–§15.

### Corregido

- **O1**: `POST /api/v1/policies/evaluate` devolvía un stub fijo.
- **O2**: `GET /api/v1/schemas/load` devolvía `{"status":"stub"}`.
- **O5**: `ActionTrait::action_name()` producía `Iam::Action::"X"` mientras
  que el motor construía `Action::"X"`. Ahora las tres rutas que construyen
  el UID de acción producen la misma forma, y coincide con la del esquema
  IAM registrado.
- **O6**: la evaluación IAM no consultaba las políticas persistidas. El hueco
  real eran **cinco capas** (finder stub sin base de datos, caso de uso nunca
  construido, puerto no cableado, sin ruta HTTP, resolvers sin implementación)
  más una sexta: el registro persistido **no guardaba atribución**, así que
  una política guardada no decía a quién servía.
- **O7**: el `inline_schema` del playground se **descartaba**. Ahora se
  decodifica con el mismo codec de persistencia (`cedar-schema-fragments-v1`)
  y se aplica: una política sobre un tipo declarado se acepta y sobre un tipo
  no declarado se reporta. Un `inline_schema` que no es un sobre se rechaza
  con `400` en vez de leerse como esquema vacío.
- **O3**: `src/infrastructure/` era un directorio de fuentes que **rustc nunca
  compilaba**, así que el `GetEffectiveScpsAdapter` que contenía no existía
  en el binario. Eliminado (3 ficheros, 155 líneas). La rama alternativa
  —declararlo y usarlo— se descartó por medida, no por gusto: el módulo
  importa `hodei_organizations`, que no es dependencia del paquete raíz, y
  cuyo crate está en cuarentena junto con su único consumidor.
- Los falsos verdes del esquema en el playground: el **namespace** lo decidía
  la petición, el **contexto se descartaba**, un verbo no declarado caía a la
  forma plana y una acción ambigua se resolvía al primer candidato. Ahora
  cada caso deniega con su motivo.
- El **estado mutable compartido** entre decisiones concurrentes (riesgo P0
  #3) queda cerrado por el snapshot inmutable por petición.
- La **descripción del paquete** decía «Hodei API stub - Temporarily disabled
  during policies crate migration», una frase heredada de cuando la API no
  tenía rutas reales. El SBOM publicado describiría el producto como un stub
  deshabilitado. Sustituida por una descripción que dice lo que la API es.
  Es metadato: el binario no la embebe, así que su digest no cambia.

### Verificación

- Puerta de calidad medida sobre el árbol publicado: `cargo fmt --all --
  --check` (PASS), `cargo clippy --workspace --all-targets -- -D warnings`
  (PASS, `No issues found`), `cargo test --workspace --no-fail-fast` (PASS) —
  **809 pruebas superadas, 0 fallidas, 64 ignoradas (sólo doctests)**,
  21 suites — y `cargo build --release --workspace` (PASS, binario
  `sha256=6c1d971117fdff6f8718f040a154d3ae89987e1beabe15c5fa12ba08e6ce8d2b`).
- Cada corrección está comprobada por **mutación con muerte observada**:
  el UID de Cedar sin scope, el snapshot compartido, el esquema inline
  descartado, el esquema vacío aplicado en crudo, el handler de carga
  devuelto al stub, el namespace tomado de la petición, el contexto
  descartado, el verbo no declarado, la acción ambigua, el finder que
  devuelve toda política, el finder IAM que no devuelve ninguna, la
  atribución no persistida, el módulo huérfano recreado y el módulo declarado.
- La aceptación efímera se ejecuta sobre la **distribución desempaquetada**,
  desde el directorio del paquete y no desde el árbol del repositorio.
- La **CI remota** es la autoridad de integración: run
  [`40067967678`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40067967678)
  (`success`, commit `d231b24`, el que contiene H1b-4) y run
  [`40068287455`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40068287455)
  (`success`, commit `bf43f5b`). El run del commit de este release se cita en
  el `release-manifest.json` del propio release.

### Limitaciones conocidas

- `Cargo.lock` está en `.gitignore`, por lo que la clave de caché de la CI es
  constante y la caché nunca se invalida (O4). El binario publicado **no es
  reproducible bit a bit entre hosts**: el `SHA256SUMS` del release identifica
  el artefacto exacto que se publicó, no una reconstrucción garantizada. El
  borrado del `Cargo.lock` fue deliberado y su reversión exige una decisión
  del maintainer.
- CORS permisivo en `src/router.rs:52` (O9). La rebanada de O9 —CORS por
  origen— sí es separable y verificable localmente, pero exige decidir el
  origen de la lista de orígenes permitidos.
- La **mitad del motor de O8** sigue abierta: el motor carga el esquema
  registrado, que es de alcance IAM, mientras que
  `POST /api/v1/policies/evaluate` es genérico. Aplicarlo en crudo denegaría
  toda petición fuera del mundo IAM, así que cerrarla exige una decisión de
  alcance de plataforma, no un cambio de código. Por la misma razón, la ruta
  IAM **no aplica** el esquema.
- La **gobernanza** de H1b sigue abierta: SCP como intersección de límites
  (riesgo P0 #2), revocaciones activas y auditadas, credenciales reales.
- **O10**: cuatro ficheros de `crates/hodei-iam` quedan fuera de la unidad de
  compilación (`features/add_user_to_group/factories.rs`,
  `features/add_user_to_group/mocks.rs`,
  `features/create_policy/validation_port.rs` e
  `internal/domain/events.rs`). Son trabajo sin cablear de un crate vivo, con
  criterio de cierre **por fichero** registrado en `BASELINE.md` §7.2. El
  guard `tests/module_tree.rs` protege solo el crate de aplicación.
- S-001 **sigue sin cumplirse por completo**: el namespace de entidad
  `Hodei::*` y el de acción `Hodei::Action::"ReadAsset"` siguen pendientes.
- `docker`, `typedb` y `typedb-server` no están disponibles en el host de
  publicación: las comprobaciones que los requieren quedan `NOT_RUN`, no
  `PASS`. No existe todavía esquema TypeQL.

## [0.1.0] - 2026-10-24

Primer release del baseline reconciliado tras el hito **H0** del roadmap
Graph-First. **No es GA** y **no declara paridad con Nexus CE**.

### Añadido

- `schema_codec` (`cedar-schema-fragments-v1`): el esquema Cedar se serializa
  como `SchemaFragment` en JSON, que reconstruye el esquema sin pérdida. El
  esquema persistido ya no se guarda con su representación `Debug`.
- `tests/binary_contract.rs`: contrato a nivel de proceso sobre el binario
  real. P1/P2 arrancan el binario, lo conectan por socket y comprueban que
  sirve `/health` con la configuración tomada del entorno. P3 (negativo)
  comprueba que una configuración inválida produce código de salida distinto
  de cero y un mensaje explícito.
- `tests/bootstrap_integration.rs` (B1–B7): sustituye al fichero de test de
  1 byte que no contenía ninguna prueba. Incluye los casos negativos B3, B4 y
  B7.
- `tests/schema_persistence.rs` (S1–S3): el esquema persistido tiene que
  volver a ser encontrado por el lector que lo aplica. S3 es la prueba
  discriminante: una política sobre una acción declarada es válida y una
  política sobre una acción o un tipo de entidad no declarados es rechazada.
- `tests/distribution_contract.rs` (P4–P5): el binario desempaquetado arranca
  solo con la configuración que se distribuye y sirve `/health` (P4); sin esa
  configuración se niega a arrancar con un error explícito (P5, negativo).
- `config/default.toml`: configuración base que el binario lee realmente
  (`config/default.toml`), frente a `config/development.toml` y
  `config/release.toml`, que son perfiles por modo de ejecución.
- `scripts/package-release.sh`: ensambla la distribución de forma
  reproducible y **se niega a construir** un paquete sin la configuración que
  el binario lee.
- `CHANGELOG.md`: este fichero.
- `docs/roadmaps/hodei-artifacts-graph-first/BASELINE.md`: censo medido del
  repositorio, clasificación por crate, defectos corregidos (D1–D6) y defectos
  abiertos (O1–O9) con referencia `file:line`.
- `docs/roadmaps/hodei-artifacts-graph-first/`: paquete de roadmap completo
  (specs, ADRs, `ROADMAP.md`, `RELEASE.md`, `TRACEABILITY.md`,
  `compatibility-matrix.csv`).

### Corregido

- La precedencia `HODEI_<SECTION>__<KEY>` documentada era **inerte**: config-rs
  0.14.1 buscaba el prefijo como `HODEI__` al no declarar el separador de
  prefijo, por lo que toda variable de entorno se descartaba en silencio y el
  servicio arrancaba siempre con los valores del fichero TOML. Corregido con
  `.prefix_separator("_").separator("__")` y `try_parsing(true)`.
- El esquema Cedar se reconstruía **vacío** en cada carga, de modo que un
  contenido almacenado que no era un esquema se aceptaba y se descartaba. Ahora
  la carga devuelve un error explícito y el esquema almacenado se aplica de
  verdad al validar políticas.
- Las suites `tests/e2e/` y `tests/integration/` **nunca se compilaron**: el
  paquete raíz sólo publicaba el binario, así que sus rutas `hodei_artifacts_api::…`
  no existían. La suite era verde sin comprobar nada. Las suites heredadas
  quedan en cuarentena en `tests/legacy/` (renombradas, contenido intacto) y
  `tests/legacy_quarantine.rs` documenta y vigila esa decisión.
- `tests/test_register_iam_schema_integration.rs` era un fichero de 1 byte (un
  salto de línea) que Cargo compilaba como un binario de test sin ninguna
  prueba.
- `Makefile` y `.github/workflows/rust-ci.yml` se reconciliaron con los
  miembros reales del workspace, de modo que ninguna puerta puede reportar
  verde sobre un crate que no se compila.
- El esquema Cedar persistido era **invisible para el lector** cuando el
  registro no nombraba versión: se guardaba con `version: null` y el lector
  selecciona el registro por su versión, así que el esquema quedaba en la
  base y ningún consumidor lo encontraba. La validación de políticas
  **degradaba a una comprobación de sintaxis** y el servicio aceptaba
  políticas sobre acciones y tipos de entidad no declarados. El registro
  guarda ahora su versión como id, con el id `latest` para los que no la
  nombran.
- La distribución publicada **no arrancaba**: incluía `config/example.toml`,
  un fichero que el binario nunca lee, y la configuración se deserializa de
  forma estricta. La aceptación efímera anterior no lo detectó porque se
  había ejecutado desde la raíz del repositorio, donde sí hay un fichero de
  configuración legible.

### Verificación

- Puerta de calidad medida sobre el árbol publicado: `cargo metadata` (0),
  `cargo check --workspace --all-targets` (0), `cargo fmt --all -- --check` (0),
  `cargo clippy --workspace --all-targets -- -D warnings` (0),
  `cargo test --workspace --no-fail-fast` (0) — **746 pruebas superadas, 0
  fallidas, 64 ignoradas (sólo doctests)**.
- Cada corrección está comprobada por mutación: el validador con esquema
  vacío, el cargador con esquema vacío, el `prefix_separator` ausente, el
  error de configuración con salida 0, el esquema guardado sin versión, la
  configuración distribuida ausente y la configuración opcional hacen fallar
  un test nombrado.
- La aceptación efímera se ejecuta sobre la **distribución desempaquetada**,
  desde el directorio del paquete y no desde el árbol del repositorio.
- El almacén RocksDB existente se sondeó sobre una copia y **no contiene datos
  de usuario** (`INFO FOR ROOT` no declara ningún namespace), por lo que no
  hubo que exportar nada y no se destruyó nada.

### Limitaciones conocidas

- `POST /api/v1/policies/evaluate` devuelve un stub fijo y
  `GET /api/v1/schemas/load` devuelve `{"status":"stub"}` (defectos O1 y O2).
- `src/infrastructure/` es un módulo que no se compila (O3).
- `Cargo.lock` está en `.gitignore`, por lo que la clave de caché de la CI es
  constante y la caché nunca se invalida (O4).
- `ActionTrait::action_name()` produce `Iam::Action::"X"` mientras que el
  motor de evaluación construye `Action::"X"` (O5).
- CORS permisivo en `src/router.rs` (O9).
- `cargo-nextest`, `docker`, `typedb` y `typedb-server` no están disponibles
  en el host de publicación: las comprobaciones que los requieren quedan
  `NOT_RUN`, no `PASS`.
- No existe todavía esquema TypeQL: TypeDB es trabajo de H1 en adelante.
- `Cargo.lock` no se versiona (O4), de modo que el binario publicado no es
  reproducible bit a bit entre hosts: el `SHA256SUMS` del release identifica
  el artefacto exacto que se publicó, no una reconstrucción garantizada.

[0.2.0]: https://github.com/Rubentxu/hodei-artifacts/releases/tag/v0.2.0
[0.1.0]: https://github.com/Rubentxu/hodei-artifacts/releases/tag/v0.1.0
