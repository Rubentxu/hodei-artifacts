# BASELINE — estado verificable remoto y deuda

**Repositorio:** https://github.com/Rubentxu/hodei-artifacts
**Revisión inspeccionada:** `2626e35b7c12375af0a1f23342eee4b4619bfc3c`
**Censo ejecutado:** 2026-10-24 sobre el árbol de trabajo de esa revisión.

Este documento contiene **datos medidos**, no estimaciones. Cada cifra procede de
ejecutar la herramienta real en el árbol real; cuando una comprobación no pudo
ejecutarse se marca `NOT_RUN` con su causa concreta, nunca se omite ni se
aproxima.

---

## 1. Inventario remoto (medido)

| Dato | Valor observado | Comando / fuente |
|---|---|---|
| `HEAD` local | `2626e35b7c12375af0a1f23342eee4b4619bfc3c` | `git rev-parse HEAD` |
| `origin/main` | `2626e35b7c12375af0a1f23342eee4b4619bfc3c` (idéntico a `HEAD`) | `git rev-parse origin/main` |
| Tags | **0** | `git tag` |
| Releases | **0** — no hay ninguna versión publicada que reutilizar | `git tag` (sin tags), `gh release list` (vacío) |
| Issues abiertas o cerradas | **0** | `gh issue list --state all` → `[]` |
| Ramas | `main`; `remotes/origin/002-docs-epics-epic` | `git branch -a` |
| Árbol de trabajo | 95 ficheros modificados sin commitear (H0 en curso) | `git status --porcelain` |
| Licencia | MIT (`LICENSE`, `license = "MIT"` en el `Cargo.toml` raíz) | lectura directa |

**Consecuencia para el versionado:** al no existir tags ni releases, la primera
etiqueta SemVer de este repositorio no colisiona con nada. La versión del
paquete raíz es `0.1.0`; la siguiente versión la fija el hito, no una suposición.

**Estado posterior a la ejecución de H0.** La tabla de arriba es el **censo**,
medido en la revisión `2626e35…`. Desde entonces H0 se ejecutó y se publicó
`v0.1.0`: el árbol de trabajo está limpio, existe el tag anotado `v0.1.0`
apuntando a `c3d79a6…` (idéntico a `origin/main`) y hay una release publicada
con 5 assets (sin `sources.zip`). La verificación de esa publicación —CI
remota verde, tag, assets y bytes publicados— está en §10.

**Posición de licencia de Nexus CE:** Nexus Repository OSS es EPL-1.0. Este
repositorio es MIT. Nexus CE se usa como **referencia de comportamiento**
(protocolo, semántica de repositorios) y **no** se distribuye ni se vincula
como dependencia; la reimplementación es MIT.

---

## 2. Toolchain (medido)

| Herramienta | Versión / estado |
|---|---|
| `rustc` | 1.99.0 (b940084d7 2026-09-28) |
| `cargo` | 1.99.0 (5f94df478 2026-08-27) |
| `rustfmt` | 1.10.0-stable (b940084d7e 2026-09-28) |
| `clippy` | 0.1.99 (b940084d7e 2026-09-28) |
| `cargo-nextest` | **AUSENTE** → `NOT_RUN` (la suite se ejecutó con `cargo test`) |
| `docker` | **AUSENTE** → `NOT_RUN` para toda comprobación que exija el daemon Docker |
| `podman` | presente (disponible como sustituto donde baste un runtime OCI) |
| `typedb` / `typedb-server` | **AUSENTES** → `NOT_RUN` (ningún hito TypeDB puede verificarse localmente) |
| `java`, `mvn` | presentes |

**Causas concretas de `NOT_RUN`** (ninguna es un fallo del producto):

1. `cargo-nextest` no está instalado en el host; la autoridad CI sí lo invoca
   (`cargo nextest run --workspace --no-fail-fast`). Localmente se usó
   `cargo test --workspace --no-fail-fast`, que ejecuta las mismas dianas.
2. `docker` no está instalado; `podman` sí, pero ningún endpoint del producto
   depende hoy de un contenedor, así que la comprobación es `NOT_RUN` por
   ausencia de daemon, no por ausencia de cobertura.
3. `typedb` y `typedb-server` no están instalados: **H2 y todo lo que dependa
   de TypeDB no puede verificarse en este host**. Queda registrado como
   bloqueo de entorno, no como trabajo de código.

---

## 3. Puerta de calidad — resultado medido

Comandos ejecutados sobre el árbol de trabajo, con su código de salida real:

```
cargo metadata --no-deps --format-version 1              → exit 0
cargo check --workspace --all-targets                     → exit 0 (0 warnings)
cargo fmt --all -- --check                                → exit 0
cargo clippy --workspace --all-targets -- -D warnings     → exit 0 ("No issues found")
cargo test --workspace --no-fail-fast                     → exit 0
```

`cargo metadata` confirma que el workspace tiene **exactamente 4 paquetes**:
`kernel`, `hodei-iam`, `hodei-policies`, `hodei-artifacts-api`. Los cuatro
declaran `edition = "2024"` (heredado de `[workspace.package]`).

**Total: 746 passed / 0 failed / 64 ignored (doctests).**

| Diana de test | passed | failed | ignored |
|---|---|---|---|
| `hodei-artifacts-api` lib | 33 | 0 | 0 |
| `hodei-artifacts-api` bin | 2 | 0 | 0 |
| `tests/binary_contract.rs` | 2 | 0 | 0 |
| `tests/bootstrap_integration.rs` | 7 | 0 | 0 |
| `tests/http_contract.rs` | 8 | 0 | 0 |
| `tests/legacy_quarantine.rs` | 5 | 0 | 0 |
| `tests/schema_persistence.rs` | 3 | 0 | 0 |
| `tests/distribution_contract.rs` | 2 | 0 | 0 |
| `hodei-iam` lib | 212 | 0 | 0 |
| `hodei-iam` `integration_create_group_comprehensive_test` | 12 | 0 | 0 |
| `hodei-iam` `integration_create_policy_new_test` | 13 | 0 | 0 |
| `hodei-iam` `integration_create_user_comprehensive_test` | 10 | 0 | 0 |
| `hodei-policies` lib | 198 | 0 | 0 |
| `kernel` lib | 227 | 0 | 0 |
| `hodei-iam` `surrealdb_integration_test` | 6 | 0 | 0 |
| doctests (`hodei-artifacts-api`, `hodei-iam`, `hodei-policies`, `kernel`) | 6 | 0 | 64 |

Los 64 `ignored` son **doctests**, no casos de prueba: son ejemplos de
documentación no ejecutados. Ningún test de comportamiento está marcado como
ignorado, y no hay ningún `SKIP` en la suite.

---

## 4. Datos persistidos existentes (medido)

**Base presente en el árbol:** `data/hodei.development.rocksdb` (48 KB, motor
RocksDB embebido de SurrealDB). Está fuera de Git (`.gitignore` cubre `data/`).

**Procedimiento aplicado (sin destruir nada):**

1. Copia íntegra del directorio a `/tmp/hodei-h0-data-backup/` (fuera del
   repositorio), con digest agregado del conjunto de ficheros:
   `b660c6e4036c83ccc2883cb54a9fc58577c481eda24f564ccc5d03876505655f`.
2. Apertura de la **copia** con el motor embebido y volcado con
   `INFO FOR ROOT` y `INFO FOR DB`.
3. Sonda temporal (`examples/h0_probe_*.rs`) **eliminada** después; no queda
   ningún residuo en el árbol ni en el índice de Git.

**Resultado del volcado:** la base **no contiene datos de usuario**.

```
ROOT: namespaces: {}   users: {}   accesses: {}
hodei::artifacts → tables: []   (probadas 16 combinaciones ns/db)
```

Los 48 KB son exclusivamente estructuras internas de RocksDB (MANIFEST, SST,
LOG, OPTIONS, LOCK), no registros de dominio. `INFO FOR ROOT` no declara
ningún namespace, por lo que no existe esquema, usuario, política ni blob que
exportar.

**Manifest de cambios y rollback:** H0 **no** cambia el esquema ni el formato de
la base existente — solo arregla la persistencia del schema Cedar (D3) y la
idempotencia de la escritura (D2), ambos dentro del mismo motor. El rollback
consiste en borrar la base de desarrollo (`data/hodei.development.rocksdb`), que
está vacía y regenerable; la copia de `/tmp/hodei-h0-data-backup/` permite
restaurarla tal cual si hiciera falta. **No se destruyó ningún dato ni
credencial**: la base original sigue intacta con sus 15 ficheros.

**Conclusión:** la exportación de datos existentes **no es necesaria** en H0
(verificado, no asumido). No hay datos que perder en la migración a TypeDB de H2.

## 5. Evidencia de código real

| Evidencia | Estado medido | Decisión de migración |
|---|---|---|
| `Cargo.toml` `[workspace] members` | activos `kernel`, `hodei-iam`, `hodei-policies`, raíz `.`; 9 directorios comentados | H0: registrar censo y recuperar por slices |
| `src/main.rs`, `src/bootstrap.rs`, `src/composition_root.rs` | composición, HTTP, inicialización RocksDB de SurrealDB | reemplazar adaptador con TypeDB; conservar contratos HTTP útiles |
| `crates/kernel/src/domain/hrn.rs` | tipo HRN actual sin región, y UID Cedar omitía cuenta | H1: HRN v2 + migración + aislamiento |
| `crates/kernel/src/domain/entity.rs` (`ActionTrait::action_name`) | produce `Iam::Action::"CreateUser"` | divergencia latente: el evaluador produce `Action::"CreateUser"` (namespace vacío) — **ver §7, O5** |
| `crates/hodei-policies/src/internal/engine/core.rs` | `RwLock` de políticas y entidades, reemplazo secuencial | H1: snapshots coherentes inmutables por evaluación |
| `crates/hodei-policies/src/features/evaluate_policies/use_case.rs` | carga un schema, `TODO` de uso efectivo | H1: validación estricta schema/policies/context |
| `crates/hodei-iam/src/features/evaluate_iam_policies/use_case.rs` | separación por puerto hacia políticas | reutilizar seam, verificar no mezclar SCPs |
| `crates/hodei-policies/src/internal/engine/schema_codec.rs` | **nuevo en H0**: envelope `cedar-schema-fragments-v1` sobre `SchemaFragment` | KEEP: persistencia fiel del schema Cedar (ver §7, D3) |
| `src/config.rs` (`environment_source`) | **corregido en H0**: `prefix_separator("_")` explícito | KEEP: el override documentado `HODEI_<SECCIÓN>__<CLAVE>` es hoy efectivo (ver §7, D6) |
| `crates/hodei-authorizer/src/authorizer.rs` | composición de IAM y SCP en un mismo PolicySet | NO reactivar sin semántica de límites y deny-overrides |
| `crates/repository/src/domain/repository.rs` | `Hosted / Proxy / Virtual` y configs ADT | rescatar ontología, renombrar `Group` al exterior solo según compatibilidad |
| `crates/artifact/src/domain/sbom.rs` y `crates/supply-chain/src/domain/sbom.rs` | dos modelos SBOM distintos | H5: un solo modelo de evidencia y normalizadores |
| `crates/artifact/src/domain/unified.rs`, `crates/repository/src/domain/unified.rs` | referencias `shared` / modelos de migración | excluir de compilación hasta clasificación |
| `crates/distribution/src/domain/format_handler.rs` | `Option<Vec<u8>>` para bodies | H2: sustituir por streaming en adaptadores |
| `crates/distribution/src/features/handle_maven_request/use_case.rs` | permission checker consultado con principal literal `system` | H3: identidad real de requester, denegación negativa |
| `crates/repository/src/domain/unified.rs` | `Private` / `Internal` con implementación ejemplo que permite acceso | nunca recuperar esa autorización como producción |
| `Makefile` | **reconciliado en H0**: `LIB_CRATES` se deriva de `cargo metadata --no-deps`; ya no invoca crates no miembro | KEEP: la tarea ya no puede dar falso verde |
| `.github/workflows/rust-ci.yml` | **reconciliado en H0**: `cargo nextest run --workspace --no-fail-fast` en vez de `-p policies` legado | KEEP: la autoridad CI existente cubre el workspace real |
| `src/infrastructure/` | **módulo muerto**: no está declarado en `src/lib.rs`, así que nunca se compila; contiene `GetEffectiveScpsAdapter` | DELETE (candidato H1): código no compilado no es evidencia de nada |
| `data/hodei.development.rocksdb` | base SurrealDB/RocksDB presente (48 KB) y **verificada vacía** de datos de usuario (§4) | no hay datos que exportar; no destruir ni migrar en H0 |
| `tests/legacy/` | cuarentena declarada en `tests/legacy/README.md`, protegida por `tests/legacy_quarantine.rs` | QUARANTINE: ver §6.3 |
| `crates/*/tests/*.rs.disabled` | 5 ficheros; el sufijo impide que Cargo los compile | QUARANTINE: ver §6.3 |
| `docs/prd/8-roadmap.md` | backlog Q3 2024–Q1 2025 abierto/histórico | conservar e indexar hacia roadmap nuevo |

---

## 6. Clasificación obligatoria por crate

Criterio: `KEEP` = demostrablemente compilable y aporta garantía; `PORT` =
conceptos útiles con adaptación al nuevo puerto y ontología; `QUARANTINE` =
excluido u obsoleto, no hacerlo público ni simular completitud; `REPLACE` =
acoplamiento inseguro o impropio para streaming o Graph-first; `DELETE` =
solo después de migración, búsquedas de uso y pruebas equivalentes.

### 6.1 Crates activos (miembros del workspace)

| Crate | Ficheros `.rs` | Clasificación | Propietario | Consumidores |
|---|---|---|---|---|
| `crates/kernel` | activo | **KEEP** | dominio compartido (`hrn`, `entity`, `application`) | `hodei-iam`, `hodei-policies`, raíz |
| `crates/hodei-iam` | activo | **KEEP** | BC IAM (11 slices en `features/`) | raíz (`src/handlers/iam.rs`), `hodei-policies` vía `evaluate_iam_policies` |
| `crates/hodei-policies` | activo | **KEEP** | BC Policies (8 slices en `features/`) | raíz (`src/handlers/policies.rs`, `playground.rs`), `hodei-iam` |
| raíz `.` (`hodei-artifacts-api`) | activo | **KEEP** (effects) | HTTP, config, wiring | binario `hodei-artifacts-api` |

### 6.2 Directorios de crate no miembro (excluidos de compilación)

Ninguno compila hoy. Se listan con su recuento real de ficheros `.rs`:

| Directorio | `.rs` | Clasificación | Propietario / consumidor esperado | Motivo |
|---|---|---|---|---|
| `crates/artifact` | 101 | **PORT** | onboarding de artefactos | ontología de artefactos recuperable; modelo SBOM duplicado con `supply-chain` |
| `crates/distribution` | 77 | **PORT** | proxy/hosted de repos | handlers de formato y `mvn`; `Option<Vec<u8>>` a sustituir por streaming (H2) |
| `crates/hodei-authorizer` | 21 | **QUARANTINE** | composición IAM+SCP | junta IAM y SCP en un mismo `PolicySet`:Incorrecto para límites. No reactivar sin deny-overrides |
| `crates/hodei-organizations` | 81 | **PORT** | multi-tenancy | organización/tenant; base de H1 (aislamiento) |
| `crates/policies` | 23 | **QUARANTINE** | policies legado | antecesor de `hodei-policies`; la CI ejecutaba `-p policies` sobre un crate no miembro (falso verde) — ya reconciliado |
| `crates/repository` | 43 | **PORT** | repositorios | ADT `Hosted/Proxy/Virtual` rescatable; `Private/Internal` de ejemplo nunca recuperable |
| `crates/search` | 64 | **PORT** | indexación | consumers de H7 (dependency accelerator) |
| `crates/security` | 62 | **PORT** | seguridad/signing | firma y verificación de artefactos (H2/H4) |
| `crates/supply-chain` | 19 | **PORT** | cadena de suministro | normalizadores SBOM; debe fundirse con `artifact` en un único modelo (H5) |

### 6.3 Suites en cuarentena

| Suite | Estado | Sustituto permanente |
|---|---|---|
| `tests/legacy/` (e2e, integration, common) | no compilada (sin `main.rs`) | `tests/http_contract.rs` (8) + `tests/bootstrap_integration.rs` (7) + `tests/binary_contract.rs` (2) |
| `crates/hodei-iam/tests/integration_{delete,get,list,update}_policy_test.rs.disabled` | 4 ficheros, no compilados | slices `delete_policy`, `get_policy`, `list_policies`, `update_policy` de `hodei-iam` (212 tests de lib en verde) |
| `crates/hodei-policies/tests/integration_test.rs.disabled` | 1 fichero, no compilado | `hodei-policies` lib (198 tests) + `tests/bootstrap_integration.rs` |

La cuarentena **no** es un olvido: `tests/legacy_quarantine.rs` (5 tests) verifica
que todo fichero aparcado está nombrado en `tests/legacy/README.md`, que el README
no nombra ficheros que ya no están aparcados, que el número de tests aparcados
coincide con el declarado, que `tests/legacy/` no puede volver a convertirse en
diana de Cargo y que el contrato sustituto existe. Si alguien añade un test
dentro de la cuarentena sin actualizar el registro, el test falla.

### 6.4 Dependencias rotas y no declaradas: aislamiento verificado

H0 exige aislar el uso de `shared`, `mongodb` y `aws-*` **no declarados**, sin
habilitar builds ficticios. Estado medido:

- **Crates activos** (`kernel`, `hodei-iam`, `hodei-policies`, raíz): **0
  ficheros** que usen `shared::`, `mongodb` o `aws_*`. Ningún `Cargo.toml` del
  workspace los declara. El aislamiento ya es real, no una intención.
- **Crates no miembro**: ahí se concentra el uso roto, y ahí se queda. El
  recuento por crate (ficheros que mencionan cada símbolo):

| Directorio | `shared` | `mongodb` | `aws-*` |
|---|---|---|---|
| `crates/artifact` | 30 | 2 | 1 |
| `crates/distribution` | 10 | 7 | 5 |
| `crates/hodei-authorizer` | 1 | 0 | 0 |
| `crates/hodei-organizations` | 0 | 0 | 0 |
| `crates/policies` | 15 | 0 | 0 |
| `crates/repository` | 30 | 13 | 0 |
| `crates/search` | 5 | 2 | 0 |
| `crates/security` | 16 | 2 | 0 |
| `crates/supply-chain` | 9 | 0 | 0 |

**Mecanismo de aislamiento:** la exclusión de `[workspace] members` impide que
`cargo metadata` los incluya, que `cargo check --workspace` los compile y que
`cargo nextest run --workspace` los ejecute. Ese es el aislamiento: **no se
habilita ningún build ficticio** y no se declara ninguna dependencia fantasma
para que compilen. Cuando un crate se reclasifique (H2–H5), la migración de sus
dependencias rotas es parte del trabajo de ese crate, no de H0.

### 6.5 Migración a Rust 2024

Los cuatro crates activos declaran `edition = "2024"` (heredado de
`[workspace.package]`). `cargo check --workspace --all-targets` compila sin
errores ni warnings, y `cargo clippy --workspace --all-targets -- -D warnings`
no encuentra nada. No hay mapa de migración pendiente en el código activo: la
deuda de Rust 2024 está íntegramente en los nueve crates no miembro, y se
resolverá crate a crate según su reclasificación en §6.2.

---

## 7. Defectos residuales

### 7.1 Corregidos en H0 (verificados por mutación)

| Id | Defecto | Por qué era un falso verde | Cómo se corrigió | Prueba discriminante |
|---|---|---|---|---|
| D1 | Una respuesta de registro individual de SurrealDB se deserializaba en `serde_json::Value` | el `id` del registro llega al visitor JSON como enum; el error se tragaba el resultado | fila `#[derive(Deserialize)]` tipada | `surrealdb_integration_test` (6) |
| D2 | `create` sobre un id existente fallaba con `RecordExists` | el segundo arranque contra la misma base moría; el servicio solo arrancaba una vez por base | `upsert` idempotente | `b5_bootstrapping_twice_against_the_same_database_succeeds` |
| D3 | El schema Cedar se persistía con `format!("{:?}", schema)` y se reconstruía **vacío** en cada carga | un schema vacío hace que *toda* política que mencione un tipo declarado parezca inválida y que *toda* política sin tipos parezca válida: el endpoint público `POST /api/v1/policies/validate` daba verde falso | codec `cedar-schema-fragments-v1` sobre `SchemaFragment`, que sí hace round-trip fiel | `b6_the_stored_schema_is_rebuilt_with_the_declared_types` + `b7_a_policy_over_an_undeclared_entity_type_is_rejected` (negativo) |
| D4 | El "último schema" no se encontraba (`latest` mal resuelto) | la validación leía un schema arbitrario o ninguno | selección ordenada por `created_at` y `select(("schema", version))` | `b2_the_registered_schema_is_persisted_and_readable_by_version` + `b3_reading_back_an_unregistered_schema_version_fails` (negativo) |
| D5 | `tests/test_register_iam_schema_integration.rs` era un fichero de **1 byte** con un salto de línea | Cargo compilaba un binario de test con **cero** tests: la suite contaba como verde sin afirmar nada | sustituido por `tests/bootstrap_integration.rs` (B1–B7, con dos negativos) | `legacy_quarantine.rs` Q5 + las 7 dianas de `bootstrap_integration` |
| D6 | El override documentado `HODEI_<SECCIÓN>__<CLAVE>` era **inerte**: config-rs 0.14.1 busca el prefijo como `HODEI__` cuando se da un separador de clave sin separador de prefijo explícito, y descarta toda variable en silencio | el servicio arrancaba siempre con los valores del TOML; nada en la suite lo observaba porque todos los tests configuraban en memoria | `.prefix_separator("_").separator("__").try_parsing(true)` | `environment_variables_override_the_configuration` + `an_invalid_environment_override_is_rejected` (negativo) + `p1_the_binary_serves_health_with_the_configuration_from_the_environment` (proceso real) + `p3_the_binary_exits_with_a_failure_code_on_an_invalid_configuration` (negativo) |
| D7 | El esquema persistido era **invisible para el lector** cuando el registro no nombraba versión: `save_schema` escribía `version: null` y `get_latest_schema` selecciona el registro **por su versión**, así que un esquema sin versión nunca era seleccionado | el esquema quedaba en la base y, aun así, todo consumidor concluía que no había esquema → la validación de políticas **degradaba a una simple comprobación de sintaxis** y el servicio aceptaba políticas sobre acciones y tipos de entidad no declarados. Es el mismo falso verde que D3, por la otra mitad del camino: D3 rompía la reconstrucción, D7 rompía el *encontrar* | invariante `UNVERSIONED_SCHEMA_ID` (`src/bootstrap.rs:129`): el id del registro es su versión, y el registro declara siempre la versión bajo la que puede volver a encontrarse; el lector cae en ese id cuando la versión es nula (`src/bootstrap.rs:198`, `src/bootstrap.rs:244`) | `tests/schema_persistence.rs` S1 (se encuentra sin versión) + S2 (control: se encuentra con versión) + S3 (**discriminante**: el esquema registrado es el que valida) — S3 incluye los dos negativos: acción no declarada y tipo de entidad no declarado |
| D8 | La distribución publicada **no arrancaba**: el paquete incluía `config/example.toml`, un fichero que el binario **nunca lee** (lee `config/default.toml`), y el binario deserializa la configuración de forma estricta | la aceptación efímera anterior se había ejecutado desde la raíz del repositorio —donde sí existe `config/development.toml`— y por eso dio verde sobre un artefacto que no arrancaba solo. Un consumidor que desempaquetaba la distribución obtenía «missing field `db_type`» | `config/default.toml` en el repositorio (la configuración base que el binario sí lee) + `scripts/package-release.sh`, que **se niega a construir** un paquete sin ese fichero y lo incluye en el tarball | `tests/distribution_contract.rs` P4 (el desempaquetado arranca solo y sirve `/health`) + P5 (**negativo**: sin configuración legible el binario se niega a arrancar, en vez de arrancar con supuestos) |
| D9 | La regresión permanente `b5_bootstrapping_twice_against_the_same_database_succeeds` era **intermitente**: al soltar el primer estado, el bloqueo exclusivo de RocksDB sobre el directorio **no se libera de forma determinista**, y el segundo arranque podía fallar con «lock hold by current process … No locks available» | el fallo depende del momento en que el motor embebido suelta el `LOCK`, no de la garantía que la prueba afirma: la idempotencia de persistir el esquema. Se observó **en la CI** (run `40042587344`, commit `91d752f`, un commit de solo documentación): la CI quedó en rojo sin que hubiera cambiado una línea de código de producción | reintento **acotado** (10 s) y **dirigido**: solo se reintenta el error de bloqueo del almacén embebido; cualquier otro fallo —«already exists», una aserción— falla de inmediato, así que la garantía de idempotencia no queda debilitada | `b5` sigue siendo la diana de D2: el mutante `create` en lugar de `upsert` hace que b5 **falle de inmediato** con «Database record `schema:`v1.0.0`` already exists», sin que el reintento lo tape |

**Verificación por mutación focalizada** (cada mutante debe morir; todos murieron y
se revirtieron):

| Mutante | Test que murió |
|---|---|
| validator vuelve a construir el schema vacío | `b7` FAILED |
| `load_schema::parse_schema` vuelve al schema vacío | `test_a_stored_content_that_is_not_a_schema_is_rejected` + `test_load_latest_schema` FAILED |
| se elimina `.prefix_separator("_")` | `environment_variables_override_the_configuration` FAILED (3000 vs 41535), `an_invalid_environment_override_is_rejected` FAILED, `p1` FAILED, `p3` FAILED |
| `main.rs` sale con `exit(0)` en error de configuración | `p3` FAILED (`p1` sobrevivió, como se esperaba) |
| `save_schema` vuelve a escribir `version: null` (se elimina la invariante `UNVERSIONED_SCHEMA_ID`) | `s1_a_schema_stored_without_a_version_is_found_again` FAILED (`left: None`) + `s3_the_registered_schema_is_the_schema_that_validates` FAILED (exactamente en «a policy over an undeclared action must be rejected»); `s2` (control) siguió pasando |
| la configuración distribuida no es la que el binario lee (se retira `config/default.toml`) | `p4_the_unpacked_distribution_boots_on_its_own` FAILED |
| la configuración pasa a ser opcional (`#[serde(default)]` en toda la jerarquía) | `p5_the_binary_refuses_to_start_without_a_readable_configuration` FAILED (`p4` sobrevivió, como se esperaba) |
| la configuración sí es legible en P5 (mutante del propio test) | `p5_the_binary_refuses_to_start_without_a_readable_configuration` FAILED **por la razón correcta** («the binary must refuse to start on its own») y **no dejó ningún proceso huérfano**: es lo que motivó el guardián `KillOnDrop` en ese test |
| `save_schema` vuelve a `create` (escritura no idempotente) con el reintento por bloqueo ya presente | `b5_bootstrapping_twice_against_the_same_database_succeeds` FAILED **de inmediato** con «Database record `schema:`v1.0.0`` already exists»: el reintento solo tolera el error de bloqueo, no el defecto de idempotencia |

### 7.2 Abiertos (con consumidor y criterio de cierre)

| Id | Defecto | Evidencia | Impacto | Cierre previsto |
|---|---|---|---|---|
| O1 | `POST /api/v1/policies/evaluate` devuelve un **stub fijo** (`decision: "Deny"`, `reasons: ["Evaluation not yet implemented"]`). El `EvaluatePoliciesPort` **sí** está cableado en `AppState`, pero el handler lo ignora (`State(_state)`) | `src/handlers/policies.rs:203-229` | el consumidor no puede decidir acceso por esta vía HTTP | H1: el handler debe invocar el puerto ya cableado |
| O2 | `GET /api/v1/schemas/load` devuelve `{"status":"stub"}` | `src/handlers/schemas.rs:142-149` | el esquema activo no es observable por HTTP | H1: delegar en `LoadSchemaPort` |
| O3 | `src/infrastructure/` es un módulo **no compilado**: no está declarado en `src/lib.rs`, así que `GetEffectiveScpsAdapter` no existe en el binario | `src/lib.rs` (sin `pub mod infrastructure`), `src/infrastructure/adapters/` | código que aparenta ser infraestructura sin estarlo | H1: declararlo y usarlo, o eliminarlo |
| O4 | `Cargo.lock` está en `.gitignore` | `.gitignore:5` | el build no es reproducible; el lock resuelto (Cedar 4.14.0, config-rs 0.14.1, SurrealDB 2.7.0) no se versiona. **Consecuencia observada en la CI**: `rust-ci.yml` usa `hashFiles('**/Cargo.lock')` como clave de caché, y como ese fichero no está en el repositorio la clave es constante → la caché nunca se invalida al cambiar una dependencia | H1: versionar el lock del workspace (el borrado fue deliberado, commit `eb42752 "borrado cargo.lock file"`, así que **no** se revierte en H0 sin decisión del maintainer) |
| O5 | `ActionTrait::action_name()` produce `Iam::Action::"X"`; el evaluador construye `Action::"X"` (namespace vacío) | `crates/kernel/src/domain/entity.rs:403-407` vs `crates/hodei-policies/src/internal/engine/core.rs:81` | divergencia latente: cuando un comando pase por la ruta de evaluación, el UID de acción no coincide con el del schema | H1: unificar la forma del UID de acción con el esquema IAM |
| O6 | El adaptador SurrealDB de `evaluate_iam_policies` no ejecuta consulta alguna (`TODO: Implement actual SurrealDB query`) y el use case pasa `context: None` | `crates/hodei-iam/src/features/evaluate_iam_policies/adapter.rs:93`, `.../use_case.rs:176` | la evaluación de IAM no consulta las políticas persistidas | H1 |
| O7 | El cargador de schema del playground **acepta y descarta** un schema JSON no vacío: registra un warning y construye un schema **vacío** | `crates/hodei-policies/src/features/playground_evaluate/adapters/schema_loader.rs:86-94` | una evaluación contra un schema inline no valida contra ese schema: el mismo falso verde que D3 corrigió en la ruta de persistencia | H1: decodificar el JSON con el codec de §7.1 D3 |
| O8 | El motor de evaluación **carga** el schema pero no lo **aplica** (`TODO: we should actually use the loaded schema`) | `crates/hodei-policies/src/features/evaluate_policies/use_case.rs:267-270` | la decisión no se valida contra el esquema activo | H1: pasar el schema al motor |
| O9 | CORS permisivo en el router | `src/router.rs:52` (`CorsLayer::permissive()`) | postura de producción insegura en el borde HTTP | H1: CORS por origen configurado |

**Distinción que conviene no perder:** `POST /api/v1/playground/evaluate` **sí**
evalúa de verdad (carga schema, valida políticas, evalúa). El stub es
`POST /api/v1/policies/evaluate`. No son la misma ruta.

---

## 8. Aceptación efímera de H0 (ejecutada y limpiada)

Ciclo `PREPARE → EXECUTE → ASSERT → EVIDENCE → CLEANUP` sobre la
**distribución desempaquetada**, ejecutada desde el directorio del paquete (que
es como la usa un consumidor), con directorio de sesión efímero y puerto
efímero.

- Paquete: `hodei-artifacts-api-0.1.0-x86_64-unknown-linux-gnu.tar.gz`
- Binario bajo prueba:
  `sha256=bd70d31c87d93a570830993f5da7090616fe735cb688e70319a2978c2dac8d5f`

**Corrección de una evidencia anterior.** La primera aceptación de este
documento (`sha256=b78a3e38…`) se ejecutó **desde la raíz del repositorio** y
dio verde, pero esa ejecución no era válida: el artefacto publicado no
arrancaba por sí solo (D8). La aceptación que sigue se ejecutó sobre el
paquete desempaquetado y desde su propio directorio, que es la condición que
un consumidor impone. El resultado del binario es el mismo
(`sha256=bd70d31c…`): los cambios de D8 fueron de empaquetado y de
configuración, no de código compilado.

| Caso | Resultado |
|---|---|
| A1 el desempaquetado arranca desde su propio directorio | PASS — `GET /health` → 200, cuerpo `{"status":"healthy","version":"0.1.0","service":"hodei-artifacts-api",...}` |
| A1 `GET /health/ready` | PASS — 200 |
| A1 `GET /health/live` | PASS — 200 |
| A2 registro del esquema IAM | PASS — `entity_types_registered: 2`, `action_types_registered: 6`, `schema_version: "latest"`, `schema_id: "schema:latest"` |
| A2 política sobre una acción registrada (`Action::"CreateUser"`) | PASS — `is_valid: true` |
| A2 política sobre un tipo de entidad registrado (`Iam::User::"alice"`) | PASS — `is_valid: true` |
| A3 (**negativo**) política sobre una acción no declarada | PASS — `is_valid: false`, «unrecognized action `Action::\"NoSuchAction\"`» |
| A3 (**negativo**) política sobre un tipo de entidad no declarado | PASS — `is_valid: false`, «unrecognized entity type `Nope::Ghost`» |
| A4 (**negativo**) configuración inválida (`HODEI_SERVER__PORT=0`) | PASS — exit 1, «Configuration error: Server port cannot be 0» |
| Limpieza: `SIGTERM` | PASS — el proceso termina |
| Limpieza: directorio de sesión efímero | PASS — eliminado |

Los dos negativos de A3 fallan **por la razón de Cedar** —el mensaje nombra
la acción o el tipo de entidad no declarados—, no por un conflicto de puerto
ni por una configuración ausente.

**Reejecución sobre los bytes publicados.** Una vez publicado el release, los
assets se descargaron de nuevo desde GitHub y se repitió el ciclo completo
A1–A4 sobre ese tarball descargado: todos los casos siguen en PASS y el
binario extraído conserva
`sha256=bd70d31c87d93a570830993f5da7090616fe735cb688e70319a2978c2dac8d5f`.
La aceptación queda, por tanto, ligada a los bytes que un consumidor descarga,
no solo a una copia local del mismo commit.

**Corrección de la evidencia de limpieza.** La comprobación «el proceso
termina en `SIGTERM`» de las primeras ejecuciones era un **falso verde**: el
guion enviaba la señal al PID del *subshell*, no al del binario, de modo que el
servidor sobrevivía mientras el guion reportaba una parada limpia. Se
detectaron **cuatro procesos `hodei-artifacts-api` huérfanos** (uno de ellos
procedente de una ejecución del mutante de configuración opcional). El defecto
estaba en el guion de aceptación, no en el producto: el binario **sí** responde
a `SIGTERM` (termina con código limpio al recibirlo, comprobado). El guion se
corrige ejecutando el binario con `exec` —así `$!` es el PID real— y
comprobando además que no sobrevive ninguna instancia. Con el guion corregido,
A1–A4 se repitieron sobre los bytes publicados: todos PASS y **0 procesos
huérfanos**. El manifiesto y las notas del release publicó registran esta
corrección.

**Negativos discriminantes de configuración** (cada uno debe fallar **por su
razón**, no por un conflicto de puerto ni por una variable duplicada):

| Override inválido | Resultado observado |
|---|---|
| `HODEI_SERVER__PORT=0` | exit 1 — "Server port cannot be 0" |
| `HODEI_SERVER__PORT=abc` | exit 1 — "invalid type: string \"abc\", expected an integer for key `server.port`" |
| `HODEI_DATABASE__DB_TYPE=mysql` | exit 1 — "Unsupported database type 'mysql'" |
| `HODEI_DATABASE__NAMESPACE=` | exit 1 — "Database namespace cannot be empty" |
| `HODEI_ROCKSDB__PATH=` | exit 1 — "RocksDB path cannot be empty" |
| `HODEI_ROCKSDB__PATH=/proc/1/x` | exit 1 — "Parent directory '/proc/1' is not writable" |
| sin fichero de configuración legible | exit 1 — "missing field …" (P5) |

---

## 9. Primeros riesgos (P0)

1. Colisión inter-tenant de Cedar UID: HRN serializado en EntityUid completo y estable. **Abierto** (H1).
2. Política SCP entendida como `permit` unido a IAM: límites aplicados por intersección, nunca como grant. **Abierto** (H1); `crates/hodei-authorizer` no se reactiva.
3. Estado mutable compartido entre decisiones Cedar concurrentes: evaluación por snapshot inmutable. **Abierto** (H1).
4. Migración de Surreal/RocksDB a TypeDB: inventario y exportación previa; rollback y verificación. **Bloqueado por entorno**: `typedb` y `typedb-server` ausentes → `NOT_RUN`.
5. Descarga a memoria completa: cambiar a streaming con backpressure y checksums. **Abierto** (H2).
6. TypeDB CE single-node: resiliencia, límites de disponibilidad y restore demostrados. **Bloqueado por entorno** (H2).

---

## 10. Publicación de v0.1.0 (autoridad CI + artefacto publicado)

**Autoridad CI.** La workflow `Rust CI` (`.github/workflows/rust-ci.yml`) sobre
`main` en el commit de origen `c3d79a6…` está **verde** en todos sus pasos
(`run 40040582255`, conclusión `success`):

| Paso | Resultado |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo build --workspace` | PASS |
| `cargo test --workspace --no-fail-fast` | PASS — 746 superadas, 0 fallidas, 64 ignoradas |
| `cargo nextest run --workspace --no-fail-fast` | PASS — 740 ejecutadas, 0 saltadas, 0 fallidas |

La segunda señal (nextest, un proceso por prueba) coincide con la primera: el
workspace no comparte estado mutable entre pruebas.

**Un rojo posterior, en un commit de solo documentación.** La CI del commit
`91d752f` (que solo toca `BASELINE.md` y `roadmap.json`) quedó en **rojo** con
`b5_bootstrapping_twice_against_the_same_database_succeeds` fallando por el
bloqueo de RocksDB descrito en D9. El release se corta en `c3d79a6`, cuya CI
(`40040582255`) estaba verde, así que **el artefacto publicado no arrastra
este defecto**; pero la intermitencia era real y está corregida en `main`
(reintento acotado y dirigido, verificado con 25 repeticiones del test y 15
del suite completo, más el mutante que demuestra que el reintento no tapa la
falta de idempotencia). Una CI verde no es solo un resultado: es una
propiedad que hay que poder repetir.

**Estado actual de `main`.** Tras la publicación, `main` siguió avanzando con
correcciones de higiene (el guardián `KillOnDrop` de los procesos de prueba y
la regresión D9) y con commits de solo documentación. La CI de la cabeza actual
`82df00e` (run `40043755981`) está **verde** en todos sus pasos, con
`746 passed, 0 failed, 64 ignored` y nextest `740 passed, 0 skipped`. Las CIs
intermedias (`40042608482` sobre `a02e692`, `40042980376` sobre `0c1c4df` y
`40043317881` sobre `afa1388`) también están verdes, de modo que desde la
corrección de D9 no ha vuelto a aparecer un rojo. El release sigue siendo el de
`c3d79a6`: las correcciones posteriores no cambian el artefacto publicado (los
tests no se distribuyen).

**Publicación observada** (no declarada: comprobada contra la API y contra el
remote):

| Hecho | Valor observado |
|---|---|
| Tag | `v0.1.0` (anotado) → `c3d79a694c7b9e9f7f9c1c359f5f309933b41899` |
| `refs/heads/main` remoto en el momento de etiquetar | `c3d79a694c7b9e9f7f9c1c359f5f309933b41899` (el tag apunta a main) |
| Release | `https://github.com/Rubentxu/hodei-artifacts/releases/tag/v0.1.0` |
| Fecha de publicación | 2026-10-23T23:57:14Z |
| Assets publicados | `hodei-artifacts-api-0.1.0-x86_64-unknown-linux-gnu.tar.gz`, `SBOM.cdx.json`, `SHA256SUMS`, `release-manifest.json`, `CHANGELOG.md` |
| `sources.zip` | **no** es asset: los enlaces "Source code" que genera GitHub no son assets publicados |

**Verificación posterior a la publicación.** Los assets descargados del release
verifican contra `SHA256SUMS` (`sha256sum -c`: 3/3 coinciden) y el binario
dentro del tarball publicado conserva
`sha256=bd70d31c87d93a570830993f5da7090616fe735cb688e70319a2978c2dac8d5f`,
el mismo valor registrado en el manifiesto y en la aceptación.

El `release-manifest.json` publicado
(`sha256=e64377d7938468520a7047fdd51a336cdbaaedacea6afa0c0844c054588358be`)
incluye la corrección de la evidencia de limpieza descrita en §8; las notas del
release la resumen. `SHA256SUMS` no cubre el manifiesto (un manifiesto que
contenga su propia suma es imposible de cerrar), y por eso el manifiesto es el
único asset cuyo digest no figura dentro de otro asset.

**No es GA y no declara paridad con Nexus CE**, tanto en el texto del release
como en `release-manifest.json` (`not_a_ga_release`, `no_nexus_parity_claim`).

---

**Condición inicial (sigue vigente):** no aceptar labels de README ni el count
de tests de Makefile como evidencia; medir ejecutando cargo real y clientes
reales. El censo de este documento se obtuvo ejecutando la suite completa
(`cargo test --workspace --no-fail-fast`, exit 0) y un binario real (no un
mock), y cada corrección de §7.1 tiene un mutante que demuestra que su prueba
discrimina.
