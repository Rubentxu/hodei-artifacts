# Guías de usuario por ecosistema

Este documento reúne las rutas reales, modalidades soportadas, límites,
configuración segura de credenciales y recetas de cliente para cada ecosistema
que sirve **Hodei Artifacts**.

> **Regla de producción (TLS y firmas):** en producción **siempre** debes
> exponer Hodei detrás de TLS (`public_base_url = "https://..."`) y **nunca**
> desactivar la verificación TLS ni la verificación de firmas/integridad
> (claves GPG en APT/RPM, digests/firmas en OCI y Helm-OCI, `integrity` SRI en
> npm, SHA-1/SHA-256 en Maven y PyPI). Los ejemplos con `http://127.0.0.1:$PORT`
> que aparecen en esta guía son **exclusivos para laboratorio local offline**
> (`localhost` / `127.0.0.1`).

---

## 1. Matriz de capacidades (`GET /api/v1/capabilities`)

La fuente única de verdad de lo que esta versión sirve vive en
[`RepositoryFormat::served()`](../../crates/hodei-catalog/src/model.rs),
[`RepositoryFormat::serves_modes()`](../../crates/hodei-catalog/src/model.rs)
y [`conformance.rs`](../../crates/hodei-catalog/src/conformance.rs), y se
expone en tiempo de ejecución en `GET /api/v1/capabilities`:

| Formato (`wire`) | `hosted` | `proxy` | `group` | Prefijo de ruta HTTP | Notas / límites |
| --- | --- | --- | --- | --- | --- |
| `raw` | Sí | Sí | Sí | `/repository/{repo}/...` | Ver [quickstart.md](quickstart.md) |
| `oci` | Sí | Sí | Sí | `/v2/{repo}/...` | Soporta artefactos OCI y Helm-OCI |
| `maven2` | Sí | Sí | Sí | `/repository/{repo}/...` | Releases inmutables; snapshots reemplazables |
| `npm` | Sí | Sí | Sí | `/npm/{repo}/...` | `unpublish` rechazado; `integrity` SRI verificado |
| `pypi` | Sí | Sí | Sí | `/pypi/{repo}/...` | PEP 503/691/592; límite `max_body_size` + 256 KiB |
| `cargo` | Sí | No | No | `/cargo/{repo}/...` | Sparse registry (`config.json`); `proxy`/`group` rechazados |
| `go` | Sí | Sí | Sí | `/go/{repo}/...` | Grupo resuelve versión completa desde un único miembro |
| `nuget` | Sí | Sí | No | `/nuget/{repo}/...` | NuGet v3; `group` rechazado en dialecto NuGet |
| `apt` | Sí | No | No | `/apt/{repo}/...` | Índice `Packages`/`Release` + `Release.gpg` + clave pública |
| `rpm` | Sí | No | No | `/yum/{repo}/...` | Repodata `repomd.xml` + `repomd.xml.asc` + `/key.asc` |
| `terraform` | Sí | Sí | No | `/terraform/{repo}/...` | Network mirror de providers (proxy) y registry privado de módulos (hosted); el mirror solo habla con upstreams que sirvan ese protocolo |
| `conan` | Sí | No | No | `/conan/{repo}/...` | Conan 2: recipes y binarios por package id (solo hosted) |
| `galaxy` | Sí | No | No | `/galaxy/{repo}/...` | Colecciones de Ansible Galaxy por API v3 (solo hosted) |
| `helm` | No | No | No | — | Declarado pero no servido: los charts usan `oci` (`/v2/`) |

### Límites de tamaño y política de upstream

- **Tamaño máximo de cuerpo (`max_body_size`)**: `10 MiB` (`10 * 1024 * 1024`
  bytes) por defecto en `src/config.rs`, más `256 KiB` de margen de sobre
  multipart (`MULTIPART_ENVELOPE_SLACK`) en `/pypi`, `/cargo`, `/apt`, `/yum`,
  `/go` y `/nuget`.
- **Política de upstream (fail-closed por defecto)**:
  1. Nivel de despliegue (`config` / `HODEI_PROXY__ALLOWED_HOSTS`): `[proxy]`
     `allowed_hosts = [...]`. Si está vacío, el servicio **no sale a la red**
     (modo offline estricto).
  2. Nivel de repositorio (`POST /api/v1/catalog/repositories`): campo
     `settings.upstream` (`id`, `url`, `scopes`, `credential_ref`,
     `require_checksum`, `auth`, `gpg_key_armor`) y `settings.cache`.
- **Grupos**: campo `settings.group.members` con la lista ordenada de claves de
  repositorios miembro.

---

## 2. Recetas por ecosistema

### Maven 2 (`maven2`)

- **Estado:** `VERIFIED` (ejercitado por `tests/hsp243_user_guides.rs` y `tests/hsp109_maven_e2e.rs`)
- **Versión de cliente:** Apache Maven `3.9+`
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream` (`id`, `url`, `scopes`, `require_checksum`)
- **Rutas reales:**
  - `hosted`: `PUT /repository/{repo}/{groupPath}/{artifact}/{version}/{file}` y `GET /repository/{repo}/...`
  - `proxy`: `GET /repository/{repo}/...`
  - `group`: `GET /repository/{repo}/...`
  - Creación de repositorio: `POST /api/v1/catalog/repositories` con `{"name": "doc-maven", "format": "maven2", "mode": "hosted", "settings": {"maven": {"version_policy": "release"}}}`
- **Credenciales seguras:** usa `~/.m2/settings.xml` (permisos `600`) con cabecera `Authorization: Bearer ${env.HODEI_TOKEN}` (nunca en la URL ni en argumentos CLI).

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
# 1. Crear repositorio Maven hosted
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-maven","format":"maven2","mode":"hosted","settings":{"maven":{"version_policy":"release"}}}'

# 2. Publicar POM y JAR
curl -sS -X PUT "http://127.0.0.1:${PORT}/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.pom" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  --data-binary @demo-1.0.0.pom

curl -sS -X PUT "http://127.0.0.1:${PORT}/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.jar" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  --data-binary @demo-1.0.0.jar

# 3. Descargar y comprobar digest
curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.jar" \
  -o downloaded.jar
sha256sum demo-1.0.0.jar downloaded.jar
```

---

### npm (`npm`)

- **Estado:** `VERIFIED` (ejercitado por `tests/hsp243_user_guides.rs` y `tests/hsp111_npm_e2e.rs`)
- **Versión de cliente:** `npm` `10+`
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream` (`id`, `url`, `scopes`, `require_checksum`)
- **Rutas reales:**
  - `hosted`: `PUT /npm/{repo}/{package}` (con `@scope%2fpkg` para paquetes con scope), `GET /npm/{repo}/{package}`, `GET /npm/{repo}/{package}/-/{tarball}`
  - `proxy`: `GET /npm/{repo}/...`
  - `group`: `GET /npm/{repo}/...`
  - Creación: `POST /api/v1/catalog/repositories` con `{"name": "doc-npm", "format": "npm", "mode": "hosted"}`
- **Credenciales seguras:** en `.npmrc` usa `//artifacts.example.com/npm/doc-npm/:_authToken=${HODEI_TOKEN}` (mantén TLS activo en producción; no uses `strict-ssl=false`).

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
# 1. Crear repositorio npm hosted
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-npm","format":"npm","mode":"hosted"}'

# 2. Publicar paquete con scope (@acme/demo 1.0.0)
curl -sS -X PUT "http://127.0.0.1:${PORT}/npm/doc-npm/@acme%2fdemo" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  --data-binary @npm-publish-payload.json

# 3. Consultar packument y descargar tarball
curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/npm/doc-npm/@acme%2fdemo"

curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/npm/doc-npm/@acme/demo/-/demo-1.0.0.tgz" \
  -o demo-1.0.0.tgz
```

---

### PyPI (`pypi`)

- **Estado:** `VERIFIED` (ejercitado por `tests/hsp243_user_guides.rs` y `tests/hsp112_pypi_e2e.rs`)
- **Versión de cliente:** `pip` `24+` / `twine` `5+`
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream` (`id`, `url`, `scopes`, `require_checksum`)
- **Rutas reales:**
  - `hosted`: `POST /pypi/{repo}/` (multipart `:action=file_upload`, `sha256_digest`, `content`), `GET /pypi/{repo}/simple/{project}/`, `GET /pypi/{repo}/packages/...`
  - `proxy`: `GET /pypi/{repo}/simple/...`
  - `group`: `GET /pypi/{repo}/simple/...`
  - Límite de carga: `max_body_size` (10 MiB) + `256 KiB` de margen multipart.
- **Credenciales seguras:** en `~/.pypirc` o variables de entorno `TWINE_USERNAME=__token__` y `TWINE_PASSWORD=${HODEI_TOKEN}` con URL `https://` en producción.

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
# 1. Crear repositorio PyPI hosted
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-pypi","format":"pypi","mode":"hosted"}'

# 2. Subir distribución wheel vía multipart
SHA256=$(sha256sum demo_pkg-1.0.0-py3-none-any.whl | awk '{print $1}')
curl -sS -X POST "http://127.0.0.1:${PORT}/pypi/doc-pypi/" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -F ":action=file_upload" \
  -F "protocol_version=1" \
  -F "name=demo-pkg" \
  -F "version=1.0.0" \
  -F "metadata_version=2.1" \
  -F "sha256_digest=${SHA256}" \
  -F "content=@demo_pkg-1.0.0-py3-none-any.whl"

# 3. Consultar índice Simple (PEP 503)
curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/pypi/doc-pypi/simple/demo-pkg/"
```

---

### OCI (`oci`)

- **Estado:** `NOT_RUN` (acreditado en workspace por `oci_push_contract`, `oci_pull_contract`, `oci_index_contract`, `hsp228_oci_proxy`, `hsp229_oci_group`)
- **Versión de cliente:** `docker` `26+` / `podman` `5+` / `oras` `1.2+`
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream` (`RepositoryFormat::Oci` sirve `hosted`, `proxy` y `group`; un proxy se crea con `POST /api/v1/catalog/repositories` y `settings.proxy.upstream`, y su salida respeta la lista de hosts permitidos del despliegue)
- **Rutas reales:**
  - `hosted`: `/v2/{repo}/blobs/uploads/`, `/v2/{repo}/blobs/{digest}`, `/v2/{repo}/manifests/{reference}`
  - `group`: `GET /v2/{repo}/manifests/{reference}` y `GET /v2/{repo}/blobs/{digest}`
  - Token endpoint: `/v2/token` (configurable vía `public_base_url`)
- **Credenciales seguras:** `docker login artifacts.example.com` (credenciales almacenadas en credential helper del sistema; en producción siempre HTTPS, nunca `insecure-registries`).

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
# Crear repositorio OCI hosted y comprobar /v2/
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-oci","format":"oci","mode":"hosted"}'

curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/v2/"
```

---

### Cargo (`cargo`)

- **Estado:** `NOT_RUN` (acreditado en workspace por `hsp119_cargo_e2e` y `hsp119_cargo_cli`)
- **Versión de cliente:** `cargo` `1.80+` (sparse registry protocol)
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream` (`proxy` y `group` rechazados hoy en `serves_modes()` hasta HSP-156)
- **Rutas reales:**
  - `hosted`: `GET /cargo/{repo}/config.json`, `GET /cargo/{repo}/{prefix}/{crate}`, `GET /cargo/{repo}/api/v1/crates/{crate}/{version}/download`, `PUT /cargo/{repo}/api/v1/crates/new`
  - Límite de carga: `max_body_size` + `256 KiB`
- **Credenciales seguras:** en `$CARGO_HOME/credentials.toml` (permisos `600`) o variable `CARGO_REGISTRIES_HODEI_TOKEN`.

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-cargo","format":"cargo","mode":"hosted"}'

curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/cargo/doc-cargo/config.json"
```

---

### NuGet (`nuget`)

- **Estado:** `NOT_RUN` (acreditado en workspace por `hsp120_nuget_e2e` y `hsp120_dotnet_client`)
- **Versión de cliente:** `.NET SDK` `8.0+` / `dotnet nuget`
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream` (`group` rechazado en dialecto NuGet)
- **Rutas reales:**
  - `hosted`: `GET /nuget/{repo}/v3/index.json`, `PUT /nuget/{repo}/package/{id}/{version}`, `GET /nuget/{repo}/flatcontainer/{id}/{version}/{id}.{version}.nupkg`, `GET /nuget/{repo}/registrations/{id}/index.json`
  - `proxy`: `GET /nuget/{repo}/...`
  - Límite de carga: `max_body_size` + `256 KiB`
- **Credenciales seguras:** `dotnet nuget add source` con clave API en variable de entorno (`--api-key "${HODEI_TOKEN}"` o `nuget.config` sin secretos en texto plano en Git).

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-nuget","format":"nuget","mode":"hosted"}'

curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/nuget/doc-nuget/v3/index.json"
```

---

### Go (`go`)

- **Estado:** `NOT_RUN` (acreditado en workspace por `hsp121_go_e2e`, `hsp121_go_client`, `hsp232_go_group`)
- **Versión de cliente:** `go` `1.22+` (`GOPROXY`)
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream`
- **Rutas reales:**
  - `hosted`: `PUT /go/{repo}/{module}/@v/{version}.{info|mod|zip}`, `GET /go/{repo}/{module}/@v/list`, `GET /go/{repo}/{module}/@latest`
  - `proxy`: `GET /go/{repo}/{module}/@v/...`
  - `group`: `GET /go/{repo}/{module}/@v/...` (resuelve `.info`, `.mod` y `.zip` de una versión desde un único miembro para preservar `go.sum`)
  - Límite de carga: `max_body_size` + `256 KiB`
- **Credenciales seguras:** en `~/.netrc` (permisos `600`) con token Bearer/basic para `artifacts.example.com` sobre HTTPS.

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-go","format":"go","mode":"hosted"}'

GOPROXY="http://127.0.0.1:${PORT}/go/doc-go" GOSUMDB=off \
  curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/go/doc-go/example.com/hello/@v/list"
```

---

### APT (`apt`)

- **Estado:** `NOT_RUN` (acreditado en workspace por `hsp122_apt_e2e` y `hsp122_apt_client`)
- **Versión de cliente:** `apt` `2.7+` (Debian/Ubuntu)
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream` (`RepositoryFormat::Apt` solo sirve `hosted` en esta versión)
- **Rutas reales:**
  - `hosted`: `PUT /apt/{repo}/pool/.../*.deb`, `GET /apt/{repo}/dists/{suite}/binary-{arch}/Packages`, `GET /apt/{repo}/dists/{suite}/Release`, `GET /apt/{repo}/dists/{suite}/Release.gpg`, `GET /apt/{repo}/key.asc`
  - Límite de carga: `max_body_size` + `256 KiB`
- **Credenciales seguras y firma:** instala la clave pública de `/apt/{repo}/key.asc` en `/etc/apt/keyrings/` (`signed-by=...`); **nunca** uses `[trusted=yes]` en producción. Usa `/etc/apt/auth.conf.d/` (permisos `600`) para credenciales.

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-apt","format":"apt","mode":"hosted"}'

curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/apt/doc-apt/key.asc" -o hodei-apt.asc
```

---

### RPM / Yum (`rpm`)

- **Estado:** `NOT_RUN` (acreditado en workspace por `hsp122_yum_e2e` y `hsp122_yum_client`)
- **Versión de cliente:** `dnf` `4.14+` / `rpm` `4.18+`
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream` (`RepositoryFormat::Rpm` solo sirve `hosted` en esta versión)
- **Rutas reales:**
  - `hosted`: `PUT /yum/{repo}/pool/{pkg}.rpm`, `GET /yum/{repo}/repodata/repomd.xml`, `GET /yum/{repo}/repodata/repomd.xml.asc`, `GET /yum/{repo}/key.asc`
  - Límite de carga: `max_body_size` + `256 KiB`
- **Credenciales seguras y firma:** configura `gpgcheck=1` y `repo_gpgcheck=1` con `gpgkey=https://artifacts.example.com/yum/doc-rpm/key.asc`; **nunca** uses `gpgcheck=0` ni `sslverify=0` en producción.

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-rpm","format":"rpm","mode":"hosted"}'

curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/yum/doc-rpm/key.asc" -o hodei-rpm.asc
```

---

### Helm-OCI (`oci` + `helm`)

- **Estado:** `NOT_RUN` (acreditado en workspace por `hsp121_helm_oci`)
- **Versión de cliente:** `helm` `3.14+` (soporte OCI nativo)
- **Política de upstream:** `proxy.allowed_hosts` + `settings.upstream` (nota: `helm` no tiene superficie HTTP propia en `serves_modes()`; los charts se publican y descargan sobre la superficie `oci` en `/v2/{repo}/...`)
- **Rutas reales:**
  - `hosted`: `/v2/{repo}/manifests/{tag}` y `/v2/{repo}/blobs/{digest}` (mismo repositorio `format: "oci", mode: "hosted"`)
  - `group`: `/v2/{repo}/...`
- **Credenciales seguras:** `helm registry login artifacts.example.com` (sobre HTTPS en producción; verifica provenance/digests y no uses `--insecure` en producción).

**Ejemplo offline (laboratorio local `127.0.0.1`):**

```bash
# Los charts Helm usan un repositorio de formato "oci"
curl -sS -X POST "http://127.0.0.1:${PORT}/api/v1/catalog/repositories" \
  -H "Authorization: Bearer ${HODEI_TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"name":"doc-helm-oci","format":"oci","mode":"hosted"}'

# Verificar disponibilidad de la superficie /v2/
curl -sS -H "Authorization: Bearer ${HODEI_TOKEN}" \
  "http://127.0.0.1:${PORT}/v2/"
```

---

## 3. Operaciones: copia de seguridad, restauración y actualización

- **Copia de seguridad, verificación, restauración y reparación (`hodei-recovery`)**: consulta la sección **RECOVERY** en [`uat/UAT-OPERATIONS.md`](../../uat/UAT-OPERATIONS.md#recovery--copia-restauración-scrub-y-reparación) (`hodei-recovery backup --to DIR`, `verify DIR`, `restore --from DIR`, `scrub`, `repair --from DIR`).
- **Actualización (upgrade) de versión**:
  1. Detén el servicio (`hodei-artifacts-api`) — la base RocksDB no admite copia en caliente.
  2. Ejecuta `hodei-recovery backup --to <dir>` y `hodei-recovery verify <dir>`.
  3. Despliega el nuevo paquete `tar.gz` generado por [`scripts/package-release.sh`](../../scripts/package-release.sh) verificando su `.sha256`.
  4. Arranca el servicio y valida `/health/ready` y `GET /api/v1/capabilities`.
