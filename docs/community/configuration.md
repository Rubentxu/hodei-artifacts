# Referencia de configuración

Esta página es la **referencia** (en el sentido de Diátaxis) de la
configuración declarativa de `hodei-artifacts-api`: cada clave de
[`config/standalone.toml`](../../config/standalone.toml) y de
[`config/external.toml`](../../config/external.toml), las variables de
entorno y la diferencia entre un laboratorio y una producción. La fuente de
verdad es [`src/config.rs`](../../src/config.rs); si algo de aquí discrepa del
código, manda el código.

> **Cómo se verificó.** Los comportamientos marcados como *(verificado)* se
> ejecutaron con el binario `hodei-artifacts-api` y con `hodei-recovery`
> compilados de este repositorio, sobre directorios temporales. El resto está
> citado de `src/config.rs`.

## 1. Cómo se carga la configuración

El servicio lee su configuración **relativa al directorio de trabajo**, de
menor a mayor precedencia:

| Orden | Fuente | Nota |
| --- | --- | --- |
| 1 (menor) | `config/default.toml` | La distribución lo trae; sin él el servicio no arranca (*verificado*: `missing field \`server\``). |
| 2 | `config/{RUN_MODE}.toml` | `RUN_MODE` vale `development` si no se define. La imagen OCI fija `RUN_MODE=standalone`. |
| 3 | `config/local.toml` | Ajustes locales; no se versiona. |
| 4 (mayor) | Variables `HODEI_<SECCIÓN>__<CLAVE>` | Ganan siempre. |

La carga es **estricta**: una clave obligatoria ausente o un valor inválido
detiene el arranque con un mensaje que nombra la clave y la variable que la
arregla (*verificado*, por ejemplo `Invalid log level 'loud'. Valid values:
trace, debug, info, warn, error`).

Cuando se usa `RUN_MODE=standalone` o `external`, el fichero de perfil se
**superpone** a `config/default.toml`: lo que el perfil no dice lo aporta el
`default.toml`. Por eso `external.toml` no necesita su propia sección
`[rocksdb]`, que sí es obligatoria en el conjunto.

## 2. Variables de entorno

### 2.1 La regla `HODEI_<SECCIÓN>__<CLAVE>`

- Prefijo `HODEI_`, y **doble guion bajo** (`__`) entre sección y clave, y
  entre tablas anidadas.
- Los valores se interpretan como número o booleano cuando lo son.
- Las claves de tipo lista se escriben **separadas por comas**. Solo estas
  cuatro se interpretan como lista:
  `security.cors_allowed_origins`, `proxy.allowed_hosts`,
  `proxy.allowed_credentials` y `public_url.trusted_proxies`.

```bash
HODEI_SERVER__PORT=8080
HODEI_ROCKSDB__PATH=/app/data/hodei.rocksdb
HODEI_CONTENT__S3__BUCKET=hodei-content          # tabla anidada [content.s3]
HODEI_SECURITY__CORS_ALLOWED_ORIGINS=https://app.example,https://admin.example
```

Una variable con el prefijo mal escrito se **descarta sin error**: comprueba
siempre en el log de arranque que el valor llegó (el servicio imprime
`Server:` y `RocksDB path:`).

### 2.2 Variables que no siguen la regla

| Variable | Quién la lee | Efecto |
| --- | --- | --- |
| `RUN_MODE` | `src/config.rs`, `src/main.rs` | Elige `config/{RUN_MODE}.toml`. Por defecto `development`. |
| `HODEI_METADATA_SIGNER_ARMOR` | `src/openpgp.rs` | Clave secreta OpenPGP blindada con la que el hub firma los metadatos de APT y RPM. Si falta, el arranque genera una clave **efímera** distinta en cada arranque. Si no es una clave secreta blindada, el proceso aborta con `HODEI_METADATA_SIGNER_ARMOR must hold an armored OpenPGP secret key` (*verificado*). Ver [`operations.md`](operations.md). |
| `RUST_LOG` | `src/main.rs` | Si está definida, tiene prioridad sobre `logging.level` para el filtrado de trazas. |
| Variable nombrada en `credential_ref = "env:NOMBRE"` | `src/proxy/fetch.rs` | Credencial de un upstream de proxy. Solo se lee si `NOMBRE` está en `proxy.allowed_credentials`. |
| Credenciales de S3 | SDK de AWS | No hay clave de credencial en el TOML: salen de la cadena estándar del SDK (entorno, configuración compartida, rol de identidad). |

## 3. Claves de `standalone.toml`

Perfil `RUN_MODE=standalone`: RocksDB embebido para los metadatos y
almacén de contenido en el sistema de ficheros bajo `/app/data`, pensado para
un rootfs de solo lectura con un volumen escribible en `/app/data`. No
contiene secretos.

| Clave | Valor en el perfil | Variable de entorno | Qué hace |
| --- | --- | --- | --- |
| `server.host` | `"0.0.0.0"` | `HODEI_SERVER__HOST` | Dirección de escucha. No puede estar vacía. |
| `server.port` | `3000` | `HODEI_SERVER__PORT` | Puerto, 1-65535 (0 se rechaza). |
| `server.request_timeout_secs` | `30` | `HODEI_SERVER__REQUEST_TIMEOUT_SECS` | Tiempo máximo de una petición; debe ser mayor que 0. |
| `server.max_body_size` | `10485760` | `HODEI_SERVER__MAX_BODY_SIZE` | Tamaño máximo del cuerpo, en bytes (10 MiB); mayor que 0. |
| `database.db_type` | `"rocksdb"` | `HODEI_DATABASE__DB_TYPE` | Único valor aceptado hoy: `rocksdb`. |
| `database.namespace` | `"hodei"` | `HODEI_DATABASE__NAMESPACE` | No puede estar vacío. |
| `database.database` | `"artifacts"` | `HODEI_DATABASE__DATABASE` | No puede estar vacío. |
| `database.pool_size` | `10` | `HODEI_DATABASE__POOL_SIZE` | Mayor que 0. |
| `rocksdb.path` | `"/app/data/hodei.rocksdb"` | `HODEI_ROCKSDB__PATH` | Directorio de la base. |
| `rocksdb.create_if_missing` | `true` | `HODEI_ROCKSDB__CREATE_IF_MISSING` | Crea la base si no existe. |
| `rocksdb.compression` | `true` | `HODEI_ROCKSDB__COMPRESSION` | Compresión de RocksDB. |
| `rocksdb.max_open_files` | `1000` | `HODEI_ROCKSDB__MAX_OPEN_FILES` | Mayor que 0. |
| `rocksdb.write_buffer_size` | `67108864` | `HODEI_ROCKSDB__WRITE_BUFFER_SIZE` | Bytes (64 MiB); mayor que 0. |
| `schema.register_iam_on_startup` | `false` | `HODEI_SCHEMA__REGISTER_IAM_ON_STARTUP` | Registra el esquema IAM al arrancar. |
| `schema.validate` | `true` | `HODEI_SCHEMA__VALIDATE` | Valida los esquemas tras construirlos. |
| `schema.storage_type` | `"rocksdb"` | `HODEI_SCHEMA__STORAGE_TYPE` | Almacén de esquemas. |
| `security.admin_api_enabled` | `false` | `HODEI_SECURITY__ADMIN_API_ENABLED` | Abre `/api/v1`; con `false` toda ruta administrativa responde `403`. Abrirla exige además autenticador y autorizador. |
| `security.raw_api_enabled` | `false` | `HODEI_SECURITY__RAW_API_ENABLED` | Superficie de contenido crudo (`/repository/...`). |
| `security.cors_allowed_origins` | `[]` | `HODEI_SECURITY__CORS_ALLOWED_ORIGINS` | Vacío = sin acceso entre orígenes. Se rechaza `*`, mayúsculas, fragmentos y puertos por defecto (`:80`, `:443`). |
| `logging.level` | `"info"` | `HODEI_LOGGING__LEVEL` | `trace`, `debug`, `info`, `warn` o `error`. |
| `logging.format` | `"json"` | `HODEI_LOGGING__FORMAT` | `pretty`, `json` o `compact`. |
| `logging.include_timestamps` | `true` | `HODEI_LOGGING__INCLUDE_TIMESTAMPS` | Marca de tiempo en cada traza. |
| `logging.include_location` | `false` | `HODEI_LOGGING__INCLUDE_LOCATION` | Fichero y línea en cada traza. |
| `repository.durable` | `true` | `HODEI_REPOSITORY__DURABLE` | Usa el adaptador durable en lugar del de memoria. |
| `content.cas_path` | `"/app/data/hodei.cas"` | `HODEI_CONTENT__CAS_PATH` | Directorio del almacén por digest. Debe ser escribible (el arranque lo comprueba). |
| `content.staging_max_age_secs` | `3600` | `HODEI_CONTENT__STAGING_MAX_AGE_SECS` | Edad a partir de la cual se limpia una subida interrumpida. Mínimo 60. |
| `content.backend` | `"filesystem"` | `HODEI_CONTENT__BACKEND` | `filesystem` o `s3`. No hay *fallback* entre ambos. |

*(verificado)* El perfil `standalone` arranca con
`RUN_MODE=standalone` y responde `{"status":"healthy",...}` en `/health`.

## 4. Claves de `external.toml`

Perfil `RUN_MODE=external`: declara metadatos en Dgraph externo y contenido
en un almacén compatible con S3. Las claves `server.*`, `database.namespace`,
`database.database`, `database.pool_size`, `schema.register_iam_on_startup`,
`schema.validate`, `security.*`, `logging.*` y `repository.durable` tienen el
mismo significado y los mismos valores que en `standalone.toml` (tabla de la
sección 3). Lo que difiere:

| Clave | Valor en el perfil | Variable de entorno | Qué hace |
| --- | --- | --- | --- |
| `database.db_type` | `"dgraph"` | `HODEI_DATABASE__DB_TYPE` | Declara el backend Dgraph. |
| `schema.storage_type` | `"dgraph"` | `HODEI_SCHEMA__STORAGE_TYPE` | Almacén de esquemas en Dgraph. |
| `content.backend` | `"s3"` | `HODEI_CONTENT__BACKEND` | Contenido en un almacén de objetos. Exige la tabla `[content.s3]`. |
| `content.s3.bucket` | `"hodei-content"` | `HODEI_CONTENT__S3__BUCKET` | Obligatoria; no puede estar vacía. |
| `content.s3.endpoint` | `"127.0.0.1:4566"` | `HODEI_CONTENT__S3__ENDPOINT` | Endpoint propio, para almacenes compatibles con S3 que no son AWS. Por defecto, el de AWS para la región. |
| `content.s3.region` | `"eu-west-1"` | `HODEI_CONTENT__S3__REGION` | Región del *bucket*. Por defecto la resuelve el SDK. |
| `content.s3.tls` | `false` | `HODEI_CONTENT__S3__TLS` | Habla TLS con el endpoint. Por defecto `true`. |

### Estado: el perfil `external` no arranca todavía

*(verificado)* Con `config/default.toml` y `RUN_MODE=external` el servicio
termina con:

```text
Configuration error: Unsupported database type 'dgraph'. Only 'rocksdb' is supported. ...
```

Es el comportamiento *fail-closed* que declara el propio perfil: la
validación de `database.db_type` solo acepta `rocksdb`, así que **hoy el perfil
`external` documenta la forma de la configuración, pero el servicio se niega a
arrancar con él**. No lo uses como perfil de despliegue hasta que exista un
adaptador Dgraph en el servicio. La validación de `content.backend = "s3"` es independiente de
`database.db_type`, de modo que el contenido en S3 con metadatos en RocksDB es
una combinación que la configuración admite (no se ejecutó contra un almacén
S3 real en esta documentación).

Los valores `endpoint = "127.0.0.1:4566"` y `tls = false` del perfil son los de
un almacén local de pruebas, no los de producción (sección 6).

### Perfil `dgraph.toml`

[`config/dgraph.toml`](../../config/dgraph.toml) es el perfil productivo sobre
la autoridad única del grafo. Se carga con `RUN_MODE=dgraph` y, como el resto
de perfiles, se superpone a `config/default.toml`. Viaja en la imagen OCI junto
con los demás ficheros de `config/` (la imagen copia el directorio entero),
aunque la imagen fija `RUN_MODE=standalone`: para usarlo hay que declarar
`RUN_MODE=dgraph` al arrancar.

Lo que el perfil declara:

- `[graph]` con `endpoint` (`host:port` del Alpha), `namespace`, la tabla
  `[graph.tls]` (`enabled`, `ca_cert_path`, `server_name`) y
  `[graph.credentials]` con `userid`. La contraseña no va en el fichero: se
  inyecta con `HODEI_GRAPH__CREDENTIALS__PASSWORD`.
- `[database] db_type = "dgraph"`, que declara el grafo y evita reclamar el
  motor embebido.
- `[content] backend = "s3"` con su tabla `[content.s3]`: los bytes no viven
  en el grafo.

El significado de cada clave de `[graph]` está en la sección 5.8. Una clave
desconocida bajo `[graph]`, `[graph.tls]` o `[graph.credentials]` se
**rechaza** con un error que nombra la clave; en particular, `namespace` va en
`[graph]`, antes de las subtablas, y bajo `[graph.credentials]` detiene el
arranque en lugar de ignorarse.

## 5. Claves que los perfiles no declaran

Tienen valor por defecto y se pueden añadir a cualquier fichero de perfil o
fijar con variable de entorno.

### 5.1 `[security]`

| Clave | Por defecto | Qué hace |
| --- | --- | --- |
| `oci_api_enabled` | `false` | Sirve la superficie de distribución OCI (`/v2`). |
| `investigation_api_enabled` | `false` | Sirve `/api/v1/investigations`. |
| `oci_upload_ttl_secs` | `3600` | Vida de una sesión de subida OCI sin actividad. |
| `oci_max_concurrent_uploads` | `64` | Sesiones de subida OCI simultáneas. |
| `auth_jwt_secret` | ninguno | Secreto HS256 de los JWT. Excluyente con `auth_jwks_path`. Nunca se serializa y el `Debug` lo oculta. |
| `auth_jwks_path` | ninguno | JWKS local para tokens RS256/ES256. Excluyente con `auth_jwt_secret`. |
| `auth_issuer` | ninguno | Emisor (`iss`) esperado. |
| `auth_audience` | ninguno | Audiencia (`aud`) esperada. |
| `authorization_policy_path` | ninguno | Paquete de políticas Cedar. |
| `authorization_schema_path` | ninguno | Esquema Cedar que valida el paquete. |

Regla del bloque de autenticación: si se define **cualquiera** de
`auth_*` o `authorization_*`, hay que definir **juntos** exactamente uno de
`auth_jwt_secret`/`auth_jwks_path`, más `auth_issuer`, `auth_audience`,
`authorization_policy_path` y `authorization_schema_path`; ninguno puede estar
vacío. Si no, el arranque falla.

### 5.2 `[content]`, `[schema]`

| Clave | Por defecto | Qué hace |
| --- | --- | --- |
| `content.cas_path` | `./data/hodei.cas` | Ver sección 3. |
| `content.gc_grace_secs` | `86400` | Gracia antes de que un blob sin referencias sea reclamable por la recolección. Mínimo 1; en producción, horas. |
| `schema.version` | ninguna | Versión concreta del esquema (opcional). |

### 5.3 `[investigation]`

| Clave | Por defecto | Qué hace |
| --- | --- | --- |
| `max_evidence` | `100` | Piezas de evidencia por investigación. |
| `max_relations` | `400` | Relaciones por investigación. |
| `max_bytes` | `262144` | Bytes de contenido de evidencia (256 KiB). |
| `max_question_length` | `2000` | Longitud máxima de la pregunta, en caracteres. |
| `graph_tenant_template` | `hrn:hodei:artifacts:{region}:{account}:artifact/{account}` | Plantilla HRN del inquilino del grafo; debe producir un HRN válido. |

### 5.4 `[trust]`

| Clave | Por defecto | Qué hace |
| --- | --- | --- |
| `policy_file` | ninguna | JSON con la política de confianza. Sin ella toda promoción se rechaza con decisión registrada. |
| `risk_catalog_file` | ninguna | JSON que mapea HRN de aviso a severidad (`critical` u `other`). |
| `policy_id` | `default` | Identificador bajo el que se activa la política. |
| `grant_max_validity_secs` | `86400` | Techo de vigencia de una concesión nueva. Mínimo 1. |
| `sweep_interval_secs` | `60` | Intervalo del barrido que reevalúa concesiones; `0` desactiva el bucle. |

### 5.5 `[proxy]`

| Clave | Por defecto | Qué hace |
| --- | --- | --- |
| `allowed_hosts` | `[]` | Anfitriones a los que el servicio puede conectarse. Vacía = ninguna salida. |
| `allowed_credentials` | `[]` | Nombres de variables de entorno que un repositorio puede usar como credencial de upstream. |
| `connect_timeout_secs` | `5` | Conexión y resolución de nombre. |
| `total_timeout_secs` | `300` | Duración total de una resolución. |
| `max_bytes` | `1073741824` | Bytes máximos de una descarga (1 GiB); un repositorio solo puede bajarlo. |
| `max_concurrent` | `8` | Descargas simultáneas. |
| `offline` | `false` | Si es `true`, no sale ninguna petición y solo se sirve lo ya almacenado con recibo. |

### 5.6 `[quota]`

| Clave | Por defecto | Qué hace |
| --- | --- | --- |
| `max_stored_bytes_per_account` | sin límite | Bytes almacenados por cuenta. `0` se rechaza. |
| `max_concurrent_uploads_per_account` | sin límite | Subidas simultáneas por cuenta. |
| `retry_after_secs` | `1` | Valor de `Retry-After` cuando se rechaza por concurrencia. Mínimo 1. |

### 5.7 `[public_url]`

| Clave | Por defecto | Qué hace |
| --- | --- | --- |
| `public_base_url` | ninguna | URL pública del despliegue, la que el registro comunica a sus clientes. |
| `trusted_proxies` | `[]` | Direcciones (`10.0.0.1`) o redes (`10.0.0.0/8`) de los proxys inversos cuyas cabeceras `Forwarded` se aceptan. Vacía = se ignoran. |

### 5.8 `[graph]`

La sección `[graph]` declara la **autoridad del grafo**: el Alpha de Dgraph
que el servicio lee y escribe por gRPC. Es opcional (`AppConfig.graph` es un
`Option`). El perfil de ejemplo es
[`config/dgraph.toml`](../../config/dgraph.toml) (`RUN_MODE=dgraph`); la
validación estructural vive en `crates/hodei-graph-client/src/config.rs`.

No hay *fallbacks* implícitos: sin `endpoint` no se prueba `localhost`, con TLS
sin CA no se confía en el almacén del sistema y con credenciales incompletas no
se entra como anónimo. Todo lo que no se declara es un error tipado.

| Clave | Por defecto | Variable de entorno | Qué hace |
| --- | --- | --- | --- |
| `graph.endpoint` | obligatoria | `HODEI_GRAPH__ENDPOINT` | `host:port` del gRPC del Alpha, **sin esquema** (`https://...` se rechaza) y con puerto explícito entre 1 y 65535. Si TLS se usa o no lo decide `graph.tls.enabled`, no el texto del endpoint. |
| `graph.tls.enabled` | obligatoria | `HODEI_GRAPH__TLS__ENABLED` | Si la conexión usa TLS. `false` es una decisión escrita, nunca un defecto. La tabla `[graph.tls]` es obligatoria aunque sea plana. |
| `graph.tls.ca_cert_path` | ninguna | `HODEI_GRAPH__TLS__CA_CERT_PATH` | CA raíz en PEM que firmó el certificado del servidor. **Obligatoria si `enabled = true`**: el clúster Dgraph emite su propia CA. |
| `graph.tls.client_cert_path` | ninguna | `HODEI_GRAPH__TLS__CLIENT_CERT_PATH` | Certificado de cliente para TLS mutuo. Exige `client_key_path`. |
| `graph.tls.client_key_path` | ninguna | `HODEI_GRAPH__TLS__CLIENT_KEY_PATH` | Clave de cliente para TLS mutuo. Exige `client_cert_path`. |
| `graph.tls.server_name` | ninguna | `HODEI_GRAPH__TLS__SERVER_NAME` | Nombre DNS con el que se verifica el certificado, para cuando el Alpha se alcanza por dirección pero el certificado se emitió para el nombre del nodo (`dgraph cert --nodes ...`). Sin él se verifica contra el host del endpoint. |
| `graph.credentials.userid` | ninguna | `HODEI_GRAPH__CREDENTIALS__USERID` | Usuario ACL (por ejemplo `groot`). No puede estar vacío. |
| `graph.credentials.password` | ninguna | `HODEI_GRAPH__CREDENTIALS__PASSWORD` | Contraseña ACL. No puede estar vacía; el `Debug` y la serialización la ocultan (`[REDACTED]`). Inyéctala por entorno, no en el fichero. |
| `graph.connect_timeout_ms` | `5000` | `HODEI_GRAPH__CONNECT_TIMEOUT_MS` | Cota del *handshake* TCP/TLS/HTTP2. `0` se rechaza. |
| `graph.request_timeout_ms` | `10000` | `HODEI_GRAPH__REQUEST_TIMEOUT_MS` | Cota de cada RPC (login, comprobación de salud, consulta DQL). `0` se rechaza. |
| `graph.namespace` | `0` | `HODEI_GRAPH__NAMESPACE` | Namespace ACL del login. `0` es el namespace guardián de un clúster Dgraph estándar. Es una clave de `[graph]`, no de `[graph.credentials]`. |

`graph.credentials` es opcional, pero si se declara el par debe estar
completo (`userid` y `password`). Sin él, el cliente no intenta ningún login y
toda llamada autenticada falla de forma explícita si el servidor lo exige.

#### Incompatibilidad con el motor embebido

Con `[graph]` presente, el grafo es la **única autoridad** y el motor embebido
(SurrealDB sobre RocksDB) se **rechaza**; sin `[graph]`, el servicio corre sobre
el motor embebido como siempre. No hay *fallback* entre ambas autoridades:
metadatos que viven en dos sitios son metadatos en los que ninguna
restauración se puede confiar.

- Si `[graph]` está presente y `database.db_type = "rocksdb"`, el arranque
  termina con este error (extraído de `DatabaseConfig::validate_without_engine`):

  ```text
  the Dgraph profile refuses the embedded RocksDB engine: [graph] declares the authority, so [database] db_type = "rocksdb" would claim a second one. There is no fallback between the two authorities (HSP-209)
  ```

- Con `[graph]` presente no se valida ni se prepara `[rocksdb]`: su ruta no se
  crea, no se abre y no se toca. El resto de claves de `[database]`
  (`namespace`, `database`, `pool_size`) se siguen validando.
- Si `[graph]` es inválido (por ejemplo, endpoint con esquema o TLS sin CA), el
  arranque falla con el error tipado del cliente de grafo, sin conectarse.

### 5.9 `[signing]`

El mirror de providers de Terraform necesita firmar URLs. Terraform descarga el
zip de un provider **sin enviar ninguna credencial** (ni el bloque
`credentials` ni netrc), así que el documento de versión que el mirror entrega
a un lector autenticado (`<versión>.json`) declara, para cada plataforma, una
URL del zip firmada y de corta vida. Esa URL solo sirve para ese zip, de ese
repositorio, a esa cuenta, hasta que expira; canjearla no necesita
`Authorization` y no sirve para nada más. Cedar se evalúa al emitirla y la
puerta de confianza (canal `trusted`) se vuelve a aplicar, por digest, al
canjearla.

La firma es HMAC-SHA256 sobre método, ruta, repositorio, cuenta, principal,
expiración e id de clave. El secreto lo pone **solo el operador**: no hay clave
por defecto y nada se deriva del secreto JWT ni del JWKS.

| Clave | Por defecto | Qué hace |
| --- | --- | --- |
| `keys` | `[]` | El anillo de claves. La **primera** firma y **todas** verifican. Cada entrada es `{ id = "...", secret_file = "..." }` o `{ id = "...", secret_env = "..." }` (exactamente una de las dos). Máximo 8. |
| `ttl_secs` | `300` | Vida de una URL emitida, en segundos. Mínimo 1, máximo 3600. |

Reglas del anillo:

- `id`: de 1 a 32 caracteres entre letras, dígitos, `-` y `_`; sin repetidos.
  Viaja en la URL, así que nombra la clave y no revela nada de ella.
- `secret_file` es una ruta a un fichero con el secreto; `secret_env` es el
  **nombre** de la variable de entorno que lo contiene. El secreto no se
  escribe nunca en el TOML (un campo `secret` literal se rechaza). Los
  espacios y saltos de línea de los extremos se ignoran.
- El secreto debe tener al menos **32 bytes**. Un secreto más corto, un
  fichero que no se puede leer o una variable ausente **impiden el
  arranque**, con un mensaje que nombra la clave y nunca el secreto.
- Todas las réplicas reciben el **mismo** anillo: una URL emitida por una
  réplica la verifica cualquier otra.
- En variables de entorno el anillo se escribe con índices:
  `HODEI_SIGNING__KEYS__0__ID`, `HODEI_SIGNING__KEYS__0__SECRET_FILE` (o
  `HODEI_SIGNING__KEYS__0__SECRET_ENV`), `HODEI_SIGNING__KEYS__1__ID`, …; el
  índice 0 es la clave que firma. El TTL es `HODEI_SIGNING__TTL_SECS`.
- La verificación tolera 5 segundos de desfase de reloj entre réplicas, y no
  cree una expiración más lejana que el TTL máximo más esos 5 segundos.

Sin ninguna clave el mirror **falla cerrado**: el arranque lo avisa en el log
(`no signing key is configured`), el documento de versión responde
`503` con un mensaje explícito y no emite URLs de zip. El resto del mirror (el
índice y la lectura autenticada del zip) sigue funcionando; un cliente
Terraform, en cambio, no puede instalar providers.

Generar una clave y arrancar con ella:

```bash
install -d -m 0700 secrets
openssl rand -hex 32 > secrets/signing-key      # 64 caracteres hexadecimales
chmod 0400 secrets/signing-key                  # legible por el UID del servicio (10001 en la imagen)
```

```toml
[signing]
keys = [{ id = "2026-11", secret_file = "/run/secrets/hodei-signing-key" }]
ttl_secs = 300
```

Rotar una clave sin cortar las URLs en vuelo (el TTL es de minutos, así que
cada paso espera, como mínimo, `ttl_secs` más 5 segundos):

1. **Añadir la nueva detrás**: `keys = [{ id = "A", … }, { id = "B", … }]`.
   Reinicia **todas** las réplicas con el mismo anillo: ahora todas verifican
   con B, y A sigue firmando.
2. **Promover la nueva**: `keys = [{ id = "B", … }, { id = "A", … }]` y
   reinicia. B firma; A solo verifica lo que ya está en vuelo.
3. **Retirar la antigua**: `keys = [{ id = "B", … }]` tras esperar el TTL.
   Lo que firmó A deja de verificar (y ya había caducado).

Si una clave se filtra, retírala del anillo de inmediato: las URLs firmadas con
ella dejan de verificar en el siguiente reinicio, sin esperar al TTL.

## 6. Perfiles: laboratorio frente a producción

| Aspecto | Laboratorio | Producción |
| --- | --- | --- |
| Para qué sirve | Probar en una máquina propia, [`quickstart.md`](quickstart.md) | Servicio al que se conectan clientes reales |
| Escucha | `127.0.0.1` (lo fija el script con `HODEI_SERVER__HOST`) | `0.0.0.0` detrás de un proxy inverso con TLS |
| TLS | Ninguno: HTTP sobre `127.0.0.1` | **TLS obligatorio y terminado fuera del servicio** (ver abajo) |
| Secreto JWT | Aleatorio y efímero (`openssl rand`), vive en el directorio temporal del laboratorio y por variable de entorno | Gestionado por quien despliega; mejor `auth_jwks_path` que un secreto compartido |
| Clave de firma APT/RPM | Efímera (se genera en cada arranque) | `HODEI_METADATA_SIGNER_ARMOR` persistente |
| Datos | Directorio temporal que se borra | Volumen persistente con copias de seguridad |
| Perfil de configuración | Perfil que genera `scripts/community-quickstart.sh` + variables | `config/standalone.toml` o `config/release.toml` + variables o secretos montados |

### 6.1 TLS

**El servicio no termina TLS**: `src/main.rs` enlaza un `TcpListener` y sirve
HTTP en claro. En producción:

1. Pon delante un proxy inverso (o un balanceador) que termine TLS.
2. Declara su dirección en `public_url.trusted_proxies` para que se atiendan
   sus cabeceras `Forwarded`, y la URL pública con esquema `https` en
   `public_url.public_base_url`. Sin proxys declarados el servicio ignora las
   cabeceras reenviadas.
3. Hacia el almacén de objetos, deja `content.s3.tls = true` (el valor por
   defecto). `tls = false` envía las credenciales del SDK en claro y solo es
   aceptable contra un almacén local de pruebas, como el `127.0.0.1:4566` del
   perfil `external`.
4. Los clientes de paquetes usan siempre URL `https://` en producción; no
   desactives la verificación de certificados ni de firmas en ningún cliente
   (ver [`ecosystems.md`](ecosystems.md)).

### 6.2 Secretos

- **Nunca en un fichero versionado.** Ni `standalone.toml` ni `external.toml`
  llevan secretos, y `scripts/community-quickstart.sh` comprueba que el perfil
  de producción no declare `auth_jwt_secret`.
- Pasa `auth_jwt_secret` con `HODEI_SECURITY__AUTH_JWT_SECRET`, o monta un
  JWKS y apunta `auth_jwks_path` a él (`HODEI_SECURITY__AUTH_JWKS_PATH`).
- La clave que firma las URLs del mirror de Terraform (sección 5.9) se genera
  con `openssl rand -hex 32` y se entrega por `secret_file` o `secret_env`;
  jamás se escribe en un TOML ni se reutiliza el secreto JWT.
- Las credenciales de S3 y de upstreams de proxy no van en el TOML: vienen de
  la cadena del SDK y de variables nombradas en `proxy.allowed_credentials`.
- El texto de los errores de arranque pasa por `redact_secrets`, que sustituye
  por `[REDACTED]` lo que parezca un secreto; no dependas de ello para guardar
  un secreto en un fichero que se comparta.

### 6.3 Lista de comprobación de producción

- [ ] El proxy inverso termina TLS y `public_url.*` lo declara.
- [ ] `security.admin_api_enabled`, `raw_api_enabled`, `oci_api_enabled` e
      `investigation_api_enabled` solo están a `true` si el paquete Cedar
      declara las acciones que cada superficie pide.
- [ ] El bloque de autenticación está completo (sección 5.1).
- [ ] `security.cors_allowed_origins` lista orígenes explícitos o está vacía.
- [ ] `HODEI_METADATA_SIGNER_ARMOR` está definida si se sirve APT o RPM.
- [ ] `[signing]` tiene al menos una clave (mismo anillo en todas las réplicas)
      si se sirve el mirror de providers de Terraform (sección 5.9).
- [ ] `/app/data` (o `rocksdb.path` y `content.cas_path`) es un volumen
      persistente y escribible por el usuario del servicio (UID 10001 en la
      imagen).
- [ ] Existe una copia de seguridad verificada (ver [`operations.md`](operations.md)).

## 7. Documentos relacionados

- [`quickstart.md`](quickstart.md): el camino de laboratorio, de punta a punta.
- [`ecosystems.md`](ecosystems.md): recetas por ecosistema y política de upstream.
- [`operations.md`](operations.md): verificación de releases, copia y restauración, rotación de claves.
- [`release-notes-template.md`](release-notes-template.md): plantilla de notas de versión.
