# Quickstart comunitario: el primer publish y el primer pull

Este tutorial levanta Hodei Artifacts, crea una cuenta, declara un
repositorio **Raw** y otro **Maven**, publica un blob y un paquete Maven, los
vuelve a descargar y comprueba sus digests. **No hace falta compilar Rust,
levantar servicios a mano ni editar una línea de código.**

Todo ocurre dentro de un directorio temporal: el `HOME`, la configuración, la
base de datos, el almacén de bytes, los logs y el token. El comando de
limpieza borra ese directorio y nada más.

> **Estado:** este camino existe y se ejecuta de punta a punta en esta
> versión. La distribución es el `tar.gz` que produce
> `scripts/package-release.sh`: **no hay imagen OCI en esta versión**, así que
> el camino reproducible es el paquete.

## 1. Qué necesitas

- `curl`, `openssl`, `tar`, `sha256sum` y `sha1sum` (presentes en cualquier
  distribución de Linux y en macOS con las herramientas de línea de comandos).
- La distribución de Hodei Artifacts.

No necesitas Rust, ni Cargo, ni Docker, ni Podman, ni red.

## 2. Consigue la distribución

Descarga el `tar.gz` de la última versión desde la página de releases y
verifica su digest contra el fichero `.sha256` que va al lado:

```bash
sha256sum -c hodei-artifacts-api-<version>-<target>.tar.gz.sha256
tar -xzf hodei-artifacts-api-<version>-<target>.tar.gz
```

Si prefieres construirla tú mismo (una sola vez, y esto **sí** compila Rust):

```bash
scripts/package-release.sh
```

## 3. Ejecuta el camino

```bash
cd hodei-artifacts-api-<version>-<target>
./community-quickstart.sh run
```

En un repositorio checkout, el mismo comando es:

```bash
scripts/community-quickstart.sh run --archive dist/hodei-artifacts-api-<version>-<target>.tar.gz
```

El script imprime nueve pasos y termina con los digests de lo publicado.
Si algo falla, dice en qué paso y con qué código de estado: no se queda
callado.

## 4. Qué hace, paso a paso

| Paso | Qué ocurre | Cómo se comprueba |
| --- | --- | --- |
| 1 | Se crea el laboratorio: un directorio temporal con su propio `HOME` | El script imprime la ruta |
| 2 | Se arranca el servicio desde el paquete de release | `/health` responde `200` |
| 3 | Se acuña un token JWT **temporal** | El secreto es aleatorio y efímero |
| 4 | Se declara la cuenta: repositorio Raw y repositorio Maven | `POST /api/v1/catalog/repositories` responde `201` en los dos |
| 5 | Se publica un blob Raw | `PUT /repository/lab-raw/...` responde `201` con su digest |
| 6 | Se publica un paquete Maven (jar, pom y sus `sha1`) | `201`, `201`, `200`, `200` |
| 7 | Se descargan los dos | El `sha256sum` de lo descargado es el digest publicado |
| 8 | Se reinicia el servicio | Mismo digest, mismo catálogo, mismo token |
| 9 | Se resume | El script dice dónde está el laboratorio y cómo limpiarlo |

### El bootstrap declarativo

La cuenta no se "crea" en el sentido de un registro de cuentas: en Hodei la
cuenta es la del **principal verificado**, y lo que se declara es el
principal y lo que puede hacer. El laboratorio escribe tres cosas:

- **La cuenta y el principal**: `lab` y `pioneer`. El sujeto del token es
  `hrn:hodei:iam::lab:User/pioneer`, y la cuenta que lleva dentro es la que
  aísla: un principal de otra cuenta no coincide con la política y la
  decisión es un rechazo.
- **La política Cedar** (`config/community/policies.cedar`): el principio de
  mínimo privilegio. El principal del laboratorio puede crear y leer sus
  repositorios y publicar y descargar en ellos. **No** puede promover al
  canal de confianza, **no** puede alcanzar la superficie administrativa y
  **no** puede tocar otra cuenta.
- **El esquema Cedar** (`config/community/schema.json`): las acciones que el
  servicio autoriza con. Un paquete que no las declara no arranca.

Los repositorios se declaran por HTTP, con el verbo del catálogo:

```json
{"name": "lab-raw",   "format": "raw",    "mode": "hosted"}
{"name": "lab-maven", "format": "maven2", "mode": "hosted",
 "settings": {"maven": {"version_policy": "release"}}}
```

## 5. Sigue usando el servicio

El comando `run` deja el servicio **parado** y el laboratorio con todo lo
publicado. Para levantarlo otra vez con los mismos datos:

```bash
scripts/community-quickstart.sh serve --lab-dir /tmp/hodei-community-lab.XXXXXX
```

El token del laboratorio está en `<lab>/token` (permisos `600`) y el puerto
lo dice el propio comando al arrancar. Con él:

```bash
TOKEN="$(cat /tmp/hodei-community-lab.XXXXXX/token)"
PUERTO=42010   # el que imprimió `serve`

# El catálogo de la cuenta
curl -s -H "Authorization: Bearer $TOKEN" \
  http://127.0.0.1:$PUERTO/api/v1/catalog/repositories

# El blob publicado
curl -s -H "Authorization: Bearer $TOKEN" \
  http://127.0.0.1:$PUERTO/repository/lab-raw/quickstart/hello.txt
```

### Con Maven (opcional)

El cliente Maven habla con el mismo servicio. La credencial viaja como
cabecera `Authorization`, en un `settings.xml` propio, nunca en la línea de
comandos:

```xml
<settings>
  <servers>
    <server>
      <id>lab-maven</id>
      <configuration>
        <httpHeaders>
          <property>
            <name>Authorization</name>
            <value>Bearer <el-token-del-laboratorio></value>
          </property>
        </httpHeaders>
      </configuration>
    </server>
  </servers>
</settings>
```

```bash
mvn -B -s settings.xml deploy \
  -DaltDeploymentRepository=lab-maven::http://127.0.0.1:$PUERTO/repository/lab-maven
```

Con Maven 3.9 el destino se declara con `-DaltDeploymentRepository`; con
versiones anteriores, con `-Durl`. Lo que se guarda es lo mismo: el jar, el
POM y sus `sha1`, y el servicio los deriva del contenido.

Este camino **sí** usa la red la primera vez (Maven baja sus plugins), así
que es opcional: el camino por defecto del tutorial no necesita red.

### Central (opcional y acotado)

El camino **por defecto no usa la red**: el paquete Maven del tutorial es un
fichero local. Si quieres un grupo de Maven que resuelva contra Central,
decláralo como un repositorio **proxy** y declara el host permitido en la
configuración del despliegue:

```toml
[proxy]
allowed_hosts = ["repo.maven.apache.org"]
```

Mientras `allowed_hosts` esté vacío, el servicio no sale a la red: un proxy
no resuelve hasta que quien opera el despliegue lo permite.

## 6. Limpia el laboratorio

```bash
scripts/community-quickstart.sh cleanup --lab-dir /tmp/hodei-community-lab.XXXXXX
```

El comando se niega a borrar un directorio que no declare(se) laboratorio
(un fichero `.hodei-community-lab` con la marca correcta) y se niega a
borrar `/`, tu `HOME` o cualquier ruta dentro de él. **No toca nada fuera
del directorio que le digas.**

## 7. Errores frecuentes

### `401 Unauthorized`

La credencial no está, o no es válida. Comprueba, por este orden:

1. **¿Hay cabecera `Authorization: Bearer <token>`?** Sin cabecera, la
   respuesta es `401` antes de mirar nada más.
2. **¿El token ha caducado?** El del laboratorio vive una hora. Se acuña
   uno nuevo y se vuelve a usar.
3. **¿El secreto es el del servicio?** El token se firma con el secreto del
   laboratorio (`<lab>/secret`). Si el servicio arrancó con otro secreto, el
   token es inválido: no es un error de permisos, es una credencial que no
   verifica.
4. **¿Issuer y audiencia son los del despliegue?** El token declara `iss` y
   `aud`, y el servicio los exige. Si no coinciden, el `401` es correcto.
5. **¿La cuenta del token es la del sujeto?** El `sub` es
   `hrn:hodei:iam::<cuenta>:User/<usuario>` y el `account` tiene que ser la
   misma cuenta. Si no lo son, el token se rechaza.

Un `401` **nunca** significa "no tienes permiso": eso es un `403`.

### `403 Forbidden`

La credencial es válida y el servicio dice que ese principal no puede. La
decisión es de Cedar, y es por repositorio:

1. **¿La política concede la acción?** El laboratorio concede
   `CreateRepository`, `ReadRepository`, `UploadAsset` y `DownloadAsset`.
   Una acción que no aparece en `config/community/policies.cedar` no se
   concede.
2. **¿El principal es exactamente el de la política?** El principal de la
   política es `hrn:hodei:iam::lab:User/pioneer`. Otro usuario, otra cuenta
   u otro caso en el HRN es otro principal, y no coincide.
3. **¿El recurso es el repositorio de esa cuenta?** Los repositorios
   pertenecen a la cuenta del principal verificado.
4. **Mira el log del servicio**: la petición llega con su actor, su
   operación, su recurso y su resultado. Nunca con el token.

El laboratorio lo demuestra en cada ejecución: tras el primer pull,
`run` firma un token válido de **otra cuenta** (`other/intruder`) y comprueba
que `POST /api/v1/catalog/repositories` responde `403` (no `401`) y que no
puede leer el blob del laboratorio. La prueba
`tests/hsp241_community_quickstart.rs` lo contrasta contra el tarball real.

Un `403` en `POST /api/v1/catalog/repositories` casi siempre es la política:
crear un repositorio necesita `CreateRepository`, y un laboratorio que sólo
publica no la tiene.

### `503 Service Unavailable`

El servicio está vivo pero una dependencia no está:

1. **El catálogo o el almacén de bytes.** `503` aquí significa que el
   catálogo no puede responder. Mira el log: el arranque dice qué ruta de
   datos usa (`rocksdb.path` y `content.cas_path`) y si el directorio
   existe y se puede escribir.
2. **Un proxy sin salida.** Si el repositorio es *proxy* y su contenido no
   está en la caché, un `503` explícito es la respuesta: el servicio no sale
   a la red porque `proxy.allowed_hosts` está vacío, o porque el upstream no
   respondió. Declara el host permitido y vuelve a intentarlo.
3. **El verificador no pudo responder.** Si el autenticador no está
   disponible, la respuesta es `503` y no `401`: un `401` ante una caída
   mandaría a cambiar una credencial que estaba bien.

> **Qué está contrastado y qué no.** Ningún test del quickstart provoca un
> `503`. El laboratorio verifica los tokens con un secreto HS256 local, que
> no es un verificador remoto que pueda caerse, y simularlo exigiría
> inventar una infraestructura (un emisor externo) que el laboratorio no
> tiene. Lo descrito en el punto 3 es el comportamiento declarado del
> servicio, no una comprobación ejecutada por esta guía.

### `404 Not Found`

Un `404` en la superficie de contenido no distingue entre "no existe" y "no
es tuyo": la respuesta es la misma a propósito, para no revelar la
existencia de lo ajeno. Comprueba que el repositorio existe en el catálogo
de **tu** cuenta.

### `409 Conflict`

La dirección ya tiene otro contenido y el repositorio publica una sola vez
(lo normal). Es lo que impide que un artefacto publicado cambie de
identidad por debajo.

## 8. Seguridad por defecto

El laboratorio y la producción **no comparten secretos ni permisos**:

- El laboratorio se cifra con un secreto **aleatorio y efímero**, que existe
  sólo dentro del directorio del laboratorio. No está en ningún fichero de
  configuración: viaja en el entorno del servicio.
- El perfil de producción (`config/release.toml`) **no declara ningún
  secreto** y no abre ninguna superficie de contenido.
- El perfil del laboratorio vive en el directorio del laboratorio, no en el
  repositorio: `config/default.toml`, `config/release.toml` y
  `config/development.toml` no se tocan.

Y se puede comprobar:

```bash
# La producción sin secreto arranca cerrada: la superficie de contenido no
# está montada, las superficies de negocio responden 401 y el health check
# sigue siendo público.
scripts/community-quickstart.sh guard
```

El `guard` comprueba tres cosas con el binario real:

1. El perfil de producción no declara ningún secreto.
2. Sin secreto, el servicio **arranca cerrado**: `PUT /repository/...` no
   está montado, `POST /api/v1/catalog/repositories` y `GET /api/v1/search`
   responden `401`, y `/health` sigue respondiendo `200`.
3. Con un secreto y **sin política**, el arranque **falla de forma
   explícita** (`Production security requires auth_issuer, auth_audience,
   authorization_policy_path and authorization_schema_path together`), y con
   una política que no se puede leer también (`cannot read authorization
   policy bundle`). Un despliegue que no puede decidir no se levanta como si
   fuera válido.

## 9. Qué queda fuera

- **No hay imagen OCI** en esta versión: el camino reproducible es el
  `tar.gz`.
- El camino por defecto **no usa la red**: el paquete Maven del tutorial es
  un fichero local. Central es un modo opt-in y acotado (§5).
- El `guard` es una comprobación de la plataforma, no del laboratorio: no
  levanta ninguna superficie de contenido.

## 10. Regenerar la demo visual

La animación que enlazan `README.md` y `README.es.md` (`docs/community/quickstart-demo.gif`) no es una captura manual: la produce `scripts/render-demo.sh`, que graba con `asciinema` una ejecución real de `scripts/community-quickstart.sh run` (arranque, token, publicación y descarga con digests, reinicio y 401 anónimo) y renderiza el GIF con `agg`. La grabación de origen se versiona en `docs/community/quickstart-demo.cast`.

```bash
scripts/render-demo.sh --archive dist/hodei-artifacts-api-<version>-<target>.tar.gz
scripts/render-demo.sh --render-only   # solo vuelve a renderizar el GIF desde el .cast
```

Requiere `asciinema` (3.x) y `agg`. La demo no incluye la vista de confianza de la consola: el flujo del quickstart no la ejercita. Los digests, el puerto y la ruta del laboratorio cambian en cada grabación.
