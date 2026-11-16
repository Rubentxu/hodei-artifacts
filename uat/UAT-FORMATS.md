# Formatos de paquetes — contrato de aceptación

**Tareas:** HSP-109 (HAR-15, Maven 2 hosted) · **Servicio medido:** `hodei-artifacts 0.2.0`

Este documento reúne los contratos de aceptación de los formatos de paquetes
nativos. Hoy solo tiene una sección, MAVEN; cada formato nuevo añade la suya.

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

- **Proxy y grupo Maven.** Solo `hosted`; el proxy y los grupos de Maven son
  de HSP-110.
- **Metadatos a nivel de grupo** (`maven-metadata.xml` de plugins) no se
  derivan; se responde `404`.
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
