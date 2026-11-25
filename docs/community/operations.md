# Operación: verificación, copias, claves y actualización

Guía práctica (Diátaxis: *how-to*) para quien opera Hodei Artifacts. Cubre
**solo lo que existe como superficie ejecutable** en este repositorio: un
script, un binario o una ruta HTTP. Lo que no existe aparece en secciones
marcadas **Estado: no disponible todavía**, con lo que falta, y sin comandos
inventados.

> **Cómo se verificó.** Cada comando de esta página se ejecutó con
> `--help`, con un directorio vacío o con datos temporales, o se cita
> literalmente del script o del código que lo implementa. Al final de cada
> sección se indica qué se ejecutó y qué no.
> Para la configuración, ver [`configuration.md`](configuration.md).

## Resumen

| Procedimiento | Estado | Superficie |
| --- | --- | --- |
| Verificar un release | Disponible | [`scripts/verify-release.sh`](../../scripts/verify-release.sh) |
| Firma de release con clave propia | Disponible | [`scripts/package-oci.sh`](../../scripts/package-oci.sh) (`RELEASE_GPG_HOME`, `RELEASE_GPG_KEY_ID`) |
| Clave OpenPGP de APT y RPM | Parcial | `HODEI_METADATA_SIGNER_ARMOR`; no hay orden que la genere ni la rote |
| Copia, verificación, restauración y reparación | Disponible | `hodei-recovery` |
| Actualización | Parcial | Secuencia manual de órdenes existentes |
| Rollback, copia en caliente, rotación con solape | No disponible todavía | Ver secciones 3.3, 4.3 y 5.2 |

## 1. Verificar un release

Un release verificable contiene el `tar.gz`, el archivo OCI, SBOM (SPDX y
CycloneDX), la matriz de licencias, la atestación de procedencia, un
`*.manifest.json`, `SHA256SUMS` y las firmas OpenPGP separadas
(`SHA256SUMS.asc`, `*.manifest.json.asc`) con la clave pública
`release-signing-key.asc`.

### 1.1 Comprobación rápida del paquete

Para quien solo descargó el `tar.gz` y su `.sha256`
(el mismo paso del [`quickstart.md`](quickstart.md)):

```bash
sha256sum -c hodei-artifacts-api-<version>-<target>.tar.gz.sha256
```

### 1.2 Verificación completa

Necesitas `gpg`, `gpgv`, `sha256sum` y `python3`.

```bash
scripts/verify-release.sh <directorio-dist>
```

El script copia a un directorio temporal limpio solo lo que lista
`SHA256SUMS`, y comprueba, en este orden:

1. Las firmas de `SHA256SUMS` y del manifiesto con `gpgv`, contra la clave
   `release-signing-key.asc` del propio directorio.
2. Los hashes de todos los ficheros con `sha256sum -c SHA256SUMS`.
3. El manifiesto: campos obligatorios, hashes de SBOM, matriz de licencias y
   procedencia, estructura SPDX/CycloneDX y que los sujetos de la
   atestación (tarball y archivo OCI) coincidan con el manifiesto.

Si todo es correcto, termina con código 0 y una línea `OK: verified ...` con
nombre, versión, arquitectura, commit y digest OCI.

**Qué se ejecutó (verificado):** sin argumentos y con un directorio
inexistente imprime `usage: scripts/verify-release.sh <dist-directory>` y sale
con 1; con un directorio vacío sale con 1 y `error: SHA256SUMS missing in ...`.
La ruta de éxito con un release completo la ejercita
[`tests/hsp244_verifiable_release.rs`](../../tests/hsp244_verifiable_release.rs),
que invoca este script; **no se repitió a mano** en esta documentación porque
construir un release exige `podman` y `syft`.

Para anclar la identidad del firmante, pasa la huella completa que te
comunicó el mantenedor por una vía independiente:

```bash
scripts/verify-release.sh --require-bundle \
  --expect-fingerprint <huella-completa-de-40-o-64-hex> <directorio-dist>
```

`--expect-fingerprint` es el nombre actual; `--trusted-fingerprint` (HSP-267)
es el **mismo mecanismo** y se conserva como alias. La huella debe publicarse
**fuera** del bundle (otro canal, notas de otra publicación, una etiqueta
firmada): la clave pública, `SHA256SUMS` y las firmas viajan juntas, así que
quien manipula el `dist` puede sustituir las tres a la vez y la comprobación
de firma por sí sola seguirá pasando.

Con esta opción, la clave empaquetada `release-signing-key.asc` debe tener
exactamente esa huella primaria (se ignoran mayúsculas y espacios); si no,
el script falla con `does not match --expect-fingerprint`. Sin la opción, un
release re-firmado de forma coherente con una clave ajena sigue pasando, porque
solo se comprueba integridad. En modo bundle, además, `mechanism`, `key_mode` y
`fingerprint` de cada manifiesto por arquitectura deben coincidir con los de
`release-bundle.json`.

`scripts/test-release-bundle.sh` ejercita esa sustitución coherente (clave,
`SHA256SUMS` y firmas reemplazados a la vez): pasa sin la opción y falla con
ella; también cubre la manipulación de `manifest.json`, `SHA256SUMS`,
`release-bundle.json` y los SBOM, con y sin re-firma del falsificador.

**Árbol limpio.** La provenance registra `tree_clean` y el manifiesto lo
repite. `scripts/package-oci.sh` y `scripts/release-bundle.sh` fallan con un
árbol git sucio; `HODEI_ALLOW_DIRTY_TREE=1` lo permite sabiendo que se
registrará `tree_clean=false`, y `verify-release.sh` rechaza ese release salvo
con `--allow-dirty-tree` (que lo avisa). La arquitectura, `OCI_RUST_TARGET` y
la plataforma de la imagen deben coincidir, y el verificador comprueba que el
`rust_target` de la provenance construye la arquitectura del manifiesto.

**Licencias.** `scripts/release-license-policy.json` es la política: un
paquete con licencia `UNKNOWN` o fuera de la lista permitida (expresiones SPDX
con `OR`, `AND`, `WITH`) hace fallar la generación y la verificación, salvo
excepciones nombradas y justificadas (hoy `surrealdb` y `surrealdb-core`, con
licencia BSL 1.1 sin expresión SPDX). La matriz registra el estado real de
`surrealdb`: `present_in_cargo_toml`, `declared_by`, `rocksdb_in_resolved_graph`
y `reachable_from_release_binary`. Mientras el binario enlace surrealdb/rocksdb,
la afirmación «el SBOM OCI no contiene surrealdb ni rocksdb» queda como
`NOT_RUN` con su motivo (la retirada, HSP-213, no está integrada en esta
rama); cuando se integre, la comprobación pasa a verificar el SBOM de verdad.

### 1.3 Qué prueba la firma y qué no

La clave pública viaja **dentro** del release. La firma prueba que los
ficheros no cambiaron desde que se firmaron con esa clave, no quién es el
dueño de la clave. Para confiar en la identidad, compara la huella que figura
en `signing.fingerprint` del manifiesto (y en el canal de publicación) con
la que te comunicó el mantenedor por una vía independiente. Cuando el release
se firmó con la clave efímera (`signing.key_mode = "ephemeral-local"`), la
firma solo da integridad del conjunto, y el manifiesto lo declara en
`signing.honest_limits`.

### 1.4 Verificación desde una máquina limpia y segunda arquitectura

`scripts/verify-oci-correlation.sh <directorio>` se ejecuta sobre un
directorio que solo contiene los activos servidos (fuera del árbol fuente) y
comprueba, con `sha256sum -c`, `tar` y `python3`:

- los checksums de `SHA256SUMS`;
- que el archivo OCI de cada arquitectura contiene exactamente un manifiesto de
  imagen cuyo digest es `oci.archive_manifest_digest`, y que su configuración
  (`architecture`/`os`) es la que declara el manifiesto, el nombre del tarball,
  `oci.platform`, el fichero `.digest` y la provenance;
- cuando hay varias arquitecturas, que el índice OCI
  (`*-oci-index.json`, lista de manifiestos) fija exactamente los digests de
  los archivos por arquitectura, con su `platform.architecture`.

`oci.digest` es el digest que Podman guarda localmente; puede diferir del que
ve un registro al subir el archivo, por eso el manifiesto registra además
`oci.archive_manifest_digest`.

`scripts/test-release-integration.sh` (etapa C) ejecuta esa verificación sobre
el bundle real en un directorio vacío, y comprueba que rechaza un archivo
alterado y un manifiesto con otra arquitectura.

La segunda arquitectura se construye con `RELEASE_BUNDLE_EXTRA_ARCHES=aarch64
scripts/release-bundle.sh` (cross-compilación con `cargo-zigbuild` e imagen
`linux/arm64`; ejecutarla en un host x86_64 exige un manejador binfmt de
`qemu-aarch64`). Sin esa variable, `aarch64` queda como `NOT_RUN` con su
motivo. `scripts/test-release-aarch64.sh` (opt-in con `HODEI_TEST_AARCH64=1`,
sale con 77 si falta algo) lo construye y verifica de extremo a extremo, y
arranca la imagen arm64 bajo emulación. Tarda decenas de minutos en frío.

## 2. Claves de firma del release

`scripts/package-oci.sh` firma `SHA256SUMS` y el manifiesto con GnuPG
(`gpg --armor --detach-sign`). Elige la clave así:

| Modo (`signing.key_mode`) | Cuándo | Qué hace |
| --- | --- | --- |
| `operator-keyring` | `RELEASE_GPG_HOME` **y** `RELEASE_GPG_KEY_ID` están definidas | Firma con la clave `RELEASE_GPG_KEY_ID` del llavero en `RELEASE_GPG_HOME`. |
| `ephemeral-local` | En cualquier otro caso (basta con que falte una de las dos) | Genera una clave Ed25519 nueva en un directorio temporal, sin passphrase, y la borra al terminar. |

```bash
RELEASE_GPG_HOME=/ruta/al/llavero RELEASE_GPG_KEY_ID=<huella> \
  scripts/package-oci.sh [directorio-de-salida]
```

Cuando `scripts/release-bundle.sh` orquesta el release, comparte su clave con
`scripts/package-oci.sh` y le indica el origen real mediante
`RELEASE_SIGNING_KEY_MODE` (`ephemeral-local` u `operator-keyring`), de modo
que el manifiesto por arquitectura y `release-bundle.json` declaran el mismo
modo.

El script exporta la clave pública activa a `release-signing-key.asc`, y la
huella y el hash de esa clave quedan en el manifiesto.

**Generar la clave del operador.** El script no tiene una orden para ello; usa
GnuPG directamente. Este bloque de parámetros es el mismo que usa el script
para la clave efímera (se ejecutó en un `GNUPGHOME` temporal y las firmas que
produce abren con `gpgv`, igual que en `verify-release.sh`):

```bash
mkdir -m 700 /ruta/al/llavero
cat > keyparams <<'EOF'
Key-Type: eddsa
Key-Curve: ed25519
Key-Usage: sign
Name-Real: Nombre de la clave de release
Name-Email: release@example.com
Expire-Date: 0
%no-protection
%commit
EOF
GNUPGHOME=/ruta/al/llavero gpg --batch --gen-key keyparams
GNUPGHOME=/ruta/al/llavero gpg --with-colons --list-secret-keys   # la huella está en la línea fpr
```

`%no-protection` crea una clave **sin passphrase**: protege el directorio del
llavero con permisos `700` y no lo copies a otras máquinas. El script firma con
`--batch`, y no se verificó una clave protegida con passphrase, así que no se
documenta ese caso.

### 2.1 Estado: rotación de la clave de release, no disponible como procedimiento

Cambiar de clave es cambiar `RELEASE_GPG_KEY_ID` (o el llavero) en el siguiente
release: el release nuevo trae su `release-signing-key.asc` y su huella en el
manifiesto. **No existe** un mecanismo que enlace la clave nueva con la
anterior, ni lista de revocación, ni solape: los consumidores deben tomar la
huella nueva por un canal independiente (sección 1.3). Falta, por tanto, un
procedimiento de publicación de la huella y de revocación que esté definido y
probado.

## 3. Claves OpenPGP de APT y RPM

El hub firma `Release.gpg` (APT) y `repomd.xml.asc` (RPM) con una única
clave OpenPGP de proceso, y sirve su parte pública en
`GET /apt/{repo}/key.asc` y `GET /yum/{repo}/key.asc`
(ver [`ecosystems.md`](ecosystems.md)).

### 3.1 Cómo se elige la clave

Lo decide `bootstrap_signer()` en [`src/openpgp.rs`](../../src/openpgp.rs):

- Con la variable `HODEI_METADATA_SIGNER_ARMOR` definida, el servicio carga esa
  clave **secreta** OpenPGP en texto blindado. Si el contenido no es una
  clave secreta blindada, el proceso aborta al arrancar con
  `HODEI_METADATA_SIGNER_ARMOR must hold an armored OpenPGP secret key`
  (*verificado*).
- Sin la variable, genera una clave **efímera** en cada arranque: cada reinicio
  firma con una clave distinta y los clientes tienen que volver a leer
  `key.asc`. Sirve para laboratorio, no para producción.

El código firma con contraseña vacía (`Password::from("")`): la clave tiene que
ser **sin passphrase**.

### 3.2 Obtener una clave persistente

No hay una orden dedicada a generar la clave del hub. Estas son las dos vías
que se comprobaron, ambas con el servicio arrancando con ella
(`/health` respondió `healthy`; **no se ejercitó** la firma de un
repositorio APT o RPM real con estas claves):

**A. Con GnuPG**, con una clave Ed25519 sin passphrase como la de la sección 2:

```bash
GNUPGHOME=/ruta/al/llavero gpg --armor --export-secret-keys <huella> > hub-signer.asc
```

**B. Con `hodei-recovery`.** La orden `bundle export --signing-key RUTA` crea
`RUTA` con una clave secreta blindada **la primera vez** que se usa y la reutiliza
después (`load_or_create_signer` en
[`src/bin/hodei-recovery.rs`](../../src/bin/hodei-recovery.rs)). Su propósito es
firmar la cápsula OCI de un paquete exportado, no el hub; usarla como fuente
de la clave del hub es un efecto lateral, y la orden además exige una cuenta y un
repositorio reales (con uno inexistente crea el fichero y termina con código 2).
Prefiere la vía A.

Después, pasa el contenido del fichero al servicio:

```bash
HODEI_METADATA_SIGNER_ARMOR="$(cat hub-signer.asc)" hodei-artifacts-api
```

Guarda `hub-signer.asc` como un secreto (permisos `600`, fuera del repositorio y
fuera de la copia de seguridad de datos: `hodei-recovery backup` copia el
catálogo y los bytes, no la clave del hub ni la configuración).

### 3.3 Rotar la clave

Lo que el código permite, y que se desprende de la sección 3.1:

1. Genera una clave nueva (sección 3.2).
2. Define `HODEI_METADATA_SIGNER_ARMOR` con la clave nueva y reinicia el
   servicio.
3. Cada cliente debe volver a descargar `key.asc` de cada repositorio y
   reinstalarla (`signed-by` en APT; `gpgkey` en dnf; ver
   [`ecosystems.md`](ecosystems.md)). Hasta entonces, la verificación de
   `Release.gpg` y de `repomd.xml.asc` **fallará** en esos clientes.

Esta secuencia es consecuencia de leer el código; **no se ejecutó de principio a fin**
con un cliente real.

**Estado: no disponible todavía.** Falta: una orden que genere y rote la clave
del hub; un periodo de solape en el que el hub publique la clave vieja y la nueva
(el hub solo conoce **una** clave); firma doble de los metadatos; y un
procedimiento de revocación.

## 4. Copia de seguridad, restauración y reparación

Los realiza el binario `hodei-recovery` (los valores por defecto de `--db`,
`--cas`, `--namespace` y `--database` salen de la configuración del servicio,
ver [`configuration.md`](configuration.md)). El contrato completo, las pruebas y
los límites están en la sección RECOVERY de
[`UAT-OPERATIONS.md`](../../uat/UAT-OPERATIONS.md#recovery--copia-restauración-scrub-y-reparación).

```bash
hodei-recovery --help
hodei-recovery [--db RUTA] [--cas RUTA] [--json] backup --to DIR
hodei-recovery verify DIR
hodei-recovery [--db RUTA] [--cas RUTA] restore --from DIR
hodei-recovery [--db RUTA] [--cas RUTA] scrub
hodei-recovery [--db RUTA] [--cas RUTA] repair --from DIR
```

Códigos de salida: `0` sano, `1` daño encontrado o blobs excluidos, `2` error.

### 4.1 Copia

> **Solo motor embebido (`legacy`).** `hodei-recovery` y su base RocksDB
> pertenecen al motor embebido, que ya no está en el binario productivo. Se
> distribuye en el archivo de herramientas de legado
> (`scripts/package-legacy-tools.sh`) hasta HSP-314 F4. En el perfil de grafo la
> copia es la de Dgraph (`deploy/dgraph/README.md`) y la del bucket S3.

1. **Detén el servicio.** RocksDB bloquea la base: no hay copia en caliente.
2. El directorio del almacén de contenido tiene que existir.
3. `hodei-recovery --db <rocksdb> --cas <cas> backup --to <dir-nuevo>`. La copia
   se escribe en un directorio temporal y se publica con un renombrado
   atómico.
4. `hodei-recovery verify <dir>` comprueba el manifiesto, los metadatos y cada
   blob, sin escribir.

### 4.2 Restauración

Solo sobre una instalación **limpia** (base y almacén vacíos):

```bash
hodei-recovery --db <rocksdb-nueva> --cas <cas-nuevo> restore --from <dir>
```

Hace al final un `scrub` e informa. Sobre una base con datos se niega
(`the destination is not clean: the database already has tables`) y no
toca nada.

`scrub` informa de blobs corruptos, referencias rotas y huérfanos, y no
repara; `repair --from DIR` restaura desde una copia verificada los blobs
corruptos o ausentes y deja el corrupto en `quarantine/`.

**Qué se ejecutó (verificado, con un registro vacío):** `--help`, `backup`
(`backup written to ...: 0 blobs, 0 excluded`), `verify` (`the backup is
sound`), `restore` en directorios nuevos (`restored 0 blobs ...`), una segunda
restauración sobre la misma base (rechazada, código 2), `repair` y
`scrub --json`. Con el almacén de contenido inexistente `backup` falla con
`the content store ... does not exist`. La restauración con datos reales y los
tiempos de RPO y RTO están medidos en el UAT enlazado, no repetidos aquí.

### 4.3 Estado: no disponible todavía

- Copia **en caliente** o incremental, y copia programada.
- Restauración **parcial** o por cuenta o repositorio, y fusión con datos
  existentes.
- Rotación, retención y cifrado de las copias: los decide el operador.
- Copia de la configuración, de los secretos y de la clave del hub: `backup`
  no los incluye.
- Una API HTTP de copia o restauración. Por HTTP solo existe el `scrub`
  (`GET /api/v1/scrub`, ruta administrativa).

## 5. Actualización y rollback

### 5.1 Actualización (secuencia manual)

La secuencia ya figura en la sección 3 de [`ecosystems.md`](ecosystems.md);
cada paso es una orden documentada arriba:

1. Detén el servicio `hodei-artifacts-api`.
2. `hodei-recovery backup --to <dir>` y `hodei-recovery verify <dir>`
   (sección 4).
3. Obtén el paquete nuevo y compruébalo: `sha256sum -c <paquete>.sha256` y, si
   hay release completo, `scripts/verify-release.sh <dist>` (sección 1). Un
   paquete se produce con [`scripts/package-release.sh`](../../scripts/package-release.sh).
4. Sustituye el binario y la configuración por los del paquete nuevo, y
   arranca el servicio.
5. Comprueba `GET /health` (se ejecutó: responde `{"status":"healthy",...}`)
   y `GET /api/v1/capabilities` (ver [`ecosystems.md`](ecosystems.md)).

Las órdenes de cada paso se ejecutaron por separado; la secuencia entera, de un
despliegue a la versión siguiente, **no se ejecutó** de principio a fin.

### 5.2 Estado: rollback, no disponible todavía

No existe un procedimiento de rollback ni una orden que lo haga. Lo que sí
afirma el contrato de recuperación es que una copia solo se restaura sobre una
instalación limpia y que se rechaza una copia de un `format` de manifiesto mayor
que el que conoce el binario (hoy `1`); no hay migración de esquema dentro de
la restauración. Un rollback real necesitaría, y hoy no existe: una política de
compatibilidad de datos entre versiones, la prueba de restaurar una copia de
la versión `N` con el binario `N-1`, y la orden que lo orqueste. Hasta que
existan, trata la copia de la sección 4 tomada **antes** de actualizar como la
única red de seguridad, y no asumas que el binario anterior abre los datos del
nuevo.

## 6. Despliegue declarativo con Compose

El repositorio incluye dos ficheros Compose y un `compose.env.example` sin secretos (un `.env.example` lo veta la politica de ficheros de la maquina).
Copia `compose.env.example` a `.env` y ajústalo antes de levantar nada.

- `compose.yaml` (solo la API, perfil de grafo): `podman compose up -d`
  levanta `hodei-artifacts-api` con `RUN_MODE=dgraph` como usuario
  `10001:10001`, con el rootfs de solo lectura y un healthcheck sobre
  `/health/ready`. El Alpha de Dgraph (`HODEI_GRAPH__ENDPOINT`,
  `HODEI_GRAPH__CREDENTIALS__PASSWORD`) y el bucket S3 son los tuyos y las
  variables que los nombran son obligatorias y no tienen valor por defecto. El
  servicio falla cerrado (no está *ready*) mientras el Alpha o el almacén de
  objetos no responden; no hay *fallback* a RocksDB ni al sistema de ficheros.
- `compose.external.yaml` (pila completa): Dgraph Zero+Alpha con ACL (el secreto
  HMAC se monta desde `HODEI_DGRAPH_HMAC_SECRET_FILE`), Garage como S3 de un
  solo nodo y la API con `RUN_MODE=dgraph`. `GARAGE_RPC_SECRET`,
  `GARAGE_ADMIN_TOKEN`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` y
  `HODEI_GRAPH__CREDENTIALS__PASSWORD` son obligatorias y no tienen valor por
  defecto. El layout, el bucket y la clave de Garage los creas tú
  (`garage layout`, `garage bucket create`, `garage key create`); la cuenta
  `groot` del Alpha nace con la contraseña por defecto de Dgraph, cámbiala.

Los ficheros ya no declaran el perfil embebido (RocksDB + CAS en volumen): la
imagen se construye sin la feature `legacy` y lo rechaza. La investigación, los
hechos de riesgo y el reactor de CVE siguen cerrados en el perfil de grafo
hasta HSP-314 F3.

`scripts/compose-smoke.sh` valida los dos ficheros, comprueba que no llevan
credenciales literales y, con un runtime real, que el perfil de grafo falla
cerrado ante un Alpha y un S3 inalcanzables (no *ready*, sin volumen
`/app/data` y sin ser rechazado como backend no soportado). El camino feliz de
la pila completa no está cubierto por el humo (necesita el arranque de Garage y
de la ACL); lo cubren `hsp237` y `hsp308` con Dgraph y S3 reales. Termina con 77
(`NOT_RUN`) si no hay Podman, proveedor de Compose o imagen.

### 6.1 Readiness y liveness con el perfil externo (HSP-237)

`scripts/hsp237-profile-e2e.sh` recorre el perfil con S3 externo y un Dgraph de
laboratorio levantados con Podman (el Dgraph de `deploy/dgraph/compose.yaml`,
fijado por digest, en red interna; el S3 es LocalStack, como en HSP-138) contra
el binario real, y mide por HTTP lo siguiente:

- Con Dgraph y S3 disponibles, `/health/ready` y `/health/live` responden 200.
- Al parar el S3, `/health/ready` deja de ser 200 (503) y `/health/live` sigue
  en 200; al devolverlo (y recrear el bucket, porque LocalStack es volátil), la
  readiness vuelve a 200.
- Los cuerpos no incluyen URLs, el nombre del bucket ni la credencial, y el log
  del servicio no repite el secreto.
- Presupuesto de red: el probe hace **una** llamada `HeadBucket` por
  veredicto (caché de 1 s, tope de 1 s por dependencia y 2 s por probe) y
  nunca lista objetos ni recorre el catálogo; 20 sondeos seguidos cuestan como
  máximo una comprobación.

### 6.1 Readiness y liveness con el perfil de grafo (HSP-237, HSP-292)

`scripts/hsp237-profile-e2e.sh` recorre el perfil de grafo (`RUN_MODE=dgraph`,
sección `[graph]`, contenido en S3) con un Dgraph de laboratorio y un S3
levantados con Podman (el Dgraph de `deploy/dgraph/compose.yaml`, fijado por
digest, en red interna; el S3 es LocalStack, como en HSP-138) contra el binario
real, y mide por HTTP lo siguiente:

- Con Dgraph y S3 disponibles, `/health/ready` y `/health/live` responden 200,
  y el cuerpo de la readiness nombra `graph` y `content_store`.
- Al parar el Alpha de Dgraph, `/health/ready` pasa a 503 (la dependencia
  `graph` falla) en menos de 2,5 s y `/health/live` sigue en 200; al
  devolverlo, la readiness vuelve a 200.
- Al parar el S3, `/health/ready` deja de ser 200 (503) y `/health/live` sigue
  en 200; al devolverlo (y recrear el bucket, porque LocalStack es volátil), la
  readiness vuelve a 200.
- Los cuerpos no incluyen URLs, el nombre del bucket ni credenciales, y el log
  del servicio no repite el secreto.
- Presupuesto de red: el probe hace **una** llamada `HeadBucket` por
  veredicto (caché de 1 s, tope de 1 s por dependencia y 2 s por probe) y
  nunca lista objetos ni recorre el catálogo; 20 sondeos seguidos cuestan como
  máximo una comprobación.

El detalle del contrato (rutas, presupuestos, cuerpo) está en
[`../api/readiness-endpoint.md`](../api/readiness-endpoint.md).
Cuando el catálogo Dgraph se cablee, ese paso debe pasar a esperar 503.

El test necesita `HSP237_PROFILE_E2E=1` (lo fija el script); sin ella se salta.
El script termina con 77 (`NOT_RUN`) si falta Podman o las imágenes. La evidencia
queda en `target/uat/hsp237-profile-e2e/`.

### 6.2 La clave de firma del mirror de Terraform (HSP-313)

El mirror de providers entrega al cliente Terraform URLs de zip firmadas y de
vida corta, porque Terraform no envía credenciales al descargar el archivo. La
clave que las firma es un secreto del operador (sección `[signing]` de
[`configuration.md`](configuration.md)); sin ella el mirror no emite URLs de zip
y el documento de versión responde `503`.

- **Compose**: `compose.yaml` y `compose.external.yaml` exigen
  `HODEI_SIGNING_KEY_FILE`, la ruta de un fichero con el secreto, sin valor por
  defecto (un valor por defecto sería una clave compartida). Se monta de solo
  lectura en `/run/secrets/hodei-signing-key`. Genéralo con
  `openssl rand -hex 32 > signing-key` y haz que lo pueda leer el UID 10001 de
  la imagen. `scripts/compose-smoke.sh` genera su propia clave efímera en un
  directorio temporal y la borra al terminar.
- **Helm**: `signing.existingSecret` nombra un Secret existente (el chart no
  crea ninguno) y `signing.keys` lista, por orden, `{ id, secretKey }`; la
  primera firma. Crea el Secret con
  `kubectl create secret generic hodei-signing --from-literal=signing-key="$(openssl rand -hex 32)"`.
  Sin `signing.existingSecret` no se configura ninguna clave y el mirror falla
  cerrado.
- **Réplicas**: todas reciben el mismo anillo. Una URL emitida por una réplica
  la verifica cualquier otra; con anillos distintos, la mitad de las
  descargas fallarían con 403.
- **Rotación** (tres pasos, sin cortar las URLs en vuelo) y retirada de una
  clave filtrada: ver la sección 5.9 de [`configuration.md`](configuration.md).
- **Qué mirar en el log**: al arrancar, `the signed URLs of the provider mirror
  are enabled` (con el TTL) o `no signing key is configured`. Nunca aparecen el
  secreto ni la firma: el log de peticiones solo nombra la ruta, y la query de
  la URL firmada no se registra. Un canje rechazado deja una línea
  `a signed archive URL was refused` con el repositorio y el motivo genérico
  (caducada, firma que no coincide, clave desconocida), y el cliente recibe
  siempre el mismo `403`.

**Nota de migración (actualizar a una versión con HSP-313).** Una instalación
que ya servía providers de Terraform y se actualiza **sin** `[signing]` deja de
poder instalarlos: el mirror falla cerrado y el documento de versión
(`<versión>.json`) pasa a responder `503` (el índice y la lectura autenticada
del zip siguen igual). Para no cortar el servicio, configura la clave **antes**
de actualizar:

1. Genera el secreto con `openssl rand -hex 32` y entrégalo como fichero o
   variable (Compose: `HODEI_SIGNING_KEY_FILE`; Helm: `signing.existingSecret`).
2. Declara `[signing]` con esa clave en todas las réplicas, con el mismo anillo.
3. Actualiza, y comprueba en el log `the signed URLs of the provider mirror are
   enabled` y no `no signing key is configured`.

Con Compose, el fichero se exige con `:?`: sin `HODEI_SIGNING_KEY_FILE`,
`compose up` falla en vez de arrancar sin clave.

## 7. Documentos relacionados

- [`configuration.md`](configuration.md): claves, variables y perfiles.
- [`release-notes-template.md`](release-notes-template.md): plantilla de notas de versión.
- [`UAT-OPERATIONS.md`](../../uat/UAT-OPERATIONS.md): contrato, pruebas y límites de la recolección y la recuperación.
