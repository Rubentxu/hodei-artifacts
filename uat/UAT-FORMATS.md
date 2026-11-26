# Formatos de paquetes — contrato de aceptación

**Tareas:** HSP-109 (HAR-15, Maven 2 hosted) · HSP-110 (HAR-16, Maven 2 proxy y group) · HSP-111 (HAR-17, npm hosted, proxy y group) · HSP-156 (HAE-23, Cargo hosted y proxy) · **Servicio medido:** `hodei-artifacts 0.2.0`

Este documento reúne los contratos de aceptación de los formatos de paquetes
nativos. Hoy tiene seis secciones, MAVEN (hosted), MAVEN-REMOTE (proxy y
group), NPM, PYPI, CARGO (hosted y proxy) y LINUX; cada formato nuevo añade
la suya.

## MAVEN — repositorios Maven 2 hosted

### 1. Qué afirma este contrato

Afirma que **un cliente Maven publica y resuelve contra el servicio con el
protocolo que ya habla**, sin plugin ni adaptador: `mvn deploy` (probado con
`maven-deploy-plugin:deploy-file`) y `mvn dependency:get` funcionan con el
layout estándar de Maven 2 (`group/con/barras/artefacto/versión/fichero`).

Las reglas, una por una:

- **Layout.** La ruta de cada petición se analiza como Maven 2: `groupId`
  con barras, `artifactId`, versión, y un nombre de fichero que repite
  `artifactId-versión[-clasificador].extensión`. Una ruta que no encaja
  (fichero de otro artefacto, versión que no coincide con su directorio,
  segmento oculto, versión con caracteres fuera de `[A-Za-z0-9._+-]`) es un
  `400` con el motivo.
- **Coordenadas contra el POM.** Al publicar un `.pom`, sus coordenadas
  (`groupId`, `artifactId`, `version`, heredando del `parent` como hace
  Maven) deben coincidir con la ruta. Un POM mal formado, con `DOCTYPE` o de
  más de 1 MiB (`413`) se rechaza; nada de eso deja contenido publicado. Un
  XML con nombres de elemento de más de 64 bytes o con rutas que sumen más
  de 4 MiB se trata como demasiado complejo (`400`).
- **Releases y snapshots.** Cada repositorio declara
  `config_settings.maven.version_policy` (`mixed` por defecto, `release` o
  `snapshot`); una versión de otra clase es un `400`. Una **release es
  inmutable**: repetir los mismos bytes es `200` (idempotente), otros bytes
  con la misma ruta es `409`, salvo que el repositorio declare
  `release_redeploy: true`. En un snapshot, el nombre flotante
  (`lib-1.0-SNAPSHOT.jar`) se repone libremente y los ficheros con timestamp
  (`lib-1.0-20240101.120000-1.jar`) son inmutables como una release.
- **Checksums.** Un `.sha1`, `.md5`, `.sha256` o `.sha512` se valida y no se
  almacena: debe ser hex del tamaño correcto (`400` si no), el fichero al que
  se refiere debe estar ya publicado (`409` si no) y debe coincidir con su
  contenido (`400` si difiere). Al leerlos, el servicio los **calcula** del
  contenido servido, así que un cliente nunca recibe un checksum que no
  corresponda a los bytes.
- **`maven-metadata.xml`.** Los metadatos de artefacto (lista de versiones,
  `latest`, `release`, `lastUpdated`) y de versión snapshot (`timestamp`,
  `buildNumber`, `snapshotVersions`) **se derivan del catálogo en cada
  lectura**, igual que sus checksums. Lo que el cliente sube se valida (XML
  bien formado, sin `DOCTYPE`, coherente con la ruta) y se acepta con `200`
  sin almacenarlo. Una versión aparece cuando existe su POM.
- **Consistencia y recuperación.** Como el metadato es una función del
  catálogo y no un fichero que alguien reescribe, no hay carrera de
  lectura-modificación-escritura entre despliegues concurrentes ni estado a
  reparar tras un fallo parcial: un despliegue interrumpido (jar sin POM)
  simplemente no aparece como versión hasta que llega su POM.
- **Acceso.** Misma autorización que el resto de superficies: `401` sin
  credencial válida, `403` sin permiso de Cedar (`UploadAsset` /
  `DownloadAsset`), `404` para un repositorio de otra cuenta.
- **Reinicio.** Todo lo publicado y sus metadatos derivados sobreviven a un
  reinicio del servicio (catálogo y almacén de contenido durables).

### 2. Cómo se comprueba

| Prueba | Qué mide |
| --- | --- |
| `crates/hodei-catalog/src/maven.rs` (unitarias) | Layout, versiones, POM, metadatos, checksums y política, como funciones puras. |
| `crates/hodei-catalog-storage/tests/hsp109_maven_contract.rs` | Contra SurrealDB real: `assets_under` y la validación de `config_settings.maven`. |
| `tests/hsp109_maven_e2e.rs` | El protocolo con `curl` contra el servicio real: despliegue y lectura, 401/403, POM con coordenadas ajenas, checksum que no coincide, rutas inválidas, política de versiones, inmutabilidad, snapshots, metadatos derivados, despliegues concurrentes, HEAD y condicionales (`304`/`412`) de los metadatos derivados y reinicio. |
| `tests/hsp109_maven_cli.rs` | El cliente **real** (`mvn` 3.9.x, `deploy-file` y `dependency:get`): release, release repetida con otro contenido (falla), dos snapshots con marca de tiempo resueltos por la metadata de la versión (el nombre flotante repuesto lo cubre el e2e con `curl`), política, credencial inválida (401), sin permiso (403) y resolución tras reinicio. La credencial viaja como cabecera `Authorization` desde un `settings.xml` temporal, nunca en argv. |

La prueba con `mvn` falla, y no se salta, si `mvn` no está en el `PATH`.

### 3. Lo que este contrato NO promete

- **Proxy y grupo Maven.** Esta sección cubre solo `hosted`; el proxy y los
  grupos están en MAVEN-REMOTE.
- **Metadatos a nivel de grupo** (`maven-metadata.xml` de plugins) no se
  derivan en un `hosted`; se responde `404`.
- **Rangos.** Los metadatos y checksums derivados no atienden `Range`.
- **`lastUpdated` determinista.** Es la fecha de la última modificación de
  los ficheros del catálogo, no la hora de la petición: dos lecturas sin
  cambios dan el mismo documento.
- **Clasificadores con puntos.** En `lib-1.0-linux.x86.jar` el clasificador
  es lo que va hasta el primer punto (`linux`) y el resto, la extensión
  (`x86.jar`). Las extensiones compuestas (`tar.gz`) funcionan; un
  clasificador que contenga un punto no se distingue de una extensión
  compuesta.
- **Carrera entre lecturas.** Un cliente que lee `maven-metadata.xml` y
  luego su `.sha1` mientras otro despliegue publica entre ambas peticiones
  puede ver un checksum de un documento más nuevo; Maven lo trata como
  checksum incorrecto y reintenta.
- **Máximos.** 20 000 ficheros por consulta de metadatos
  (`MAX_ASSETS_UNDER`), 1 MiB por POM o metadato subido. Si un artefacto
  acumula más ficheros de los que caben en esa consulta (los snapshots con
  marca son inmutables y no se borran), su `maven-metadata.xml` responde
  `503` hasta que se depure el repositorio; es un límite deliberado.
- **Grupo.** Los directorios del grupo no llevan puntos (`com/acme`, no
  `com.acme/`): una sola grafía por coordenada. El `artifactId` y la versión
  sí pueden llevarlos.
- **Orden de versiones.** Sigue el de Maven, que no es transitivo en casos
  raros (`1.0-alpha < 1 < 1-sp`); el servicio ordena sin abortar, pero el
  resultado en esos casos extremos es el de una inserción estable.
- **Coste de los checksums.** El `.md5`/`.sha1`/`.sha512` de un fichero se
  calcula leyéndolo entero en cada petición.

## MAVEN-REMOTE — repositorios Maven 2 proxy y group

### 1. Qué afirma este contrato

Afirma que **el mismo cliente Maven resuelve dependencias a través de un
group de repositorios internos y proxies** (un hosted y un "Central"), que lo
bajado queda en caché y se vuelve a resolver sin red, y que el servicio no
reescribe lo que un upstream publica ni mezcla ficheros y checksums de
miembros distintos.

- **Proxy Maven 2.** Reutiliza el proxy de HSP-106/107 (TTL, validadores,
  `stale_if_error`, offline, cuarentena, credencial por repositorio). Sirve
  POM, jar y metadatos **tal cual** los entrega el upstream: no los
  reescribe, ni los normaliza, ni los valida contra el layout (una ruta
  legítima que el servicio no entendiera no debe romperse). Lo único que
  aplica es la `version_policy` del repositorio: un proxy `release` no
  pregunta por un snapshot (`404` sin tocar el upstream).
- **Qué es mutable y qué no.** El contenido de una release o de un snapshot
  con marca de tiempo es inmutable: si el upstream entrega otros bytes, el
  proxy los retiene (`502` «withheld») hasta que cambie la configuración. Los
  `maven-metadata.xml`, sus checksums y el nombre flotante
  `lib-1.0-SNAPSHOT.jar` son revalidables y siguen el TTL de metadatos: con
  TTL 0 se pregunta en cada lectura y lo nuevo se sirve; dentro del TTL no se
  pregunta.
- **Group Maven 2.** Para un fichero gana el primer miembro que lo tiene
  (un `404` pasa al siguiente; un error del miembro **no** salta al
  siguiente). El checksum de un fichero se calcula **sobre el fichero del
  miembro ganador**, nunca con el sidecar de otro miembro ni con el del
  upstream. Los `maven-metadata.xml` de artefacto son la **unión de las
  versiones** de todos los miembros (hosted: las que tienen POM publicado;
  proxy: las que declara su metadato) con `latest`, `release` y
  `lastUpdated` recalculados; los de versión (snapshots) son los del primer
  miembro que los tiene; sus checksums se calculan sobre los bytes servidos.
  Un metadato de upstream que no se puede leer es un `502`, no una lista
  incompleta. Un proxy no aporta al group versiones que su `version_policy`
  no admite (un proxy `release` no anuncia `2.0-SNAPSHOT`), y los metadatos
  de otro artefacto o con versiones inválidas no aportan nada. Solo los
  metadatos de estas coordenadas que no listan **ninguna** versión (grupo de
  plugins) se sirven verbatim desde el primer upstream que los trae.
- **Sin red.** Con el servicio en `offline`, un proxy del group que no tiene
  copia no aporta (no es un error) y se sigue con el siguiente miembro; si
  ninguno la sirve, la respuesta es `503` («sin red»), no un `404`.
- **Rastro.** Las cabeceras `x-hodei-resolved-from` y `x-hodei-group-version`
  dicen qué miembro sirvió y con qué versión de la configuración, y cada
  resolución queda auditada con su recorrido.
- **Credenciales.** Cada proxy envía solo la suya y solo a su upstream; la de
  quien pregunta no sale nunca, y ninguna aparece en el registro.

### 2. Cómo se comprueba

| Prueba | Qué mide |
| --- | --- |
| `crates/hodei-catalog/src/maven.rs` y `proxy_cache.rs` (unitarias) | Contribución de versiones, fusión de metadatos y clasificación snapshot flotante/inmutable, como funciones puras. |
| `tests/hsp110_maven_remote_e2e.rs` | El protocolo con `curl` contra el servicio real y upstreams locales: Central cacheado y resuelto offline desde el group (POM verbatim, checksum del fichero servido, versiones unidas), snapshot flotante y sus metadatos revalidados por TTL (y sin preguntar dentro del TTL), release y snapshot con marca no sobrescritos, prioridad entre miembros, `404` que continúa y `5xx` que no salta, conflicto de coordenadas hosted/proxy en los dos órdenes con su checksum, segregación de credenciales (también la ausencia de credencial), `version_policy` en el proxy y a través del group, revalidación con `ETag` y `304`, `PUT` rechazado, checksums `md5`/`sha1`/`sha256`/`sha512` de metadatos del group, metadatos de plugins y de versión, metadatos de otro artefacto y metadato ilegible, y un group con varios proxies sirviendo offline desde el que tiene copia. |
| `tests/hsp110_maven_cli.rs` | El cliente **real** (`mvn` 3.9.x, `dependency:get`) resuelve a través de un group con hosted y proxy, y otra vez con el servicio en offline y la caché local vacía, sin que el upstream reciba una petición y sin avisos de checksum. |

La prueba con `mvn` falla, y no se salta, si `mvn` no está en el `PATH`.

### 3. Lo que este contrato NO promete

- **Metadatos de plugins no fusionados.** Se sirve el primer documento que
  trae un upstream, tal cual; no se unen los de varios miembros.
- **Tope de versiones fusionadas.** 20 000 (`MAX_MERGED_VERSIONS`, se
  conservan las más nuevas); un metadato de más de 1 MiB es un `502`. Ambos
  topes se comprueban solo con pruebas unitarias.
- **Confusión de dependencias.** La unión de versiones es la semántica
  habitual de un group Maven: si un upstream anuncia `org.acme:interna:99.0`
  y el hosted no la tiene, el group la anuncia (y la resuelve desde el
  proxy). La prioridad protege las versiones que el hosted **sí** tiene, no
  a las que no tiene. Quien necesite cerrar eso debe poner el hosted en un
  group aparte, sin proxies, o acotar el `scope` del upstream.
- **Un proxy directo no filtra su metadato.** `GET` al proxy sirve el XML del
  upstream tal cual, incluidos snapshots que su `version_policy` no admite;
  el filtrado de versiones se aplica en el group.
- **Un miembro caído es un error del group.** Mientras un miembro falle, los
  metadatos de artefacto del group no se sirven (sería una lista incompleta
  que Maven daría por buena); la disponibilidad se recupera con
  `stale_if_error` o `offline` en el proxy.
- **Un fichero oculto por confianza es un `404`** y no se busca en el
  siguiente miembro (la revocación de un digest no debe hacer aparecer otro
  contenido), igual que en los grupos de Raw. Los metadatos de versión
  siguen la misma regla; en los de artefacto, que son una unión, un miembro
  que oculta su metadato simplemente no aporta.
- **Coste.** Cada lectura de metadatos de un group relee y recalcula la
  unión; con upstreams de miles de versiones eso es CPU por petición, sin
  caché.
- **Sin `Range`** en lo derivado, y los checksums de un fichero se calculan
  leyéndolo entero en cada petición.
- **Escritura.** Un proxy o un group no aceptan `PUT`; se despliega en el
  hosted.

## NPM — repositorios npm hosted, proxy y group

### 1. Qué afirma este contrato

Un repositorio con `format: npm` se sirve en `/npm/{repo}/…` con el protocolo
de registro que el cliente `npm` espera. Lo que sigue lo comprueba el propio
cliente `npm`, no un simulacro.

- **Publicar e instalar.** `npm publish` y `npm install` funcionan con
  paquetes sin alcance y con alcance (`@acme/widget`). El tarball se guarda tal
  cual llegó; el packument anuncia su `shasum` (sha1) y su `integrity`
  (sha512) calculados por el servicio, y el `package-lock.json` resultante
  lleva el mismo `integrity` que el sha512 del tarball servido. Un `dist`
  declarado que no coincide con los bytes es un `400`. Las URL de los
  tarballs del packument son absolutas y salen del `Host` (y de
  `x-forwarded-proto`) de la petición.
- **Versiones inmutables.** Publicar otra vez una versión existente es un
  `403` (npm lo traduce a «You cannot publish over the previously published
  versions»); los bytes históricos nunca se reescriben.
- **Packument y dist-tags versionados.** El packument es una instantánea por
  cambio (publicar, mover una etiqueta, deprecar): cada una queda en el
  historial del catálogo y las anteriores no se tocan. `npm dist-tag add/rm/ls`
  funciona; `latest` se puede mover pero no quitar (`400`) y una etiqueta no
  puede apuntar a una versión inexistente (`404`). Las escrituras concurrentes
  del packument se resuelven con una precondición sobre su digest y hasta 6
  intentos (cuatro publicaciones simultáneas se conservan todas); con más
  contención el que agota los intentos recibe un `409` explícito, nunca una
  pérdida silenciosa.
- **Deprecar sí, despublicar no.** `npm deprecate` marca versiones y deja
  rastro auditado; `npm unpublish` (de una versión o del paquete entero) es un
  `403` con su rastro `unpublish`/`Denied` en el registro del servicio. Es la
  política conservadora: nada publicado desaparece.
- **Autenticación y alcances.** Sin credencial, `401` con
  `WWW-Authenticate: Bearer`; sin permiso Cedar, `403`. Un repositorio puede
  limitar los alcances que admite (`config_settings.npm.scopes`); publicar un
  paquete de otro alcance, o sin alcance cuando se exige, es un `403`. El origen de cada
  versión (quién, cuándo, correlación) queda en el catálogo.
- **Proxy y group.** El proxy sirve packuments (revalidables por TTL) y
  tarballs (inmutables) del upstream y reescribe las URL de los tarballs a las
  suyas, de modo que el cliente nunca habla con el upstream. El group sigue el
  patrón de los demás formatos: gana el primer miembro, un `404` continúa, un
  error del upstream es `502` y no salta, y en offline lo cacheado se sirve
  sin tocar la red (sin copia, `503`).
- **Errores.** Siempre con cuerpo `{"error": "…"}`, que es lo que `npm`
  muestra al usuario.

### 2. Cómo se comprueba

| Prueba | Qué mide |
| --- | --- |
| `crates/hodei-catalog/src/npm.rs` (unitarias) | Nombres, rutas del protocolo, parseo y validación de publicaciones y deprecaciones, dist-tags, integridad declarada y política de alcances, como funciones puras. |
| `tests/hsp111_npm_e2e.rs` | El protocolo con `curl` contra el servicio real: tarballs con URL absolutas e integridad, `304`, `ping` y `whoami`, auditoría (404 con JSON), método no soportado (405 con JSON), cuatro publicaciones simultáneas del mismo paquete, errores JSON (401/404/403/400), instantáneas con tarballs publicados intactos, dist-tags (incluido `latest` y versión inexistente), deprecate y unpublish con su rastro, política de alcances y escrituras rechazadas en proxy y group. |
| `tests/hsp111_npm_cli.rs` | El cliente **real** (`npm`) con `HOME`, caché y `.npmrc` temporales: publicar con y sin alcance, republicar (falla), instalar comprobando el `integrity` del lock contra el sha512 servido, `dist-tag`, `deprecate`, `unpublish` rechazado; y una instalación a través de un group con proxy y hosted, el `E404` de un paquete inexistente y una segunda instalación offline sin que el upstream reciba una petición. El token viaja solo en el `.npmrc` temporal, nunca en argv. |

La prueba con `npm` falla, y no se salta, si `npm` no está en el `PATH`.

### 3. Lo que este contrato NO promete

- **Sin despublicación.** No hay forma de retirar una versión por el cliente;
  la retirada de contenido es una operación de administración del catálogo, no
  de `npm unpublish`.
- **Sin búsqueda ni auditoría.** `npm search` y `npm audit` no se implementan;
  `audit` recibe un `404` que `npm` tolera.
- **Sin `_rev`.** El packument no sirve `_rev`; `npm unpublish` envía
  `undefined`, algo que se rechaza igualmente.
- **Un packument de un proxy es del upstream.** Se reescriben las URL de los
  tarballs, no el resto del documento.
- **Una publicación son dos escrituras.** El tarball y luego el packument; si
  el servicio cae entre ambas queda un tarball sin versión anunciada.
- **Provenance.** Se registra el origen en el catálogo; no se verifican
  atestaciones de Sigstore.
- **Una publicación fallida deja el tarball.** Si tras escribir el tarball el
  packument agota los intentos (`409`), el tarball queda en el catálogo sin
  anunciarse; reintentar con los mismos bytes completa la publicación y con
  otros se rechaza como versión ya existente.
- **Un `deprecate` obsoleto se toma por `unpublish`.** El cliente envía el
  packument entero que leyó; si alguien publica en medio, su documento no trae
  la versión nueva y se rechaza con `403` (y rastro `unpublish`) en vez de
  reconciliarse. Es el lado seguro: nunca se borra nada. Por la misma razón, un
  `deprecate` obsoleto puede deshacer otra deprecación hecha entre medias.
- **Permisos por repositorio, no por paquete.** Quien puede subir al
  repositorio publica, depreca y mueve etiquetas de cualquier paquete que su
  política de alcances admita.
- **URL de tarball según `Host`.** El `dist.tarball` servido se construye con
  `Host` y `x-forwarded-proto`; detrás de una caché compartida hay que variar
  por esas cabeceras (`Vary`), que el servicio no emite.
- **Tarballs en memoria.** Se cargan enteros (tope de contenido del servicio) y
  no admiten `Range`.

## PYPI — repositorios PyPI hosted, proxy y group

### 1. Qué afirma este contrato

Un repositorio con `format: pypi` se sirve en `/pypi/{repo}/…` con la Simple
API de PyPI. Lo que sigue lo comprueban los clientes `twine` y `pip` de
verdad, no un simulacro.

- **Publicar e instalar.** `twine upload` publica un wheel y un sdist, y
  `pip install` los instala (el wheel tal cual, y el sdist construyéndolo en
  el cliente). El hash que declara el cliente tiene que ser el de los bytes
  que han llegado: si no, un `400` y no se publica nada. El nombre y la
  versión tienen que ser los del nombre del fichero, y la versión se compara
  en su forma canónica (PEP 440): `1.0-2`, `1.0.rc1` y `v1.0` son la misma
  versión que `1.0.post2`, `1.0rc1` y `1.0`. La gramática de la PEP no
  distingue mayúsculas (`1.0RC1` es `1.0rc1`) y un segmento numérico de la
  versión local no lleva ceros (`1.0+01` es `1.0+1`). El nombre del fichero
  tiene que ser uno que el `pip` acepte: un wheel al que le falte una parte no
  se publica, porque un enlace que el cliente no puede instalar es un enlace
  roto; y un wheel que el `pip` instalaría —con etiqueta de compilación
  (`1.5`) o con una plataforma local (`any+mips`)— no se rechaza. Una subida
  firmada (`twine upload --sign`) se publica como las demás: la firma no se
  guarda.
- **El límite de la subida es el configurado, y se aplica en dos capas.** Una
  distribución de más de dos megas —que es lo normal— se publica; lo que no
  cabe en el límite del servicio es un `413`, y se dice antes de publicar
  nada. El tope lo ponen las dos capas: el transporte topa el cuerpo entero,
  con el sobre `multipart` incluido, y el manejador topa los bytes del
  fichero. Un sobre con una cabecera gigante no pasa: el `413` es la prueba
  de que el cuerpo entero tiene un límite.
- **Nombres normalizados (PEP 503).** `Lab.Pkg`, `lab_pkg`, `lab-pkg` y
  `LAB--PKG` son el mismo proyecto, y el índice lo publica con el nombre
  normalizado (`lab-pkg`). Un proyecto que no existe es un `404`: no se
  inventa.
- **El índice negocia su representación.** `GET /pypi/{repo}/simple/{proyecto}/`
  sirve HTML (PEP 503) o JSON (PEP 691) según la cabecera `Accept`. Los dos
  dicen lo mismo: el hash (en el fragmento `#sha256=` del HTML, en
  `hashes.sha256` del JSON), la versión de Python que necesita
  (`data-requires-python` / `requires-python`), el estado *yanked* y el
  tamaño. Las URL de las distribuciones son absolutas y salen del `Host` (y de
  `x-forwarded-proto`) de la petición. El índice lleva su `ETag`, y con él no
  se sirve otra vez (`304`).
- **Distribuciones inmutables.** Publicar el mismo nombre de fichero con
  otros bytes es un `409` y los bytes publicados no se reescriben; el mismo
  contenido otra vez es idempotente.
- **El canal de confianza manda sobre el índice.** Un repositorio en el canal
  `trusted` solo sirve los digests con concesión activa: el índice no anuncia
  las distribuciones que no se pueden descargar, y pedir una que no está
  admitida es un `404`, como lo que no existe.
- **Las distribuciones admiten `Range`.** Un tramo se sirve con un `206` y su
  `Content-Range`, y un tramo que no cabe es un `416` que dice en su
  `Content-Range` cuánto hay (`bytes */<longitud>`).
- **El índice es una instantánea.** El documento de un proyecto se escribe
  entero cuando se publica o se retira, direccionado por su digest, con una
  precondición sobre el documento anterior y reintentos acotados. Las
  instantáneas anteriores no se tocan, y las distribuciones viven en su propia
  ruta y no se reescriben con el documento.
- **Retirar una versión (PEP 592).** `POST /pypi/{repo}/yank` retira la
  versión **entera**, con su motivo: el JSON lo dice con `yanked: "motivo"` y
  el HTML con `data-yanked`. Lo retirado sigue descargable cuando se pide
  exactamente esa versión.
- **Autenticación.** `pip` y `twine` no saben mandar un `Bearer`: la
  superficie acepta la cabecera `Bearer` con la credencial de la plataforma y
  la autenticación básica, cuya **contraseña es** la credencial de la
  plataforma. Sin credencial, un `401` con
  `WWW-Authenticate: Basic realm="hodei", Bearer`; sin permiso Cedar, un
  `403`.
- **Proxy.** El proxy pide el JSON de la Simple API a su upstream (con la
  cabecera `Accept` del protocolo) y **vuelve a pintar el índice**: el HTML que
  sale de este servicio lo escribe este servicio, con los valores escapados, y
  el HTML del upstream no se devuelve. Las distribuciones se traen de la URL
  que declara el upstream y se comprueban contra el hash que el índice
  publica: si no coinciden, un `502`. Lo que ya se trajo se cachea (el
  documento del proyecto es revalidable, la distribución es contenido
  inmutable).
- **Group.** El grupo une los proyectos de sus miembros, y cada fichero lo
  gana el primer miembro que lo tiene.
- **Un proxy o un grupo no aceptan escrituras.** Publicar en ellos es un
  `405`.

### 2. Cómo se comprueba

| Prueba | Qué mide |
| --- | --- |
| `crates/hodei-catalog/src/pypi.rs` (unitarias) | Normalización de nombres (PEP 503), nombres de fichero de wheel y sdist —incluido el wheel que el `pip` rechazaría y el que el `pip` instalaría—, versiones de PEP 440 y su forma canónica, el documento de un proyecto, la negociación de la representación, el escapado del HTML, las rutas de la superficie y la comprobación de una subida contra sus bytes, como funciones puras. |
| `tests/hsp112_pypi_e2e.rs` | El protocolo con `curl` contra el servicio real: la subida multipart que habla `twine` (con y sin firma), el índice en JSON y en HTML con el mismo hash, el `ETag` y el `304`, los cuatro nombres del mismo proyecto y el `404` del que no existe, el hash que no es el de los bytes (`400`, y no se publica), la distribución de tres megas, el `Range` de una distribución (`206` y `416` con su `Content-Range`), el límite de la subida (`413` del manejador y `413` del transporte, y nada publicado), la versión que no es canónica, la versión local con ceros, el wheel con etiqueta de compilación, el canal `trusted` que no anuncia lo que no sirve, el `yanked` de una versión y su descarga, el group que une los proyectos de sus miembros, el proxy que vuelve a pintar el índice del upstream sin su HTML y con los valores escapados, y los `401` y `405`. |
| `tests/hsp112_pypi_cli.rs` | Los clientes **reales** (`twine` y `pip`) con `HOME` y `netrc` temporales: publicar un wheel y un sdist, comprobar el índice que ve el `pip`, instalar el wheel y comprobar el módulo instalado, instalar el sdist construyéndolo, y el `401` de una credencial que no vale. La credencial viaja en las variables de entorno de `twine` y en el `netrc` temporal, nunca en argv. |

Las pruebas con `twine` y `pip` fallan, y no se saltan, si no están en el
`PATH`.

### 3. Lo que este contrato NO promete

- **Sin API JSON de PyPI.** No se sirve `/pypi/{proyecto}/json` (la que usa
  `twine` para preguntar si un paquete ya existe): el `twine` solo publica.
- **Sin búsqueda ni `pip index`.** No hay búsqueda de proyectos.
- **Sin metadatos del proyecto.** El `METADATA` del paquete no se valida ni se
  sirve: lo que se comprueba es el hash, el nombre y la versión del fichero.
  Las dependencias (`Requires-Dist`) se leen del wheel ya instalado, no del
  índice.
- **Sin firmas ni atestaciones.** La superficie no guarda ni verifica firmas
  (PEP 740): una subida firmada se publica, y su firma se ignora.
- **Los números de la versión no son infinitos.** La release y el número de
  una pre release se guardan en 64 bits: una versión cuyo número no cabe ahí
  (`99999999999999999999999.0`) no es una versión de este servicio, aunque la
  PEP la admita.
- **El retiro es una extensión propia.** PyPI no tiene API para retirar una
  versión, y `pip` no la invoca: se retira con `POST /pypi/{repo}/yank`.
- **Una publicación son dos escrituras.** La distribución y después el
  documento; si el servicio cae entre ambas, la distribución queda en el
  catálogo sin anunciarse, y reintentar con los mismos bytes completa la
  publicación. Antes de publicar se comprueba que el índice del proyecto
  cabe: si no cabe, se dice (`413`) y no se publica nada.
- **El proxy no reescribe el JSON entero.** Se reescriben las URL de las
  distribuciones; el resto del documento es el del upstream. El upstream tiene
  que servir el JSON de la Simple API: uno que solo sirva HTML no se puede
  proxar.
- **Las URL del índice salen del `Host` de la petición.** Son absolutas, y
  las construye el servicio con la cabecera `Host` (y `x-forwarded-proto`).
- **El alcance del upstream no acota la URL de la distribución.** El
  alcance (`scopes`) acota las rutas que se sirven del upstream; la URL de
  una distribución la declara el upstream y la acota la lista de destinos
  permitidos del servicio. Un proxy de PyPI necesita el alcance completo.
- **El índice de un proxy no se puede enumerar.** Sale vacío: el cliente
  pregunta proyecto a proyecto, porque lo que hay en el upstream no se puede
  saber sin pedirlo.
- **El índice de un grupo no tiene paginación.** Se recorren sus miembros y
  sus proyectos en cada petición.
- **Distribuciones en memoria.** Se cargan enteras (tope de contenido del
  servicio) antes de servirlas: admiten `Range`, pero no hay streaming.
- **El sdist se construye en el cliente.** Instalar desde un sdist necesita las
  herramientas de construcción allí (`--no-build-isolation`); el servicio no
  las trae ni las ejecuta.

## CARGO — repositorios Cargo hosted y proxy

### 1. Qué afirma este contrato

Un repositorio con `format: cargo` se sirve en `/cargo/{repo}/…` con el
protocolo del registro de Cargo en su variante sparse. Lo que sigue lo
comprueba el cliente `cargo` de verdad (`publish`, `add`, `fetch`, `build`),
no un simulacro.

- **Hosted: publicar y resolver.** `cargo publish` sube el sobre de
  longitud prefijada (cabecera JSON y fichero `.crate`) a
  `PUT api/v1/crates/new`. Cada versión deja dos assets inmutables (los
  bytes del `.crate` y su línea de índice), y una versión publicada es para
  siempre: repetir los mismos bytes es `200` (idempotente) y otros bytes es
  `409`. El índice se ensambla listando las líneas, ordenadas por semver,
  y el `cksum` que el cliente verifica al descargar lo calcula el servicio.
- **El índice sparse es determinista.** `config.json` declara `dl`, `api`
  y `auth-required: true` (sin esto `cargo` no manda el token); el camino
  de cada crate es el del protocolo (`1/a`, `2/ab`, `3/a/abc`, `ab/cd/nombre`)
  con el nombre normalizado a minúsculas. Un crate que no existe es un
  `404`, y un camino que no es del protocolo también.
- **Proxy: el documento del upstream, byte a byte.** El índice de un crate
  se trae del upstream por el mismo camino sparse que pide el cliente y se
  sirve **tal cual**: el orden de versiones y los `cksum` son del upstream,
  y reordenar o reescribir el documento rompería la verificación del
  cliente. Es **metadata** (`proxy.cache.ttl.metadata_secs`): el TTL la
  revalida contra el `ETag` del upstream y un `304` renueva el recibo sin
  mover bytes.
- **Proxy: yank y descarga verificada.** Un yank del upstream aparece
  cuando la revalidación trae el documento nuevo (`yanked: true` en la
  línea). La descarga se trae por la plantilla `dl` que el `config.json`
  del upstream declara (con sus marcadores `{crate}`, `{version}`,
  `{lowerprefix}`…, y sin marcadores se añade `/{crate}/{version}/download`)
  y **solo se sirve si el sha256 de los bytes es el que su línea de índice
  declara**: un upstream que entrega otros bytes recibe un `502` y la
  versión no se sirve.
- **Proxy: nunca publica y no suplanta al hosted.** `cargo publish`
  contra un proxy es un `405`. Un nombre que un repositorio cargo
  **hosted** de la misma cuenta ya conoce está reservado: índice y
  descarga por el proxy son un `409`, sin fallback silencioso al upstream
  (ni vivo ni de cache) — el upstream no puede suplantar lo que el hosted
  publicó.
- **Proxy: offline y cache negativa.** Sin salida a la red, lo que ya se
  verificó se sigue sirviendo (`X-Hodei-Cache: offline`); lo que no, un
  `503`. La ausencia que el upstream contestó con `404` se recuerda
  (`proxy.cache.negative_secs`).
- **Acceso.** La autorización del hosted aplica al proxy: `401` sin
  credencial, `404` para el repositorio de otra cuenta (un paquete que el
  principal no puede ver no aparece), la misma decisión de Cedar
  (`DownloadAsset`) en índice y descarga, y la compuerta de confianza
  antes de servir.
- **No hay group para cargo.** Componer un índice por paquete entre
  miembros no está probado con cliente real: crear un repositorio cargo
  en modo group se rechaza en la alta.
- **Límites.** El índice sirve el documento entero del crate (sin
  paginación por versión); el proxy no reescribe las líneas del upstream
  ni valida la gramática de sus `deps`; la plantilla `dl` relativa se
  resuelve contra la base del upstream.

## LINUX — repositorios hosted Debian (apt) y Fedora (yum/dnf)

### 1. Qué afirma este contrato

- **Dos superficies, solo hosted.** `/apt/{repositorio}/...` habla el
  lenguaje de `apt-ftparchive` y `/yum/{repositorio}/...` el de
  `createrepo_c`. Los formatos `apt` y `rpm` admiten **exclusivamente el
  modo hosted**: crear su repositorio como proxy o group se rechaza en la
  alta. No hay mirror de archivos Debian ni caché de distribuciones
  Fedora: lo que publica una mano, lo sirve Hodei.
- **La ruta la dicta el binario.** Un `.deb` se publica en
  `pool/main/{Package}/{archivo}.deb` y un `.rpm` en
  `pool/{name}-{epoch:}-{version}-{release}.{arch}.rpm`; las rutas que
  declaren el `control` y el header reales del paquete. Subirlo a otro
  sitio es un `409` que nombra la ruta correcta, y un cuerpo que no es lo
  que dice su ruta, un `400`.
- **Metadata derivada, nunca escrita.** Los índices no son documentos que
  se reescriben: `Packages` se ensambla listando registros, y el
  `primary.xml.gz` se arma por contenido desde los headers de los `.rpm`
  publicados. La ruta del primary es su propio SHA256
  (`repodata/{sha256}-primary.xml.gz`): es inmutable y cacheable para
  siempre. La revisión y los tiempos del `repomd.xml` salen del último
  asset publicado, no del reloj: mismo estado, mismos bytes, dos lecturas.
- **Firmas de metadata OpenPGP.** `Release.gpg` y `repomd.xml.asc` son
  firmas separadas en armadura sobre esos mismos bytes, y `key.asc` es la
  clave pública del hub, también en armadura: el `signed-by` de apt y el
  `gpgkey` de dnf. El oráculo `gpgv` abre la firma y rechaza cualquier
  metadata alterada.
- **Inmutabilidad y updates parciales.** Cada versión publicada es para
  siempre: bytes distintos bajo la misma ruta son `409`; reenviar los
  mismos bytes es el eco (`200`). Cada `.deb` deja dos assets atómicos —
  los bytes y su registro `.record`—; si una caída deja el binario sin
  registro, el paquete es invisible para el índice, nunca corrupto. Las
  lecturas de metadata llevan ETag e `If-None-Match` (`304`).

### 2. Cómo se comprueba

- `tests/hsp122_apt_e2e.rs` y `tests/hsp122_yum_e2e.rs`: gramática,
  ensamblado de índices, checksums cruzados del `repomd` con el gz
  servido, firma verificada con `gpgv` real, condicionales e
  inmutabilidad a nivel HTTP.
- `tests/hsp122_apt_client.rs`: `apt-get` de `debian:bookworm` (distro
  fijada) construye el `.deb` con `dpkg-deb`, lo publica el hub y el
  cliente real instala verificando la firma del `Release`; con la clave
  de un tercero, `apt-get update` se niega.
- `tests/hsp122_yum_client.rs`: `dnf` y `microdnf` de
  `quay.io/fedora/fedora:latest` (distro fijada) construyen el `.rpm` con
  `rpmbuild`, el hub lo indexa y ambos clientes lo instalan con
  `repo_gpgcheck=1`; con la clave de un tercero, `microdnf` se niega con
  `repomd.xml GPG signature verification error`.

### 3. Lo que este contrato NO promete

- **No hay proxy ni group para apt/rpm.** Nada de espejar el archivo de
  una distro, cachejar descargas de upstream o fusionar repositorios de
  distribución en un group. Está fuera de alcance por decisión de diseño
  (los índices de upstream se validan contra sus propios firmantes, no
  contra el del hub).
- **No se firman los paquetes.** `gpgcheck=0` / sin verificación de `.deb`
  es el diseño: la garantía pedida es la firma de la **metadata**; la
  integridad del paquete la dan los SHA256 y tamaños que el índice fija y
  el cliente comprueba tras la descarga.
- **Subconjunto de superficies del formato.** apt: solo componente
  `main`, índices `Packages` sin comprimir y sin índice `Sources` de
  `.dsc`; yum: solo `primary`, sin `filelists`, `other`, `updateinfo` ni
  deltarpm. El parser de `.rpm` lee el header para indexar; la integridad
  del payload la verifica `rpm`, no el hub.
- **El rigor del cliente es del cliente.** `dnf5` 5.4.x avisa del fallo de
  firma de repomd y continúa; quien aprieta `repo_gpgcheck` hasta negarse
  es `microdnf` (y `apt`+`gpgv` en el lado Debian). El contrato promete
  que la firma servida abre con la clave publicada y que ningún cliente
  que la exige instala con otra clave.

## CONAN — repositorios Conan 2 hosted

### 1. Qué afirma este contrato

Un repositorio `conan` en modalidad **hosted** es un **remote de Conan 2**
con revisiones: `conan create` construye contra perfiles reales,
`conan upload` publica contra Hodei y `conan install` resuelve desde él en
una cache limpia. Lo que el remote preserva es la identidad versionada del
propio dialecto: la **recipe revision** (`rrev`), el **`package_id`** y la
**package revision** (`prev`) son segmentos que el servidor conserva
opacos — nunca derivados del nombre ni igualados a la versión — y los
binarios de dos `package_id` del mismo recipe viven bajo prefijos de asset
distintos (`packages/{n}/{v}/{u}/{c}/{rrev}/{pid}/{prev}/{file}`), de modo
que un `package_id` pide y recibe SUS bytes, no los del otro.

El scoping es el del hub: la credencial del remote se verifica como
credencial bearer del principal en el authenticate del dialecto
(`GET /v2/users/authenticate`, Basic de entrada, token de salida), y el
listing de un principal de otra cuenta no ve lo que no le toca.

### 2. Cómo se comprueba

- `tests/hsp159_conan_client.rs`: el cliente **conan 2.x real** (el del
  venv de la UAT, fijado por `CONAN_BIN` y por el assert de versión
  major 2) crea el mismo recipe con **dos perfiles** (opción `flavor`
  distinta → dos `package_id` de la misma `rrev`), lo sube con
  `conan upload`, lista el remote (`conan list "ref#*" -r`) y compara que
  el remote diga las mismas revisiones que la cache local; instala los dos
  perfiles en caches limpias y comprueba, por `package_id`, que el
  contenido bajado (el `flavor.txt` de cada binario) es el del perfil y
  que el sha256 de cada `conan_package.tgz` descargado es el del subido;
  con el servicio parado, el re-install de la cache caliente funciona sin
  red, y faltando el binario, el install falla. Otro principal no ve la
  receta de la cuenta primera.
- El **dialecto HTTP** (tabla de rutas v2, ping con
  `X-Conan-Server-Capabilities: revisions`, documentos de revisiones y
  snapshots) quedó fijado con un **spike contra `conan_server` 1.66** como
  remote de referencia, detrás de un proxy de registro; la gramática vive
  en `hodei_catalog::conan` (pura, con sus unitarios) y los efectos en
  `src/handlers/conan.rs`. La evidencia del spike y de la UAT está en
  `target/uat/hsp159/evidencia.md`.

### 3. Lo que este contrato NO promete

- **No hay proxy ni group para conan.** El upstream de un proxy conan
  necesita su dialecto validado con cliente real antes de poder
  declararse, y la resolución compuesta de un group lo mismo. Solo la
  hosted está probada y declarada.
- **No se sirve `DELETE`** (el `conan remove` contra el remote): la
  superficie hosted mínima no borra revisiones.
- **La deduplicación `checksum_deploy` no se anuncia.** El ping declara
  solo `revisions`; la subida es de ficheros completos.
- **El token del remote es la credencial del hub**, no un JWT emitido por
  el servidor de conan: el authenticate verifica la contraseña como
  credencial bearer y la devuelve como token. La semántica de scoping es
  la del hub, no una tabla de usuarios propia.

## FMT-06 — selección de ecosistema: Ansible Galaxy elegido, Conda aplazado

**Tarea:** HSP-168 (HAE-35). Ficha de selección, no contrato de formato:
ninguno de los dos ecosistemas se sirve todavía (no hay variante
`RepositoryFormat` ni fila en la matriz).

### Qué afirma esta ficha

El primer ecosistema adicional con demanda es **Ansible Galaxy**
(collections, API v3), y se recomienda abrir UNA tarea vertical de hosted.
**Conda queda aplazado** (KEEP_AS_IS) sin compromiso de implementación.

La selección se sustenta en uso real contrastado en el spike:

- **Audiencia.** El hub ya sirve al consumidor de IaC/automación (mirror de
  providers Terraform servido, registry privado de modules/providers en
  backlog); Ansible es la otra pata natural de esa automatización y ningún
  formato servido lo cubre. Conda solapa con la audiencia Python que PyPI ya
  atiende en hosted, proxy y group.
- **Trust.** El artefacto Galaxy es un tarball auto-descripto (MANIFEST.json
  y FILES.json con sha256 por fichero) y el protocolo verifica el sha256 del
  artefacto contra la API al instalar; tiene además firma GPG opcional por
  versión, encaje directo con la línea de políticas de confianza.
- **Cliente reproducible.** `ansible-core==2.19.6` fija el CLI completo por
  pip en un venv (python 3.14; el 3.15 del sistema rompe esta versión). El
  cliente real de Conda exige un binario externo (micromamba/miniconda).
- **Coste de hosted.** El dialecto Galaxy v3 son documentos JSON derivados
  del catálogo más un tarball; Conda exige generar `repodata.json` por
  subdir y servir variantes `.bz2`/`.zst`.

### Evidencia del spike (cliente real 2.19.6)

- **Remote de referencia real** (galaxy.ansible.com, galaxy_ng/Pulp): captura
  de wire con `ansible-galaxy collection install ansible.posix:2.2.1`
  anónima: descubrimiento `/api/`, documentos de colección y versiones
  paginados DRF, artefacto servido por cadena de 302 con `validate_token`
  hasta un objeto S3 presigned cuya clave es `artifact/{2}/{sha256}` — CAS
  por contenido, el mismo modelo de blobs del hub.
- **Fixture local mínimo** en el worktree (127.0.0.1, venv): instalación
  nativa de `hodei.spike:1.0.0` OK con el dialecto v3 completo; **rechazo
  nativo de artefacto corrupto** (`Mismatch artifact hash with downloaded
  file`); **publish nativo** OK con `Authorization: Token`, multipart con
  fichero base64 y campo `sha256` (verificado), y polling de tarea de
  import emulada síncrona.

### Lo que esta ficha NO promete

- **Ningún formato se sirve.** Sin variante `RepositoryFormat::Galaxy` ni
  `::Conda`, sin handler, sin fila en la matriz: la tarea vertical
  propuesta queda para decisión del supervisor, no creada.
- **El proxy de Galaxy no está probado**: el upstream real entrega por
  redirects firmados; no se declara proxy sin cliente verificado contra él.
- **La firma GPG no se ejercitó** (opcional sin keyring); los roles con la
  API legacy v1 quedan fuera de alcance.
- La evidencia completa (wire logs, comandos, comparación CAS vs adapter,
  incompatibilidades y propuesta de tarea) está en
  `target/uat/hsp168/evidencia.md` del worktree del spike.

## GALAXY — repositorios Ansible Galaxy hosted

### 1. Qué afirma este contrato

Un repositorio `galaxy` en modalidad **hosted** es un **servidor Ansible
Galaxy por API v3**: `ansible-galaxy collection publish` publica contra
Hodei y `ansible-galaxy collection install` / `verify` resuelven y
comparan contra él con el cliente real (`ansible-core` 2.19). La
identidad versionada es la del propio dialecto: el **fully qualified
collection name** `{namespace}.{name}` y la versión, declarados por el
`MANIFEST.json` que el artefacto lleva dentro (gzip + tar, miembro en la
raíz) y leídos en el publish. La superficie es la del cliente:

- `GET /galaxy/{repo}/api/` — discovery con `available_versions` (solo
  `v3`); el cliente llega aquí tras fallar la raíz del servidor, como
  hace con cualquier Galaxy.
- `GET /galaxy/{repo}/api/v3/collections/{ns}/{name}/` — documento de la
  collection, con `created_at`/`updated_at` que el cliente usa para
  invalidar su cache de respuestas.
- `GET /galaxy/{repo}/api/v3/collections/{ns}/{name}/versions/` —
  versiones paginadas al estilo DRF (`meta`, `links.next` absoluto,
  `data`), con `limit`/`offset` y `next` como ruta absoluta, la única
  forma que el cliente acepta.
- `GET .../versions/{version}/` — documento de versión: `download_url`
  (ruta absoluta), `artifact.sha256`, `metadata.dependencies` y `href`.
- `GET /galaxy/{repo}/artifact/{sha256}/{file}` — los bytes del
  artefacto desde el CAS; la URL lleva el digest que el documento
  declara, y el que se sirve es el mismo: es el que el cliente verifica
  al bajar (`Mismatch artifact hash with downloaded file` si no
  coincide).
- `POST /galaxy/{repo}/api/v3/artifacts/collections/` — publish:
  multipart con `sha256` y `file`, el fichero en base64 con
  `Content-Transfer-Encoding` (el dialecto MIME del cliente) y
  `Authorization: Token` con la credencial del hub. La superficie
  verifica el sha256 **en el commit**, lee la identidad del
  `MANIFEST.json`, y contesta la tarea de import que el cliente sondea
  en `api/v3/imports/collections/{task}/` (aquí síncrona: al leerla ya
  está `completed`).

El scoping es el del hub: la API key del cliente (`Authorization: Token
<key>`) se verifica como credencial bearer del principal, y un principal
de otra cuenta ni ve ni publica en un repositorio que no es suyo.

### 2. Cómo se comprueba

- `tests/hsp217_galaxy_client.rs`: el cliente **ansible-core 2.19 real**
  (el del venv de la UAT con Python 3.14, fijado por `GALAXY_BIN` y por
  el assert de versión) construye la collection con
  `ansible-galaxy collection build`, la publica con `collection publish`
  (dos veces: la repetición del mismo contenido es un eco, no una
  reescritura), la instala con `collection install` en un directorio
  limpio, la compara con `collection verify` ("Successfully verified"),
  corrompe un fichero del instalado y espera que el `verify` falle, y
  comprueba que otro principal ni instala ni publica; con el servicio
  parado, el re-install de la cache caliente no toca la red ("Nothing to
  do") y un requisito sin servir falla.
- El **rechazo del artefacto corrupto** va por el cliente real: un
  tarball cuyo `MANIFEST.json` no es el documento del formato se publica
  y la superficie lo rechaza con `HTTP Code: 400` sin guardar nada; el
  artefacto bueno se publica después en su versión libre. El **sha256
  mentiroso** (los bytes no son los que el formulario declara) va por el
  socket, porque el cliente real no puede decir esa mentira: la
  superficie lo rechaza con 400 y el digest mal formado también.
- La **gramática del dialecto** (tabla de rutas v3, documentos, página
  DRF, lectura del `MANIFEST.json` dentro del gzip/tar) vive en
  `hodei_catalog::galaxy` (pura, con sus unitarios) y los efectos en
  `src/handlers/galaxy.rs`; la derivación es del código del cliente
  (`ansible/galaxy/api.py` de 2.19.6), el mismo método que fijó conan
  contra `rest_routes.py`. La evidencia está en
  `target/uat/hsp217/evidencia.md`.

### 3. Lo que este contrato NO promete

- **No hay proxy para galaxy.** El upstream real (galaxy_ng/Pulp)
  entrega los artefactos por una cadena de redirects con tokens
  firmados; un proxy sin cliente verificado contra ese dialecto no se
  declara.
- **No hay firma GPG por versión.** Los documentos llevan `signatures:
  []` y el cliente no ejercita keyring: la firma es otro contrato.
- **Los roles con la API legacy v1 quedan fuera.** La superficie es de
  collections v3; `ansible-galaxy role` no la habla.
- **La resolución de dependencias de una collection** (el
  `metadata.dependencies` de su manifiesto) se sirve en el documento de
  versión, pero la suite solo publica collections sin dependencias: un
  grafo de dependencias entre collections servidas por el mismo hub no
  está ejercitado.

## TERRAFORM — registry de módulos privado (hosted) — FMT-04

**Tareas:** HSP-158 (HAE-25, registry de módulos privado) sobre el mirror de HSP-157 (HAE-24) · **Servicio medido:** `hodei-artifacts 0.2.0` · **Cliente fijado:** `terraform 1.10.1` (asdf, `.tool-versions` del worktree)

### 1. Qué afirma este contrato

Afirma que **un cliente `terraform` real resuelve e instala un módulo privado por versión** contra un repositorio Terraform **hosted** del hub, con el *module registry protocol v1*: `terraform init` de una fuente `127.0.0.1:PUERTO/hodei/demo/local` con restricción `version` pasa por discovery → índice de versiones → download con `X-Terraform-Get` → go-getter, que baja el tarball y **verifica el `?checksum=sha256:`** contra los bytes recibidos.

Las reglas, una por una:

- **Dos dialectos, una modalidad cada uno.** El formato Terraform sirve dos modalidades y cada una habla su protocolo: **proxy** → *provider network mirror* (HSP-157, `hodei_catalog::terraform`), **hosted** → *module registry protocol v1* (HSP-158, `hodei_catalog::terraform_modules`). La fila de la matriz queda `Terraform → [Proxy, Hosted]`: no es una variante de formato nueva, es el mismo ecosistema con dos contratos, y la modalidad decide el dialecto de la superficie `/terraform/{repositorio}/...`. Un repositorio hosted no sirve rutas de mirror y un proxy no sirve rutas de registry.
- **Service discovery.** El hub sirve `GET /.well-known/terraform.json` con `{"modules.v1": "/terraform/{repo}/v1/modules/"}`: nombra al **único** registry de módulos de la cuenta de la credencial (0 registries → `404`, varios → `409`, y la config de CLI nombra el repositorio explícito). El CLI pide ese documento **siempre por https** (`terraform-svchost/disco` lo fija): un hub http declara el servicio en la **config de CLI** (`host` + `services`, que es el `ForceHostServices` del disco) — es la vía que ejercita la UAT; con TLS el well-known es alcanzable por el cliente.
- **Índice de versiones.** `GET /v1/modules/{namespace}/{name}/{system}/versions` con la credencial Bearer del CLI config (`credentials "host" { token }`). El documento lleva `modules[0].versions[]` en orden; el cliente resuelve la restricción semver (las pre-release se ignoran salvo petición exacta, como el cliente hace).
- **Download autorizado.** `GET /v1/modules/{namespace}/{name}/{system}/{version}/download` responde **`204` sin cuerpo** con `X-Terraform-Get: /terraform/{repo}/artifacts/{ns}/{name}/{sys}/{version}.tar.gz?checksum=sha256:{digest}`. Un valor que empieza por `/` el cliente lo resuelve contra la URL pedida (el propio `registry/client.go` lo permite), y el checksum es el digest del CAS: el cliente verifica los bytes contra el contenido almacenado, no contra una promesa.
- **Artefacto por netrc.** go-getter **no** lleva la credencial del registry: su HTTP getter lee `~/.netrc` (o `$NETRC`) y manda `Basic`. La superficie del artefacto acepta el Bearer del hub y el **Basic de netrc cuya contraseña es la clave** (el login se ignora). La búsqueda de netrc es por `host:puerto`.
- **Publicación inmutable.** El CLI no publica módulos: la publicación la define el hub, `POST /terraform/{repo}/publish/{ns}/{name}/{sys}/{version}` con el tarball entero. El hub valida el gzip, calcula el sha256 **al commit** y devuelve el recibo (`201` con namespace/name/system/version/sha256/size_bytes/path). **Re-publicar una versión existente es `409`, también con los mismos bytes** (inmutabilidad por contrato, más estricta que la eco de galaxy); el catálogo rechaza además el contenido distinto en la carrera. Un tarball que no es gzip es `400`.
- **Acceso.** Misma autorización que el resto de superficies: `401` sin credencial verificable, `403` de Cedar (`UploadAsset`/`DownloadAsset`), `404` para el repositorio de otra cuenta (el init de otra cuenta no encuentra el módulo).
- **Offline.** El módulo instalado queda en `.terraform/modules` con su manifiesto: el `init` repetido con caché templada **no necesita red** (el instalador reutiliza lo instalado, `initwd/module_install.go`); un init de caché fría sin hub falla. El hub no tiene upstream: todo lo servido sale del CAS.

### 2. Cómo se verificó

- **Suite:** `tests/hsp158_terraform_modules_client.rs` (`TF_CLI`, por defecto `terraform`, fijado a 1.10.x) contra el servicio real del arnés: `terraform init` real con credencial de CLI config y netrc, instalación verificada por el manifiesto (`"Version":"1.0.0"`), re-publicación y mutación rechazadas (`409`), índice con las dos versiones, init por `2.0.0`, init de otra cuenta denegado, offline templado/frío, y el wire de discovery/download/artefacto por socket.
- **Gramática:** la tabla de rutas y los documentos viven en `hodei_catalog::terraform_modules` (pura, con unitarios de traversal y de documentos); los efectos en `src/handlers/terraform_modules.rs` y el despacho por modalidad en `src/handlers/terraform.rs`.
- **Dialecto contrastado** contra el código fuente de terraform v1.10.1 (`internal/registry/client.go`, `internal/registry/response/module_versions.go`, `internal/initwd/module_install.go`, `internal/getmodules/getter.go`), `terraform-svchost/disco` y `go-getter` v1.7 (`client.go` y `get_http.go`: checksum por query solo en descarga de archivo, netrc por host con puerto). La evidencia está en `target/uat/hsp158/evidencia.md`.

### 3. Lo que este contrato NO promete

- **Evidencia del mirror (proxy): protocolo y cliente nativo.** La modalidad proxy de Terraform está acreditada por `hsp157_terraform_mirror` (protocolo con `curl` contra un upstream falso) y por `hsp304_terraform_init`, que ejecuta el CLI `terraform` real con `provider_installation { network_mirror }` sobre https verificado: `init` instala el provider y el cliente verifica el hash `zh:` del zip. Terraform no envía credenciales al bajar el zip, así que el documento de versión declara una URL del zip firmada y de vida corta (HSP-313, `hsp313_signed_archive_url`; clave en `[signing]`, ver `docs/community/configuration.md`).
- **No hay provider origin registry.** El discovery devuelve solo `modules.v1`: el registry de providers de origen (AC3 condicional) **no se implementa** y nada no implementado se anuncia. El mirror de providers (HSP-157) sigue siendo la vía de providers.
- **No hay source `git::` para módulos privados.** El módulo privado se sirve por HTTP con checksum; una fuente git del propio cliente queda fuera por diseño (no se sustituye Git).
- **El well-known no es alcanzable por el cliente sobre http.** `disco` fija https; el documento existe y está ejercitado por socket, y el cliente real lo usa vía `services` de la config de CLI. Sobre un despliegue con TLS el well-known es el camino natural.
- **OpenTofu (`tofu`) no está ejercitado.** `tofu` no está instalado en este entorno y no se ha provisionado; el protocolo es el mismo, pero no se declara sin ejecución.
- **Multi-registry por cuenta en el well-known.** Con más de un registry de módulos por cuenta, el discovery responde `409` y la selección es de la config de CLI; no hay aliasing por host.

## FMT-07 — selección de cartera: modelos Hugging Face elegidos, APK runner-up

**Tarea:** HSP-169 (HAE-36). Ficha de selección de cartera, no contrato de
formato: ningún candidato se sirve todavía (no hay variante
`RepositoryFormat` ni fila en la matriz).

### Qué afirma esta ficha

De la cartera (APK Alpine, Composer, RubyGems, Pub/Swift, Git LFS,
VSIX/Open VSX, CRAN/p2, modelos Hugging Face) se elige por necesidad el
slice de **model artifacts / hub-compatible** (lectura con el CLI `hf` de
`huggingface_hub`) y se recomienda abrir UNA tarea vertical de hosted.
**APK Alpine queda como runner-up** (próxima ficha si hay demanda); Git LFS
y VSIX quedan condicionados por exclusiones de servicio; Composer,
RubyGems, Pub/Swift y CRAN/p2 sin cliente instalable en el entorno de
trabajo.

La selección se sustenta en uso real contrastado en el spike:

- **Cliente reproducible.** `huggingface_hub==1.2.3` instala el CLI `hf`
  por pip en venv (python 3.14) y quedó demostrado contra el remote real y
  contra un fixture local. Ruby/PHP ausentes en el host; apk es binario
  estático sin gestor de fijación equivalente.
- **Esfuerzo de protocolo mínimo.** Lectura = JSON de revision
  (`/api/models/{repo}/revision/{rev}`) + `HEAD resolve` con
  `X-Repo-Commit`/`X-Linked-Etag` (sha256)/`X-Linked-Size` + `GET` streaming
  del blob. Sin firma en el protocolo base; APK exige índice firmado RSA por
  repo×arch.
- **Encaje CAS exacto.** El etag del formato ES el sha256 y el cliente nombra
  sus blobs por sha256: CAS por contenido, el mismo modelo del hub.
- **Streaming por diseño.** Los pesos de modelos son los artefactos grandes
  típicos: 256 MiB descargados con RSS acotado en cliente (~99 MB) y
  servidor (~25 MB), sin buffers completos.

### Evidencia del spike (cliente real 1.2.3)

- **Remote real** (huggingface.co): wire de `hf download prajjwal1/bert-tiny`
  en dos modos — xet por defecto (302 con metadata + token de
  `xet-read-token` y CAS nativo invisible a httpx) y ruta HTTP clásica con
  `HF_HUB_DISABLE_XET=1` (302 con metadata + `GET` del resolve => 200 con
  los bytes).
- **Fixture local mínimo**: repo `hodei/spike-model` con `model.safetensors`
  de 256 MiB y `config.json`; descarga de fichero y de repo completo OK con
  checksum sha256 verificado (`9f215d17...` == etag == nombre de blob).
- **Hallazgo de trust**: en la ruta HTTP clásica el cliente solo verifica el
  TAMAÑO; el sha256 declarado en `X-Linked-Etag` no se verifica en cliente
  (una descarga servida corrupta fue ACEPTADA por el CLI, detectada solo por
  `sha256sum` externo). En hosted la garantía de integridad debe venir del
  CAS del servidor — conexión directa con HSP-105.

### Lo que esta ficha NO promete

- **Ningún formato se sirve.** Sin `RepositoryFormat::HfHub` ni fila en la
  matriz: la tarea vertical propuesta queda para decisión del supervisor,
  no creada.
- **Model inference, Git server y marketplace IDE quedan FUERA** (encargo
  explícito): el dialecto probado es HTTP puro sin git, sin xet nativo y sin
  inferencia. El binario/extension de VSIX sí entra en un futuro slice, pero
  esta ficha no lo abre.
- **Upload/publish no probado** (solo lectura); el dialecto de escritura
  clásica y el protocolo xet quedan para decisión posterior.
- La evidencia completa (score de la cartera, wire logs, incompatibilidades,
  exclusiones y propuesta de tarea) está en
  `target/uat/hsp169/evidencia.md` del worktree del spike.

## HF — repositorios Hugging Face hosted (lectura)

> **Limitación vigente (HSP-218, revisión de integración).** El release **no
> tiene ingesta por API** para HF: el handler solo admite `GET`/`HEAD` y el
> contenido entra únicamente por la siembra del operador
> (`HODEI_SEED_HF`, usada por la suite). Por eso `hf` **no es una modalidad
> servida**: `serves_modes` es vacío, no se ofrece en `/api/v1/capabilities`,
> no se puede crear un repositorio `hf` por API y la matriz de conformidad
> lo marca `NOT_SUPPORTED` en hosted, proxy y group. Lo que sí está probado
> con cliente real (`hsp218_hf_client`, `hf` 1.2.3) es la **lectura** del
> contenido sembrado. Declarar `hosted` exige antes una ingesta por API.

### 1. Qué afirma este contrato

Un repositorio `hf` (sembrado, de solo lectura) es un **hub de modelos en el
dialecto de lectura de Hugging Face**: `hf download` del CLI `hf`
(`huggingface_hub` 1.2.3, que habla httpx) resuelve un modelo contra
Hodei con el cliente real. El dialecto es el que el spike de HSP-169
verificó contra huggingface.co (ruta HTTP clásica, sin xet) y contra un
fixture local; la superficie, con `HF_ENDPOINT=http://{hub}/hf/{repo}`:

- `GET /hf/{repo}/api/models/{owner}/{name}/revision/{rev}` — documento
  de revisión: `id` (el `repo_id`), `sha` (el commit) y `siblings[]`
  (`rfilename`). El commit es **el digest del listado del snapshot**
  (`rfilename sha256` ordenado): content-addressed como el CAS que lo
  sirve, y solo cambia cuando cambia el contenido que declara. Una
  revisión que no es `main` ni el commit del snapshot es un 404: el
  catálogo guarda el estado actual, no una historia de ramas.
- `HEAD /hf/{repo}/{owner}/{name}/resolve/{rev}/{rfilename}` — una `302`
  con las cabeceras del wire real: `X-Repo-Commit`, `X-Linked-Etag`
  (el sha256 del fichero, entre comillas), `X-Linked-Size` y una
  `Location` **relativa** (ruta desde la raíz del servidor, sin scheme ni
  host — la forma del hub clásico). El cliente solo sigue los redirects
  relativos; la Location lleva el modelo, el digest y el fichero porque
  la URL nombra lo que sirve.
- `HEAD`/`GET {Location}` (`/hf/{repo}/cdn-lfs/{owner}/{name}/{sha256}/
  {rfilename}`) — la metadata se lee de la respuesta final del redirect y
  los bytes van **en streaming** desde el CAS, a tramos de 64 KiB, sin
  buffer completo en memoria: los pesos de un modelo son el artefacto
  grande típico. El `ETag` de la respuesta es el sha256 del contenido.
- `GET` del resolve directo: los mismos bytes, sin redirect.

La identidad del blob es su sha256 — etag, nombre de cache del cliente y
digest del CAS son el mismo valor —: CAS exacto, el mismo encaje que la
ficha FMT-07 prometió. El servidor no emite `X-Xet-*`, así que el cliente
cae a la ruta HTTP clásica (decisión del fixture del spike, documentada).

El scoping es el del hub: el cliente lleva el token como
`Authorization: Bearer`, se verifica como credencial bearer del principal
y otro principal no ve el repositorio de la cuenta primera. Un modelo del
catálogo es el conjunto de assets `artifacts/{owner}/{name}/{rfilename}`;
la escritura no se declara (el upload clásico y el xet quedan fuera) y el
contenido entra por la siembra del hub.

### 2. Cómo se comprueba

- `tests/hsp218_hf_client.rs`: el cliente **`hf` real** (el del venv de
  la UAT, fijado por `HF_BIN` y por el assert de versión) descarga un
  modelo pequeño sembrado, y el sha256 de lo instalado es el etag que la
  superficie declaró; con el servicio parado, la cache caliente resuelve
  sin red; otro principal no ve el modelo de la cuenta primera. La
  gramática vive en `hodei_catalog::hf` (pura, con sus unitarios) y los
  efectos en `src/handlers/hf.rs`.
- **Test negativo documentado** (límite del cliente): en la ruta HTTP
  clásica el CLI **solo verifica el tamaño**, nunca el sha256 declarado
  en `X-Linked-Etag` — una descarga servida corrupta es ACEPTADA por el
  cliente real y solo un `sha256sum` externo la detecta (hallazgo del
  spike, reproducido en la suite con el fixture del spike en modo
  `CORRUPT=1`). En hosted la garantía de integridad la cubre el **CAS del
  servidor** (conexión con HSP-105): el almacén no entrega bytes que no
  son su digest, y el etag que el cliente usa para nombrar su cache es el
  digest que el CAS verificó al servir.

### 3. Lo que este contrato NO promete

- **No hay escritura.** Ni el upload clásico ni el protocolo xet se
  declaran: la superficie es de lectura y el contenido entra por la
  siembra del hub. El dialecto de escritura queda para decisión
  posterior.
- **No hay proxy ni group para hf.** El upstream real anuncia xet y
  entrega por redirects firmados a un CDN: otro dialecto que no se ha
  probado con cliente.
- **Una sola revisión servida.** `main` y el commit del snapshot; no hay
  ramas ni historial de commits en la hosted.
- **El rigor del cliente es del cliente.** El CLI no verifica el sha256
  de la descarga (solo el tamaño): es el límite documentado arriba, y el
  hub lo cubre desde el CAS, no desde el wire.
