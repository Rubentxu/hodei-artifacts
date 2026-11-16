# Repository proxy remoto — contrato de acceso a upstream

**Tarea:** HSP-106 (HAR-12) · **Servicio medido:** `hodei-artifacts 0.2.0`

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

## 2. Qué se ha medido

| Prueba | Qué comprueba |
| --- | --- |
| `src/proxy/egress.rs`, `fetch.rs` (unitarias) | Clasificación de direcciones, política de hosts y puertos, esquema, `BodyGuard` (presupuesto, longitud, checksum). |
| `crates/hodei-catalog/src/proxy.rs` (unitarias) | Validación de la configuración del upstream y de las referencias de credencial. |
| `crates/hodei-catalog-storage/tests/hsp106_provenance_ledger_contract.rs` | El libro de recibos sobre el motor real, incluida la persistencia tras reabrir. |
| `tests/hsp106_proxy_e2e.rs` | El proceso real por HTTP, con un upstream local autorizado y endpoints hostiles. |

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

## 3. La superficie

| Ruta | Acción Cedar | Qué hace |
| --- | --- | --- |
| `GET`/`HEAD /repository/{repo}/{path}` | `DownloadAsset` | En un proxy sin el asset, lo resuelve contra el upstream (una vez). |
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

- **No hay TTL, revalidación ni modo offline.** Presente es servido; no se
  refresca contra el upstream. Eso es la caché del proxy (HSP-107) y no está
  en este contrato.
- **No hay single-flight.** Dos peticiones simultáneas de la misma ruta
  pueden bajar el contenido dos veces; el segundo publish se ignora y se
  sirve el que ganó. El límite `max_concurrent` acota la carga.
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
- **Solo `200`.** No se sigue un `206`, `304` ni se negocia compresión.
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
```
