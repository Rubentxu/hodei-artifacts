# Repository proxy remoto — contrato de acceso a upstream

**Tareas:** HSP-106 (HAR-12), HSP-107 (HAR-13) · **Servicio medido:** `hodei-artifacts 0.2.0`

## 1. Qué afirma este contrato

Afirma que **un proxy solo sirve lo que verificó** y que **el servicio solo
sale a la red donde se le ha autorizado**. La primera lectura de una ruta de
un repositorio proxy trae el contenido de su upstream, lo comprueba, lo
guarda como contenido propio y deja un recibo de procedencia. Un fetch que
falla en cualquier punto no deja contenido ni recibo: nada de lo que quede
en el sistema puede presentarse como contenido verificado.

Las reglas, una por una:

- **Configuración versionada.** El upstream de un repositorio vive en
  `config_settings.proxy.upstream` (`id`, `url`, `scopes`, `credential_ref`,
  `redirects`, `max_bytes`, `require_checksum`). Se valida al crear o
  reconfigurar el repositorio y un campo desconocido, dentro del upstream o
  junto a él bajo `proxy`, es un error: un ajuste de salida a la red
  ilegible nunca degrada a «permitido». La credencial es una
  **referencia** (`env:NOMBRE`), nunca el secreto.
- **Dos niveles de autoridad.** El repositorio dice a qué upstream habla; la
  política de salida del servicio (`[proxy] allowed_hosts`) dice a qué
  hosts **puede** llegar el servicio, y `[proxy] allowed_credentials` dice
  qué variables de entorno puede leer como credencial. Por defecto ambas
  listas están vacías: el servicio no sale a ningún sitio y no envía
  ninguna credencial, aunque un repositorio nombre una variable que existe.
- **Protección SSRF.** Cada salto (el primero y cada redirect) resuelve el
  nombre, valida **todas** las direcciones y fija la conexión a la dirección
  validada, de modo que un rebind de DNS entre la comprobación y la conexión
  no cambia el destino. Siempre se deniegan las direcciones no
  especificadas, multicast, broadcast, link-local (incluido el servicio de
  metadatos `169.254.169.254`) y reservadas. Loopback, RFC1918, CGNAT y ULA
  solo se abren con el permiso explícito `private` en la entrada de la lista.
  Una IPv4 embebida en IPv6 (mapeada, NAT64, 6to4) se clasifica por la
  IPv4. Sin proxy del sistema, sin credenciales en la URL.
- **Redirects.** Se siguen a mano, con un tope, y cada destino pasa de nuevo
  por la política. Un redirect hacia otro origen está desactivado salvo que
  el repositorio lo permita (`redirects.cross_origin`) y el destino esté
  autorizado. La credencial viaja **solo** al origen del upstream: nunca
  tras un salto a otro origen.
- **Verificación.** La respuesta debe ser `200` completa, sin
  compresión, dentro del presupuesto de bytes (el menor entre el del
  upstream y el de la política) y, si declara `X-Checksum-Sha256`, con ese
  checksum. Con `require_checksum` una respuesta sin checksum se rechaza.
  Sin checksum declarado y sin exigirlo, el recibo dice `computed_only`:
  es identidad del contenido, no verificación contra el upstream.
- **Receipt de procedencia.** Cada primera resolución escribe un recibo
  (upstream, URL final sin credenciales ni consulta, versión de la
  configuración, digest, bytes, evidencia de checksum, redirects, ETag,
  quién lo pidió y la correlación). Se lee con
  `GET /api/v1/provenance?repository=&path=` y la acción Cedar
  `DownloadAsset`.
- **Confianza.** Lo recién bajado pasa por la misma admisión que lo
  subido: en un repositorio proxy del canal trusted no se sirve hasta que se
  promueve.
- **Un proxy no acepta escrituras de clientes.** `PUT` responde `405`.
- **Errores sin filtraciones.** `404` si el upstream no tiene la ruta o la
  ruta cae fuera de los scopes (sin llamar al upstream), `503` si el servicio
  está saturado, `504` si el upstream no respondió a tiempo y `502` para el
  resto de fallos del upstream, con un mensaje genérico: ni el log ni la
  respuesta llevan la credencial. Un fallo del almacén o del libro de
  procedencia responde `503` y un servicio sin libro `501`. El tiempo
  límite es de la resolución entera (redirects y cuerpo incluidos), no de
  cada salto.

### Caché del proxy (HSP-107)

La política vive en `config_settings.proxy.cache`, junto a `proxy.upstream`,
y se versiona con el repositorio. Sin ella no cambia nada: el contenido
bajado no caduca. Un campo desconocido es un error, como en el upstream.

```json
{ "proxy": { "cache": {
    "ttl": { "content_secs": null, "metadata_secs": 300 },
    "metadata_paths": ["maven-metadata.xml", "dists/"],
    "negative_secs": 60,
    "stale_if_error_secs": 3600,
    "offline": false
} } }
```

- **Dos clases de ruta.** El *contenido* (un jar, una capa) se identifica
  por sus bytes y es inmutable; la *metadata* (un índice, un
  `maven-metadata.xml`) es lo que el upstream republica. Es metadata el
  último segmento de la ruta que figura en `metadata_paths` (por defecto
  `maven-metadata.xml`, `repomd.xml`, `Packages`, `Packages.gz`,
  `Packages.xz`, `Sources`, `Sources.gz`, `Sources.xz`, `Release`,
  `Release.gpg`, `InRelease`, `index.json`) o una ruta bajo un prefijo que
  termina en `/`. El checksum o la firma de una metadata
  (`maven-metadata.xml.sha1`, `.asc`...) sigue su clase; los de un artefacto
  siguen siendo contenido.
- **TTL por clase.** Ausente = no caduca; `0` = se revalida en cada lectura;
  el tope es de diez años. Un TTL vencido no sirve nada por sí mismo:
  dispara una petición **condicional** (`If-None-Match` con el `ETag`,
  `If-Modified-Since` con el `Last-Modified` del recibo que respalda la
  copia). Los validadores solo viajan al origen del upstream, y un `304` solo
  vale si lo contesta ese origen.
- **`304` o mismos bytes = confirmación.** Se escribe un recibo
  `revalidated` (con el checksum heredado del recibo anterior) y no se mueve
  nada. Otros bytes para **metadata** son una actualización y se publican
  (`refreshed`).
- **Otros bytes para contenido inmutable = cuarentena.** No se publican: el
  recibo `quarantined` guarda lo que se ofreció, la lectura responde `502`
  («withheld pending review») y mientras no cambie la versión de la
  configuración no se sirve lo viejo ni se vuelve a preguntar. La palanca
  para reabrir es una configuración nueva, que revalida la ruta **sin
  tratarla como inmutable**: si el upstream sigue entregando otros bytes,
  se adoptan. Es un riesgo aceptado y documentado: no hay ruta de borrado de
  assets ni de aprobación manual, y quien reconfigura el repositorio decide
  fiarse de lo que el upstream sirve hoy.
- **Un cambio de configuración revalida, sin heredar nada.** Con una versión
  de configuración distinta de la que confirmó la copia, la siguiente lectura
  la baja **sin validadores** (una descarga por ruta y cambio de
  configuración): el origen nuevo no puede dar por bueno con un `304` lo que
  bajó el viejo, y la evidencia de checksum se comprueba con la política de
  ahora. Si la ruta ya no cae en los scopes, responde `404` sin llamar al
  upstream aunque esté cacheada. La admisión de confianza (`trust`) se
  evalúa en cada lectura, también en las degradadas: un paquete revocado o
  que el canal deja de servir no se sirve fresco, ni stale ni offline.
- **El upstream se identifica por su `id`.** Offline y stale sirven lo que se
  confirmó con ese `id`, aunque la `url` o la versión de configuración hayan
  cambiado después; para cambiar de origen se cambia el `id`, y la copia
  deja de tener procedencia vigente. Es un riesgo aceptado: ni offline ni
  stale pueden preguntar al origen nuevo, y ambos exigen la opción explícita
  de la política.
- **Stale solo si se pide.** Con `stale_if_error_secs > 0`, un fallo del
  upstream (saturado, timeout, inalcanzable, sin verificar, incompleto o un
  fallo `5xx`) sirve la copia mientras su edad sea menor que el TTL más ese
  margen, con `x-hodei-cache: stale`, `x-hodei-upstream` y
  `x-hodei-confirmed-at`. Con `0` (por defecto) el fallo se propaga. Un
  `404` o `410` del upstream **no** es un fallo: dice que la ruta ya no
  existe, y no se sirve stale.
- **Caché negativa.** Con `negative_secs > 0` un `404` se recuerda durante
  ese tiempo, en memoria, por repositorio, ruta y versión de configuración;
  solo para rutas sin asset. Por defecto no se recuerda. El tope de 4096
  entradas es global al proceso y lo comparten todos los repositorios.
- **Offline determinista.** Con `offline = true` en la política del
  repositorio, o `[proxy] offline = true` (`HODEI_PROXY__OFFLINE`) en el
  servicio, no se abre ninguna conexión: se sirve únicamente lo que ya está
  cacheado, tras comprobar su integridad en el almacén, con
  `x-hodei-cache: offline` y la procedencia (`x-hodei-upstream`,
  `x-hodei-confirmed-at`). Un fallo de integridad o una ruta sin copia
  responden `503` con un mensaje explícito; no hay fallback a la red.
- **Single-flight.** Las peticiones simultáneas de la misma ruta, versión de
  configuración y repositorio comparten una sola resolución (la clave es
  ruta y versión de configuración, dentro del repositorio): una descarga,
  un recibo (a nombre de quien la lideró) y el mismo resultado para todas.
  Si el líder se cancela, un seguidor lo releva. Las rutas distintas no se
  esperan entre sí, y la concurrencia total sigue acotada por
  `max_concurrent`.
- **Observabilidad.** Cada respuesta del proxy lleva `x-hodei-cache` con
  `hit`, `miss`, `revalidated`, `refreshed`, `stale` u `offline`. El servicio
  cuenta hits, misses, revalidaciones, stale, errores del upstream, cuarentenas
  y fallos offline en contadores internos y los registra con `tracing`
  (`target = "hodei_artifacts::proxy"`). Los recibos de
  `GET /api/v1/provenance` incluyen `outcome` (`fetched`, `revalidated`,
  `quarantined`) y `last_modified`. Los contadores son por petición: los
  seguidores de un single-flight cuentan como `miss` o `refreshed` y como
  coalescidos. La lectura caliente del libro mira solo los 32 recibos más
  recientes de la ruta, porque el libro crece con cada revalidación.

## 2. Qué se ha medido

| Prueba | Qué comprueba |
| --- | --- |
| `src/proxy/egress.rs`, `fetch.rs` (unitarias) | Clasificación de direcciones, política de hosts y puertos, esquema, `BodyGuard` (presupuesto, longitud, checksum). |
| `crates/hodei-catalog/src/proxy.rs` (unitarias) | Validación de la configuración del upstream y de las referencias de credencial. |
| `crates/hodei-catalog-storage/tests/hsp106_provenance_ledger_contract.rs` | El libro de recibos sobre el motor real, incluida la persistencia tras reabrir. |
| `tests/hsp106_proxy_e2e.rs` | El proceso real por HTTP, con un upstream local autorizado y endpoints hostiles. |
| `crates/hodei-catalog/src/proxy_cache.rs` (unitarias) | La política de caché y la decisión de frescura, pura: TTL, stale, cuarentena, offline, cambio de configuración. |
| `src/proxy/cache.rs`, `fetch.rs` (unitarias) | Single-flight (relevo del líder, errores compartidos), caché negativa acotada y por versión, contadores; `304`, validadores solo al origen y modo offline del cliente. |
| `tests/hsp107_proxy_cache_e2e.rs` | El proceso real por HTTP con un upstream controlable (ETag, `Last-Modified`, cuerpo mutable, retardo, caída). |

### PROXY-01 — verificar, guardar, dejar recibo

Una ruta del upstream se sirve, el recibo existe con digest, tamaño,
evidencia `declared` y URL final; la credencial llegó al upstream y no
aparece ni en el recibo ni en el log; la segunda lectura no vuelve a pedir
al upstream. Sin checksum declarado el recibo dice `computed_only`; con
`require_checksum` la respuesta se rechaza.

### PROXY-02 — un upstream hostil no deja nada verificado

Checksum falso, cuerpo truncado, exceso de bytes y error `500` responden
`502`; una ruta inexistente `404`; en ningún caso queda recibo, y la segunda
petición repite el fallo (no se sirve nada). Una ruta fuera de los scopes
responde `404` sin llamar al upstream.

### PROXY-03 — la salida solo va donde se autorizó

Sin lista de salida, o con el host listado pero sin el permiso `private`
para el loopback, no sale ninguna petición. Un redirect hacia otro origen,
hacia `169.254.169.254`, hacia una red privada, hacia un host no autorizado
o en bucle responde `502` sin seguir el salto (con `cross_origin = false` el
rechazo llega por el cambio de origen; que el destino **resuelva** a una red
prohibida se mide en las unitarias de `fetch.rs` y `egress.rs`, también con
`cross_origin = true`). Un redirect dentro del origen
se sigue y el recibo cuenta el salto. Con redirect entre orígenes permitido,
el segundo origen **no** recibe la credencial. Un upstream https cuyo
handshake TLS falla se rechaza.

### PROXY-04 — la confianza no se salta por bajar

En un repositorio proxy del canal trusted, lo recién bajado responde `404`
hasta que se promueve, y `GET /api/v1/provenance` no lista el recibo de
ese digest mientras el canal no lo sirva: es la misma compuerta que la
lectura. El upstream sí fue consultado (el `404` no es un fallo de
resolución).

### PROXY-05 — TTL y revalidación condicional

Con `metadata_secs = 0`, cada lectura de una metadata envía `If-None-Match`
(o `If-Modified-Since` si solo hay fecha) y un `304` responde con la copia y
un recibo `revalidated`; con TTL positivo, dentro del tiempo no se molesta al
upstream y pasado se revalida; el contenido no caduca y metadata cambiada se
actualiza (`refreshed`).

### PROXY-06 — single-flight, stale y caché negativa

Ocho peticiones simultáneas a un upstream lento descargan una vez y dejan un
recibo. Con el upstream caído, sin `stale_if_error_secs` la lectura falla
`502` y con él se sirve la copia con `x-hodei-cache: stale` y la confirmación;
un `404` del upstream no se sirve stale. Con `negative_secs` un `404` repetido
llama una vez al upstream; sin él, cada vez.

### PROXY-07 — offline

Con `HODEI_PROXY__OFFLINE=true` y con `offline` en la política del
repositorio, una ruta ya cacheada se sirve con `x-hodei-cache: offline` y su
procedencia; una ruta sin copia responde `503` y el upstream no recibe
ninguna petición.

### PROXY-08 — mismatch, cuarentena y cambio de política

Otros bytes para contenido inmutable responden `502` sin servir lo ofrecido,
dejan un recibo `quarantined`, no vuelven a consultar al upstream y se
reabren con una configuración nueva. Tras un cambio de scopes, una ruta
cacheada que queda fuera responde `404` sin llamar al upstream y una que
sigue dentro se revalida una vez, sin validadores.

### PROXY-09 — revocación

Un digest revocado deja de servirse en las tres vías: en fresco (`404`), con
el upstream caído con `stale_if_error_secs` (no hay stale) y en offline
(`404`, sin red), sin que el cambio de canal tenga que esperar al TTL.

## 3. La superficie

| Ruta | Acción Cedar | Qué hace |
| --- | --- | --- |
| `GET`/`HEAD /repository/{repo}/{path}` | `DownloadAsset` | En un proxy, resuelve, sirve o revalida contra el upstream según la política de caché; `x-hodei-cache` dice cuál. |
| `PUT /repository/{repo}/{path}` | `UploadAsset` | Sobre un proxy responde `405`. |
| `GET /api/v1/provenance?repository=&path=` | `DownloadAsset` | Los recibos de la ruta, el más reciente primero. |

### Cómo se activa

- **Repositorio.** `mode = proxy` con `config_settings.proxy.upstream`. No
  existe aún una ruta HTTP que cree repositorios del catálogo.
- **Salida del servicio.** `[proxy] allowed_hosts` (o
  `HODEI_PROXY__ALLOWED_HOSTS`, separado por comas) con entradas
  `host[:puerto][;private][;http]` o, en fichero, objetos
  `{host, port, allow_private, allow_http}`. Más `connect_timeout_secs`,
  `total_timeout_secs`, `max_bytes` y `max_concurrent`.
- **Credencial.** `credential_ref = "env:NOMBRE"` lee la variable `NOMBRE`
  del entorno del servicio **solo si** `NOMBRE` está en
  `[proxy] allowed_credentials` (o `HODEI_PROXY__ALLOWED_CREDENTIALS`,
  separado por comas). Si no, la petición falla con `502` antes de salir.

## 4. Lo que no se promete

- **El libro de recibos es la fuente de la frescura.** Una copia sin recibo
  del upstream actual (publicada antes de HSP-106, o de otro `id`) no tiene
  confirmación y se trata como caducada: se revalida y, offline, responde
  `503`; nunca se sirve sin procedencia.
- **El single-flight es por proceso.** Dos réplicas del servicio no se
  coordinan y pueden descargar el mismo contenido una vez cada una. El
  recibo queda a nombre de quien lideró la descarga.
- **La caché negativa es de memoria.** Se pierde al reiniciar y no se
  comparte entre réplicas.
- **Las métricas son contadores internos, cabeceras y logs.** No hay
  endpoint `/metrics` ni acción Cedar nueva.
- **La cuarentena se levanta con cualquier configuración nueva**, aunque el
  cambio no tenga relación con la ruta retenida, y deja de tratarla como
  inmutable (ver arriba). No hay borrado de assets ni aprobación manual.
- **Un `304` de un upstream hostil** que redirige la ruta dentro de su mismo
  origen se acepta como confirmación: el origen es quien lo contesta.
- **El relevo de un líder cancelado** puede duplicar una descarga y su recibo
  en una ventana estrecha.
- **No hay repositorios virtuales ni de grupo** (HSP-108).
- **`HEAD` baja el contenido.** En un proxy sin el asset, un `HEAD` resuelve
  igual que un `GET`: el coste es la descarga completa.
- **La lista de salida es del servicio, no de la cuenta.** Cualquier cuenta
  con permiso para configurar un repositorio proxy puede apuntarlo a
  cualquier host de la lista, y el semáforo `max_concurrent` es global. La
  configuración de repositorios proxy es una capacidad administrativa; la
  vinculación de hosts y credenciales a una cuenta no está en este contrato.
- **Una entrada `http` admite el Bearer en claro.** `;http` es la opción
  explícita del operador para una red de confianza o un laboratorio; sin
  ella, un upstream `http://` no se contacta.
- **El recibo muestra quién bajó el contenido** (`actor`, `correlation_id`)
  a quien puede leer el repositorio de su cuenta. Los recibos de otra cuenta
  no se ven.
- **Un fallo del libro tras el commit deja un blob sin catalogar** que
  recoge el GC; nunca se sirve sin su recibo. Un `publish` perdido deja el
  recibo y la siguiente petición vuelve a bajar y escribe otro.
- **La lectura de un blob grande carga el cuerpo entero en memoria** (el
  camino de descarga es el de los repositorios hosted, ajeno a esta tarea).
- **TLS usa el almacén de confianza del sistema** (`native-tls`).
- **El checksum es el que el upstream declara.** Un upstream que miente con
  un checksum coherente con sus bytes no se detecta aquí: sin una firma o un
  checksum independiente, la verificación es contra lo que dice el
  upstream.
- **`200` y `304`.** No se sigue un `206` ni se negocia compresión; un `304` solo vale como respuesta a una petición condicional.
- **Solo autenticación Bearer** con una credencial por upstream.
- **Los recibos no caducan.** Se conservan como historia.
- **El rebind se prueba en unitarias** con un resolvedor guionado, no en el
  e2e: un DNS hostil no se simula con procesos locales.
- **TLS no se mide con certificados propios.** El e2e comprueba que un
  handshake que falla se rechaza; no hay entidad certificadora de prueba.

## 5. Cómo se ejecuta

```bash
cargo test --lib proxy
cargo test -p hodei-catalog --lib proxy
cargo test -p hodei-catalog-storage --test hsp106_provenance_ledger_contract
cargo test --test hsp106_proxy_e2e -- --test-threads=2
cargo test -p hodei-catalog --lib proxy_cache
cargo test --test hsp107_proxy_cache_e2e -- --test-threads=2
```
