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

### 1.3 Qué prueba la firma y qué no

La clave pública viaja **dentro** del release. La firma prueba que los
ficheros no cambiaron desde que se firmaron con esa clave, no quién es el
dueño de la clave. Para confiar en la identidad, compara la huella que figura
en `signing.fingerprint` del manifiesto (y en el canal de publicación) con
la que te comunicó el mantenedor por una vía independiente. Cuando el release
se firmó con la clave efímera (`signing.key_mode = "ephemeral-local"`), la
firma solo da integridad del conjunto, y el manifiesto lo declara en
`signing.honest_limits`.

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

- `compose.yaml` (standalone): `podman compose up -d` levanta
  `hodei-artifacts-api` como usuario `10001:10001`, con el rootfs de solo
  lectura, un volumen con nombre (`hodei-artifacts-data`) para RocksDB y el CAS,
  y un healthcheck sobre `/health/ready`. RocksDB y el CAS de ficheros son de
  escritor único: no escales por encima de una réplica.
- `compose.external.yaml` (Dgraph + S3 compatible con Garage): declara el
  perfil externo sin rutas de RocksDB ni credenciales; `GARAGE_RPC_SECRET`,
  `GARAGE_ADMIN_TOKEN`, `AWS_ACCESS_KEY_ID` y `AWS_SECRET_ACCESS_KEY` son
  obligatorias y no tienen valor por defecto. Garage usa volúmenes separados
  para `meta` y `data`.

Límite conocido: hoy la API solo admite el backend de metadatos `rocksdb`;
con `RUN_MODE=external` rechaza `dgraph` y termina con código distinto de cero
(fallo cerrado). El perfil externo está declarado y validado, pero su camino
feliz no se ha ejecutado.

`scripts/compose-smoke.sh` comprueba con un runtime real el arranque no root con
rootfs de solo lectura, el healthcheck, la persistencia tras `down`/`up` y el
fallo cerrado ante un S3 inalcanzable y ante el perfil externo. Termina con 77
(`NOT_RUN`) si no hay Podman, proveedor de Compose o imagen.

## 7. Documentos relacionados

- [`configuration.md`](configuration.md): claves, variables y perfiles.
- [`release-notes-template.md`](release-notes-template.md): plantilla de notas de versión.
- [`UAT-OPERATIONS.md`](../../uat/UAT-OPERATIONS.md): contrato, pruebas y límites de la recolección y la recuperación.
