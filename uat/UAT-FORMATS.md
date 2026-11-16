# Formatos de paquetes — contrato de aceptación

**Tareas:** HSP-109 (HAR-15, Maven 2 hosted) · HSP-110 (HAR-16, Maven 2 proxy y group) · HSP-111 (HAR-17, npm hosted, proxy y group) · **Servicio medido:** `hodei-artifacts 0.2.0`

Este documento reúne los contratos de aceptación de los formatos de paquetes
nativos. Hoy tiene tres secciones, MAVEN (hosted), MAVEN-REMOTE (proxy y
group) y NPM; cada formato nuevo añade la suya.

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
  versión tienen que ser los del nombre del fichero.
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
| `crates/hodei-catalog/src/pypi.rs` (unitarias) | Normalización de nombres (PEP 503), nombres de fichero de wheel y sdist, versiones (PEP 440), el documento de un proyecto, la negociación de la representación, el escapado del HTML, las rutas de la superficie y la comprobación de una subida contra sus bytes, como funciones puras. |
| `tests/hsp112_pypi_e2e.rs` | El protocolo con `curl` contra el servicio real: la subida multipart que habla `twine`, el índice en JSON y en HTML con el mismo hash, el `ETag` y el `304`, los cuatro nombres del mismo proyecto y el `404` del que no existe, el hash que no es el de los bytes (`400`, y no se publica), el `yanked` de una versión y su descarga, el group que une los proyectos de sus miembros, el proxy que vuelve a pintar el índice del upstream sin su HTML y con los valores escapados, y los `401` y `405`. |
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
- **Sin firmas ni atestaciones.** `twine` no firma y la superficie no acepta
  ni verifica firmas (PEP 740).
- **El retiro es una extensión propia.** PyPI no tiene API para retirar una
  versión, y `pip` no la invoca: se retira con `POST /pypi/{repo}/yank`.
- **Una publicación son dos escrituras.** La distribución y después el
  documento; si el servicio cae entre ambas, la distribución queda en el
  catálogo sin anunciarse, y reintentar con los mismos bytes completa la
  publicación.
- **El proxy no reescribe el JSON entero.** Se reescriben las URL de las
  distribuciones; el resto del documento es el del upstream.
- **El índice de un proxy no se puede enumerar.** Sale vacío: el cliente
  pregunta proyecto a proyecto, porque lo que hay en el upstream no se puede
  saber sin pedirlo.
- **Distribuciones en memoria.** Se cargan enteras (tope de contenido del
  servicio) y no admiten `Range`.
- **El sdist se construye en el cliente.** Instalar desde un sdist necesita las
  herramientas de construcción allí (`--no-build-isolation`); el servicio no
  las trae ni las ejecuta.
