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
| `src/infrastructure/` | **eliminado en H1b-4**: era un módulo muerto (no está declarado en `src/lib.rs`, así que nunca se compila) y contenía `GetEffectiveScpsAdapter` | DELETE ejecutado: el código no compilado no es evidencia de nada (ver §7.7) |
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

**O1 y O5 se cerraron en H1a** (§7.3), **O2 y O7 se cerraron en H1b-1** (§7.4),
**O6 se cerró en H1b-3** (§7.6) y **O3 se cerró en H1b-4** (§7.7), cada uno con
su mutante y su prueba discriminante. Los que siguen aquí siguen abiertos.

| Id | Defecto | Evidencia | Impacto | Cierre previsto |
|---|---|---|---|---|
| O4 | `Cargo.lock` está en `.gitignore` | `.gitignore:5` | el build no es reproducible; el lock resuelto (Cedar 4.14.0, config-rs 0.14.1, SurrealDB 2.7.0) no se versiona. **Consecuencia observada en la CI**: `rust-ci.yml` usa `hashFiles('**/Cargo.lock')` como clave de caché, y como ese fichero no está en el repositorio la clave es constante → la caché nunca se invalida al cambiar una dependencia | **H1b**: versionar el lock del workspace (el borrado fue deliberado, commit `eb42752 "borrado cargo.lock file"`, así que **no** se revierte en H0 sin decisión del maintainer) |
| O8 | El motor de evaluación **carga** el schema pero no lo **aplica** (`TODO: we should actually use the loaded schema`) | `crates/hodei-policies/src/features/evaluate_policies/use_case.rs:267-270` | la decisión no se valida contra el esquema activo | **Mitad del playground CERRADA en H1b-2** (§7.5). La **mitad del motor sigue abierta**, y su causa está medida: el esquema registrado es de alcance IAM y `POST /api/v1/policies/evaluate` es genérico, así que aplicarlo en crudo **denegaría toda petición fuera del mundo IAM** (los casos `G9`–`G12` solo sobreviven porque pasan `evaluation_mode: "NoSchema"`). Cerrarla exige decidir **qué esquema es el activo para una petición genérica**: una decisión de alcance de plataforma, no de código. Ver `ADR/011` |
| O9 | CORS permisivo en el router | `src/router.rs:52` (`CorsLayer::permissive()`) | postura de producción insegura en el borde HTTP | **H1b**: CORS por origen configurado |
| O10 | Cuatro ficheros `.rs` de `crates/hodei-iam` quedan **fuera de la unidad de compilación**: ninguna declaración `mod` los alcanza y rustc no los compila. Medido sobre el árbol real con un resolutor de módulos (§15.1): `features/add_user_to_group/factories.rs`, `features/add_user_to_group/mocks.rs`, `features/create_policy/validation_port.rs` e `internal/domain/events.rs` | `crates/hodei-iam/src/features/add_user_to_group/mod.rs` (declara `dto`, `error`, `ports`, `use_case`, `use_case_test`; **no** `factories` ni `mocks`), `crates/hodei-iam/src/features/create_policy/mod.rs` (**no** `validation_port`), `crates/hodei-iam/src/internal/domain/mod.rs` (declara `actions`, `group`, `user`; **no** `events`) | a diferencia de O3, el crate es **vivo** y el código es trabajo sin cablear, no una rama en cuarentena: `AddUserToGroupUseCase` **no se construye fuera de su test unitario** (la factoría DI que lo construiría es justamente uno de los huérfanos), `validation_port.rs` es un **duplicado obsoleto** del `PolicyValidator` que `features/create_policy/ports.rs` ya reexporta, y los eventos de dominio (`UserCreated`, `GroupCreated`) **no los publica nadie** | **H1b**, con criterio **por fichero**: `validation_port.rs` se **elimina** (duplicado obsoleto); `factories.rs` y `mocks.rs` se **declaran y cablean** o se eliminan, según si la slice se compone; `events.rs` se **declara y publica** en el bus de eventos o se elimina. Medido además: `kernel` y `hodei-policies` **no tienen** huérfanos |

**Lo que H1b-1 no abre ni cierra aquí.** H1b-1 leyó el schema en las dos rutas de
lectura (el playground y el endpoint de carga) y lo aplicó a la **validación**
del playground. **O8 seguía abierto** tras H1b-1: el motor de evaluación seguía
cargando el schema sin aplicarlo a la petición, y el evaluador del playground
seguía decidiendo sin schema (`Authorizer::new()`). Lo que H1b-1 arregla es la
*lectura*; lo que O8 pide es la *decisión*. Ver §7.4 y §12.

**Lo que H1b-2 sí cierra y lo que deja bloqueado.** H1b-2 cierra la **mitad del
playground** de O8: el playground decide **según** el schema que el consumidor
envía (§7.5). La **mitad del motor** sigue abierta y su causa está medida, no
supuesta: el esquema que el motor carga es el **esquema registrado**, que es de
alcance IAM, mientras que `POST /api/v1/policies/evaluate` acepta peticiones
genéricas. Aplicar ese esquema en crudo **denegaría toda petición fuera del
mundo IAM** — y eso no es un defecto que se pueda cerrar tocando el código: es
una pregunta de alcance de plataforma («¿qué esquema gobierna una petición
genérica?») que este ciclo no tiene autoridad para responder. El hueco queda
**registrado con su causa**, no difuminado. Ver §7.5 y §13.

**Distinción que conviene no perder:** `POST /api/v1/playground/evaluate` **sí**
evalúa de verdad (carga schema, valida políticas, evalúa). El stub era
`POST /api/v1/policies/evaluate`. No son la misma ruta. Desde H1a la segunda
también evalúa de verdad (§7.3, O1).

**Lo que H1b-3 cierra, y cómo se Midió la afirmación heredada.** La fila de
O6 decía «la evaluación IAM no consulta las políticas persistidas». Al
investigar, esa frase resultó **cierta pero mal acotada**, y la diferencia es
lo que decide el alcance del hito. El `TODO` que citaba estaba **en la
superficie equivocada**: el adaptador SurrealDB de `evaluate_iam_policies`
**no ejecutaba consulta alguna** (devolvía un conjunto vacío), pero el hueco
real era **más grande** que ese `TODO`. Medido:

| Hecho medido en el código | Consecuencia |
|---|---|
| el `PolicyFinderPort` de `evaluate_iam_policies` lo implementaba un **stub en memoria** que no tocaba la base de datos | la evaluación IAM no consultaba **nada** persistido: ni la base, ni siquiera un store |
| el use case `EvaluateIamPoliciesUseCase` **nunca se construía** fuera de los tests unitarios | no había caso de uso vivo que décision |
| el Composition Root **no declaraba** el puerto `EvaluateIamPoliciesPort` | no había cableado de DI: la ruta no existía |
| **no había ruta HTTP** para la evaluación IAM | el consumidor no tenía superficie que usar |
| los resolvers de principal y de recurso (`PrincipalResolverPort`, `ResourceResolverPort`) **no tenían implementación** | aunque hubiera datos, no había entidades que materializar |
| el registro persistido **no guardaba ninguna atribución**: `create` escribía solo `{content, ...}`, y `attached_principals` **nunca se persistía** | una política almacenada no decía **a quién** sirve: no había nada por lo que «consultar las políticas del principal» significara algo |
| el `EvaluationRequest` del kernel **no tiene campo `context`** | el `context: None` que citaba la fila de O6 señalaba un **hueco distinto** (el contexto no viaja por el kernel), no la ausencia de consulta |

Cerrar el `TODO` literal —poner una consulta real en el stub y seguir
—habría producido un **verde falso**: el sistema seguiría sin política
atribuida a nadie, sin caso de uso vivo, sin ruta y sin resolvers. H1b-3
cierra el hueco **real**, que es el de la fila, y por eso se reshapea el hito
antes de ejecutarlo (§7.6, §14).

**O6 y O8 no se tocan entre sí.** La mitad bloqueada de O8 (§7.2) es la del
motor genérico `POST /api/v1/policies/evaluate`, cuya causa es que el esquema
**registrado** es de alcance IAM mientras la ruta es genérica. O6 es la ruta
**IAM-específica** (`POST /api/v1/iam/evaluate`), donde el mundo es IAM y el
esquema aplicable es el IAM. Por eso O6 **se cierra sin** la decisión de
alcance de plataforma: no depende de ella. Lo que H1b-3 mantiene explícito es
que la ruta IAM **sigue sin aplicar el schema** (§14.6), y esa es una decisión
deliberada y registrada, no un descuido.

**Lo que H1b-4 cierra, y por qué la rama «declararlo» era imposible.** La fila
de O3 ofrecía dos cierres: «declararlo y usarlo» o «eliminarlo». Medido en el
código, **solo uno era alcanzable**:

| Hecho medido en el código | Consecuencia |
|---|---|
| `src/infrastructure/adapters/get_effective_scps_adapter.rs` importa `hodei_organizations` | el crate no es una dependencia del paquete raíz, así que **declarar** el módulo rompe la compilación |
| `crates/hodei-organizations` está **comentado** en `[workspace] members` y depende del crate legado `policies`, que también está comentado | reactivar la rama «declararlo» exige reactivar dos crates en cuarentena |
| el único consumidor de `kernel::GetEffectiveScpsPort` es `crates/hodei-authorizer`, también en cuarentena | nadie en el workspace vivo necesita ese adaptador |
| `SPECS/authorization-hrn.md` § «Migración legado» prohíbe reactivar `hodei-authorizer` tal cual | la rama «declararlo» choca con la especificación vigente |

Declarar el módulo, por tanto, no era «más trabajo»: era **imposible** sin
reactivar una rama que la especificación veta. La única lectura honesta es la
que la propia §5 recommending: **eliminar**. Y el binario **confirma** la
medición: el `sha256` del artefacto release es **idéntico** antes y después de
eliminar el módulo (§15.5), porque esos ficheros nunca fueron entrada de la
compilación.

**Lo que H1b-4 además deja registrado: O10.** Al medir el invariante que
protege el cierre (ningún `.rs` del crate de aplicación puede quedar fuera de
la unidad de compilación), el mismo resolutor encontró **cuatro** huérfanos en
`crates/hodei-iam` (§7.2, O10). No se han cerrado aquí: son código de un crate
**vivo**, y cerrarlos exige decidir por fichero si se cablea o se elimina —
una decisión de producto, no un borrado. El guard de H1b-4 es **deliberadamente
estrecho**: cubre el crate de aplicación, que es donde vivía O3.

---

### 7.3 Cerrados en H1a (verificados por mutación)

| Id | Defecto | Por qué era un falso verde | Cómo se corrigió | Prueba discriminante |
|---|---|---|---|---|
| O1 | `POST /api/v1/policies/evaluate` devolvía un **stub fijo** (`decision: "Deny"`, `reasons: ["Evaluation not yet implemented"]`). El `EvaluatePoliciesPort` ya estaba cableado en `AppState`, pero el handler lo ignoraba (`State(_state)`) | el endpoint respondía `200` con un `Deny` **correcto en la forma**: un consumidor que solo mirase el código de respuesta o la decisión vería un deny plausible. Ninguna prueba afirmaba una **decisión**: el contrato HTTP cubría salud, enrutado y ciclo de vida IAM, y nada comparaba la razón devuelta con la política enviada | el handler invoca el puerto cableado y devuelve la decisión real (`Allow`/`Deny`), las **políticas determinantes**, los motivos y los diagnósticos. Un HRN malformado, una acción vacía, un modo desconocido o un contexto que no sea un objeto son `400`, no una decisión silenciosa | `G9` (permit → `Allow` nombrando `policy_0`) + `G10` (**negativo**: forbid → `Deny` nombrando `policy_1`; sin políticas → `Deny`) + `G11` (**negativo**: HRN malformado → `400`) + `G12` (otra cuenta → `Deny` **sin** diagnóstico de error) + A6/A7/A8 de la aceptación efímera |
| O5 | `ActionTrait::action_name()` producía `Iam::Action::"X"`; el motor construía `Action::"X"` (namespace vacío) | las dos formas son sintaxis Cedar válida, así que ninguna prueba rompía: el divergente solo se manifiesta cuando una política escrita contra el **schema registrado** (`Action::"X"`) se evalúa con el UID de otra ruta (`Iam::Action::"X"`) — y esa ruta nunca se ejercitó | una **única** forma plana `Action::"<Nombre>"` en las tres rutas (motor, playground y `ActionTrait::action_name()`), construida con las APIs Cedar tipadas. El playground dejó de fabricar el UID con `EntityUid::from_str` sobre texto del HRN y pasó a usar el mismo traductor que el motor | `an_action_maps_to_the_flat_action_uid` (traductor) + `action_trait_full_name` y `test_create_user_action` (kernel/IAM) |

**Mutación focalizada de H1a** (cada mutante debe morir; ambos murieron y se
revirtieron):

| Mutante | Test que murió |
|---|---|
| el `EntityId` Cedar vuelve a ser el **id sin scope** (`hrn.resource_id()` en lugar del HRN canónico completo) | `G12` FAILED (`Deny` donde se esperaba `Allow`) y `A2` FAILED: el homónimo de otra cuenta vuelve a ser **la misma entidad** y la política de una cuenta alcanza a la otra — es la colisión inter-tenant (P0 #1) reintroducida |
| el motor vuelve a guardar **el último snapshot en estado compartido** y evalúa contra él | `A1` FAILED («request 0 was contaminated»): dos peticiones concurrentes con políticas opuestas se contaminan — es el state bleed (P0 #3) reintroducido |

Los dos mutantes se revirtieron tras observar la muerte de su prueba; la suite
volvió a verde (`http_contract` 12/12, `authorization_isolation` 3/3).

---

### 7.4 Cerrados en H1b-1 (verificados por mutación)

| Id | Defecto | Por qué era un falso verde | Cómo se corrigió | Prueba discriminante |
|---|---|---|---|---|
| O7 | El cargador de schema del playground **aceptaba y descartaba** un schema JSON no vacío: registraba un warning y construía un schema **vacío** (`schema_loader.rs`, rama no-`{}`) | la petición respondía `200` y el consumidor recibía una decisión: el fallo era **invisible**. Peor: como el schema vacío no declara nada, toda política que nombrase un tipo **parecía válida** — el mismo falso verde que D3 corrigió en la ruta de persistencia, por la otra mitad del camino (D3 rompía la reconstrucción; O7 rompía la lectura) | el schema inline se decodifica con el **mismo codec** que usa la persistencia (`cedar-schema-fragments-v1`, §7.1 D3), de modo que las dos rutas de lectura coinciden en qué es un schema. Un contenido que no es un sobre de ese formato se **rechaza** (`400`) en vez de leerse como schema vacío; `{}` y `null` siguen siendo la petición explícita de un schema vacío. El validador **aplica** el schema leído: una política sobre un tipo declarado se acepta y sobre un tipo no declarado se reporta | `G14` (**negativo**: tipo no declarado → error de validación que lo nombra; la misma política con `{}` → sin error, lo que prueba que el rechazo viene del schema) + `G15` (**negativo**: contenido que no es un sobre → `400`) + `G13` (control: tipo declarado → sin error) + 4 tests unitarios del cargador y 3 del validador |
| O2 | `GET /api/v1/schemas/load` devolvía `{"status":"stub"}` | el endpoint respondía `200` con un cuerpo **bien formado**: un consumidor que solo mirase el código de respuesta creía tener el esquema activo. El `LoadSchemaPort` ya estaba cableado en `AppState`, y el handler lo ignoraba | el handler delega en `state.load_schema.execute(LoadSchemaCommand::latest())` y devuelve el **estado explícito** (`active` / `no_active_schema`), la **versión**, el **identificador** y el **contenido** del esquema activo. Sin esquema almacenado la respuesta es `200` con `no_active_schema` y campos nulos: «no hay esquema» es una respuesta, no un error | `G17` (**negativo**: tras registrar el esquema IAM → `active`, versión `v1.0.0` y contenido que es un sobre `cedar-schema-fragments-v1` con fragmentos) + `G16` (**negativo**: sin nada almacenado → `no_active_schema` con versión y contenido nulos) |

**Mutación focalizada de H1b-1** (cada mutante debe morir; los tres murieron y
se revirtieron):

| Mutante | Test que murió |
|---|---|
| el cargador del playground vuelve a **aceptar y descartar** el schema inline (warning + schema vacío) | `G14` FAILED («a policy over an undeclared entity type must be reported») + `G15` FAILED (`200` donde se esperaba `400`) + 3 tests unitarios del cargador FAILED (`test_inline_envelope_is_read_as_the_schema_it_declares`, `test_inline_content_that_is_not_an_envelope_is_rejected`, `test_inline_envelope_with_an_unknown_format_is_rejected`); `G13` sobrevivió, como se esperaba: no afirma que el schema se lea, sino que un tipo declarado no se rechaza |
| el validador **aplica el schema aunque no declare nada** (se retira la guarda `declares_anything`) | `G14` FAILED en su segunda mitad («with no schema declared only the syntax can be checked») + `test_empty_schema_validates_syntax_only` FAILED («unrecognized entity type `Iam::User`»): un schema vacío rechazo policies que no incurre en error, un **falso rechazo**, no una validación |
| el handler de `GET /api/v1/schemas/load` vuelve al **stub** `{"status":"stub"}` | `G16` FAILED («the endpoint must answer with a declared status, not with a placeholder») + `G17` FAILED (`stub` donde se esperaba `active`) |

Los tres mutantes se revirtieron tras observar la muerte de su prueba; la suite
volvió a verde (`http_contract` 17/17).

---

### 7.5 Cerrados en H1b-2 (verificados por mutación)

O8 tenía **dos mitades**: el motor de evaluación (`evaluate_policies`) y el
playground. H1b-2 cierra la del playground y deja la del motor **bloqueada con
causa medida** (§7.2).

| Id | Defecto | Por qué era un falso verde | Cómo se corrigió | Prueba discriminante |
|---|---|---|---|---|
| O8 (mitad del playground) | El evaluador del playground decidía **sin schema**: construía la petición con `Request::new(..., None)` y el `Authorizer` no recibía ninguna autoridad. La acción la fabricaba el propio evaluador en la forma plana `Action::"X"`, y el **contexto de la petición se descartaba** en el use case | la respuesta era `200` con una decisión plausible, así que nada delataría el defecto: el playground **aceptaba** verbos que el schema no declaraba, **ignoraba** el namespace que el schema declaraba, **evaluaba** peticiones cuyo tipo el schema no conocía y **ninguna** política condicionada sobre el contexto podía coincidir nunca. El schema se leía (H1b-1) y se validaba contra, pero la **decisión** no dependía de él | el schema pasa a ser la **autoridad**: el verbo se resuelve **por id** contra `schema.actions()` (el namespace lo decide el schema), la petición **se construye con el schema** para que Cedar compruebe que encaja, y el **contexto** se propaga del comando al evaluador. Tres desenlaces que no admiten atajo: verbo no declarado → **deniega** nombrándolo; acción declarada dos veces → **deniega** nombrando **ambos** candidatos (elegir uno sería evaluar contra una acción que nadie pidió); petición que no encaja → **deniega** con el motivo. Un schema que **no declara nada** no se aplica: aplicarlo en crudo sería un falso rechazo | `G18` (**negativo**: verbo no declarado → `DENY` nombrándolo; y un schema que declara el tipo pero ningún verbo → `DENY` también) + `G19` (**negativo**: el namespace viene del schema — la misma política concede con el schema namespaced y **no** concede con el plano) + `G20` (**negativo**: el contexto llega — con la IP coincidente concede, con otra deniega) + `G21` (**negativo**: recurso cuyo tipo el schema no declara → `DENY` nombrando `Storage::Document`) + `G22` (**negativo**: schema vacío → `ALLOW`, no una falsa denegación) + `G23` (**negativo**: acción declarada dos veces → `DENY` nombrando `Hodei` **y** `Other`) + 7 tests unitarios de `internal/engine/authority.rs` y 4 del evaluador |

**Mutación focalizada de H1b-2** (cada mutante debe morir; los seis murieron y
se revirtieron):

| Mutante | Test que murió |
|---|---|
| **M1** el schema **no se aplica** a la petición (`Request::new` sin schema) | `G21` FAILED: el recurso de un tipo no declarado deja de ser una denegación y la política concede |
| **M2** el namespace de la acción se toma de la **petición** en vez del schema | `G19` FAILED: la política sobre `Hodei::Action::"read"` deja de conceder con el schema que declara ese namespace |
| **M3** el schema se aplica **aunque no declare nada** (se retira la guarda) | `G22` FAILED: el schema vacío pasa de no constreñir nada a denegar una petición que encaja — un **falso rechazo** |
| **M4** el **contexto** de la petición se descarta (`Context::empty()`) | `G20` FAILED: la política condicionada sobre `context.ip` deja de conceder con la IP coincidente |
| **M5** un verbo que el schema **no declara** cae a la forma plana y se evalúa **sin** schema | `G18` FAILED: un verbo inexistente en el mundo del schema pasa a estar autorizado — un **fail open** |
| **M6** una acción que el schema declara **dos veces** se resuelve al **primer** candidato | `G23` FAILED: la ambigüedad deja de denegar y la petición se evalúa contra una acción que nadie pidió |

Los seis mutantes se revirtieron tras observar la muerte de su prueba; la suite
volvió a verde (`http_contract` 23/23, `cargo test --workspace` 797 superadas).

---

### 7.6 Cerrados en H1b-3 (verificados por mutación)

| Id | Defecto | Por qué era un falso verde | Cómo se corrigió | Prueba discriminante |
|---|---|---|---|---|
| O6 | La evaluación IAM **no consultaba las políticas persistidas**. El hueco medido era de **cinco capas**, no una: (a) el `PolicyFinderPort` lo implementaba un **stub en memoria** que no tocaba la base de datos; (b) el use case **nunca se construía** fuera de los tests; (c) el Composition Root **no cableaba** el puerto; (d) **no había ruta HTTP**; (e) los resolvers de principal y de recurso **no tenían implementación**; y, además, (f) el registro persistido **no guardaba atribución** (`attached_principals` nunca se escribía), así que una política almacenada no decía a quién servía | el `TODO: Implement actual SurrealDB query` que citaba la fila señalaba **un** hueco visible y tapaba los otros cinco. La fila además mezclaba el `context: None` (que habla del **contexto**, no de la consulta) y daba por hecho que existía una política persistida **atribuida**, que no existía | la **política persistida declara sus principales**: `create` guarda `attached_principals` (HRNs canónicos validados: servicio `iam`, tipo `User`) y `get` los devuelve, así que la atribución **sobrevive al viaje de ida y vuelta**. El `IamPolicyFinderPort` **consulta SurrealDB de verdad** (`SELECT * FROM policy WHERE $principal_hrn IN attached_principals`) sobre el mismo adaptador que escribe, de modo que las políticas que deciden son **las que están guardadas**. Los resolvers **materializan las entidades** desde los HRNs de la petición, y el caso de uso **decide** con Cedar y nombra las políticas determinantes. La ruta se expone en `POST /api/v1/iam/evaluate` | `G24` (la política creada por la API y atribuida a un principal es **la que consulta y nombra** la evaluación: `policy_ids_evaluated` y `determining_policies`) + `G25` (**negativo**: una política atribuida a **otro** principal **ni se consulta** — `policy_ids_evaluated` vacío y motivo de denegación implícita) + el round-trip de la atribución en `G24` + 4 tests unitarios del adaptador y 2 del resolver |

**Mutación focalizada de H1b-3** (cada mutante debe morir; tres murieron y se
revirtieron, y uno resultó **equivalente**):

| Mutante | Test que murió |
|---|---|
| **M1** el finder devuelve **toda** política, ignorando la atribución | `G25` FAILED: la política de alice pasa a concederse a bob — el `decision` seguía siendo `false` por una razón distinta (la política no concede a bob), pero `policy_ids_evaluated` dejó de estar vacío: la **atribución** es lo que se rompió, y las aserciones de `policy_ids_evaluated` y del motivo de denegación implícita son las que lo detectan |
| **M2** la consulta del finder **no filtra** por `attached_principals` (mutación aplicada a la **primera** ocurrencia del literal, la del `get_effective_policies` legacy, línea 428) | **mutante equivalente**: `G24` sobrevivió. El camino IAM **no** usa esa implementación (la línea 428 pertenece a otra feature, `get_effective_policies`, que la evaluación IAM no invoca), así que la mutación no cambió el comportamiento observado. Se rehízo como **M2b** con un ancla única (el bloque completo `let mut response = self.db.query(query)`) |
| **M2b** el finder IAM **no devuelve ninguna** política (consulta que no encuentra nada) | `G24` FAILED: la evaluación deja de consultar la política persistida y el `decision` pasa a `false` con `policy_ids_evaluated` vacío — el hueco original de O6, reintroducido |
| **M3** `create` **no persiste** la atribución | `G24` FAILED en su mitad de ida y vuelta: la atribución se pierde al guardar, así que la evaluación no encuentra la política — el mismo hueco por el otro extremo |

Los tres mutantes se revirtieron tras observar la muerte de su prueba; el
adaptador quedó **byte a byte** como estaba
(`sha256=fa0c48914c37eac3f3c23061845fea8d787f19e10671b9ce96682754c754fac3`)
y la suite volvió a verde (`http_contract` 25/25, `cargo test --workspace` 806
superadas).

### 7.7 Cerrados en H1b-4 (verificados por mutación)

| Id | Defecto | Por qué era un falso verde | Cómo se corrigió | Prueba discriminante |
|---|---|---|---|---|
| O3 | `src/infrastructure/` era un módulo **no compilado**: no estaba declarado en `src/lib.rs`, así que rustc **nunca lo veía** y el `GetEffectiveScpsAdapter` que contenía **no existía en el binario** | el build estaba **verde** y el endpoint **respondía**: nada en la cadena de herramientas puede avisar de un `.rs` que no es entrada de la compilación. El defecto era invisible porque la **ausencia** del código no rompe ninguna aserción: solo el árbol mismo era la prueba, y nadie lo miraba | el módulo **se eliminó** (3 ficheros, 155 líneas) y se añadió un **guard permanente** (`tests/module_tree.rs`) que resuelve las declaraciones `mod` desde las raíces del crate y **falla** si algún `.rs` de `src/` queda fuera de la unidad de compilación. La rama «declararlo y usarlo» se descartó por **medida**: el módulo importa `hodei_organizations`, que no es dependencia del paquete raíz (§7.2) | `T1` (invariante: todo `.rs` de `src/` es alcanzable desde `lib.rs`/`main.rs`) + `T2` (**negativo**: el resolutor **sí** detecta un módulo no declarado, para que `T1` no pueda pasar en vacío) + `T3` (**negativo**: un módulo declarado dentro de un módulo inline **no** se reporta, para que el layout legítimo de Rust 2018 no rompa el guard) |

**Mutación focalizada de H1b-4** (cada mutante debe morir; los dos murieron y
se revirtieron):

| Mutante | Test que murió |
|---|---|
| **M1** el módulo `src/infrastructure/` se **recrea** (3 ficheros) sin declararlo | `T1` FAILED: el guard nombró los tres ficheros (`infrastructure/mod.rs`, `infrastructure/adapters/mod.rs`, `infrastructure/adapters/get_effective_scps_adapter.rs`) como no alcanzados — el **mismo** defecto que O3, reintroducido |
| **M2** `pub mod infrastructure;` se **declara** en `src/lib.rs` | `cargo check` FAILED con **10 errores `E0433`**: `use of unresolved module or unlinked crate hodei_organizations`. Esto no es un mutante de prueba, es la **medición** de que la rama «declararlo» de O3 es imposible: el crate no es dependencia del paquete raíz |

Ambos mutantes se revirtieron tras observar la muerte. El árbol volvió a su
estado y la suite completa volvió a verde (§15.2).

**Lo que este cierre deja fuera, a propósito.** La guardia protege **el crate
de aplicación** (`src/`), que es donde vivía O3. Al medir el invariante sobre
los demás crates del workspace se encontró **O10**: cuatro huérfanos en
`crates/hodei-iam` (§7.2). Ampliar el guard al workspace entero **fallaría
hoy** por esos cuatro ficheros, así que hacerlo sin cerrarlos sería un rojo
permanente; y cerrarlos exige una decisión de producto por fichero. Queda
**registrado con su medida**, no difuminado.

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

1. Colisión inter-tenant de Cedar UID: HRN serializado en EntityUid completo y estable. **CERRADO en H1a** — el identificador Cedar es el HRN canónico completo, construido solo con APIs Cedar tipadas; dos homónimos de cuentas distintas son entidades distintas (evidencia y mutante en §7.3).
2. Política SCP entendida como `permit` unido a IAM: límites aplicados por intersección, nunca como grant. **Abierto (H1b)**; `crates/hodei-authorizer` no se reactiva.
3. Estado mutable compartido entre decisiones Cedar concurrentes: evaluación por snapshot inmutable. **CERRADO en H1a** — el motor no conserva policies/entities mutables compartidas; cada petición evalúa contra su propio snapshot inmutable (evidencia y mutante en §7.3).
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

## 11. H1a — núcleo de identidad y decisión (ejecutado)

H1 se partió en H1a y H1b (`ADR/011-h1a-identity-and-decision-core.md`).
Esta sección recoge lo **medido** de H1a.

### 11.1 Puerta de calidad local (medida)

| Comprobación | Resultado |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS — `No issues found` |
| `cargo test --workspace --no-fail-fast` | PASS — **768 superadas, 0 fallidas, 64 ignoradas** (doctests) |
| `cargo build --release --bin hodei-artifacts-api` | PASS — binario `sha256=06d9cb97101b2afce8ef8d9bff00ad2f6e0f90284b14d6b7fd3daed7e17b6245` |

**CI remota: `SUCCESS`.** El workflow `Rust CI` remoto es la autoridad de
integración, y el run
[`40066417952`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40066417952)
terminó en **success** (`build-and-test`, único job) sobre el commit
`a645a46`, que contiene H1a. Ese run ejecuta `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo check --workspace --all-targets`, `cargo build --workspace`,
`cargo test --workspace --no-fail-fast` y `cargo nextest run --workspace
--no-fail-fast`: la misma puerta local de la tabla anterior, más la segunda
señal de `nextest`, que aísla cada test en su proceso. La evidencia local de
H1a queda **confirmada** por la autoridad de integración.

### 11.2 Regresiones permanentes promovidas

| Id | Ubicación | Garantía |
|---|---|---|
| G9–G12 | `tests/http_contract.rs` | contrato HTTP de `POST /api/v1/policies/evaluate`: `Allow` nombrando la política determinante, forbid que prevalece, default deny, HRN malformado → `400`, y el límite de cuenta |
| A1–A3 | `tests/authorization_isolation.rs` | a nivel de **puerto**: dos evaluaciones concurrentes con políticas opuestas no se contaminan; una política de una cuenta no alcanza al homónimo de otra; sin políticas, deny |

El motor conserva además, en sus tests unitarios, las propiedades del HRN v2
(alias v1 con región vacía, rechazo de tipo o id vacíos) y la unicidad del
UID con scope.

### 11.3 Aceptación efímera (ejecutada y limpiada)

Ciclo `PREPARE → EXECUTE → ASSERT → EVIDENCE → CLEANUP` sobre el **binario
release construido desde el árbol de H1a**, ejecutado desde un directorio de
paquete que contiene `config/default.toml` (que es como lo lee el binario: si
faltara, el arranque falla con `missing field 'db_type'`).

- Binario bajo prueba:
  `sha256=06d9cb97101b2afce8ef8d9bff00ad2f6e0f90284b14d6b7fd3daed7e17b6245`
- Directorio de sesión efímero y puerto efímero: `/tmp/hodei-h1a-uat/session`
- Script de aceptación: efímero, **fuera del repositorio** (`/tmp/hodei-h1a-uat/`), retirado al cerrar el ciclo

| Id | Comprobación | Resultado observado |
|---|---|---|
| A5 | el binario arranca y sirve `/health` | PASS — `status: healthy` |
| A6 | un `permit` permite y nombra la política determinante | PASS — `Allow`, `determining_policies: ["policy_0"]` |
| A7 | el `forbid` prevalece; sin políticas, deny; HRN malformado | PASS — `Deny` con `determining_policies: ["policy_1"]`; `Deny` sin políticas; **HTTP 400** |
| A8 | el homónimo de otra cuenta no alcanza; sin diagnóstico de error | PASS — `Allow` en la cuenta nombrada, `Deny` en la otra, **0** diagnósticos de nivel `error` |
| A8 | 16 peticiones concurrentes con políticas opuestas | PASS — `total: 16, contaminated: 0` |

**Limpieza verificada:** el proceso termina con `SIGTERM` y **no sobrevive
ningún proceso huérfano** del binario (`pgrep -f` → 0). El directorio de
sesión efímero es el único estado que dejó la ejecución y queda fuera del
repositorio.

**Una corrección de instrumentación que no debe confundirse con un defecto del
producto.** Las dos primeras ejecuciones de esta aceptación fallaron en A5 con
`Configuration error: missing field 'db_type'`. La causa era el **directorio
de trabajo** de la aceptación: el binario se lanzaba desde `pkg/bin`, donde no
existe `config/default.toml`, en lugar de la raíz del paquete. El producto
comporta lo que debe (se niega a arrancar sin configuración legible, P5 de
§7.1); lo que estaba mal era el arnés.

### 11.4 Lo que H1a **no** cierra

- **S-001 no se cumple por completo**: el namespace de entidad derivado de un
  tipo conocido con namespace de plataforma `Hodei::*` y el namespace de
  acción `Hodei::Action::"…"` requieren que el schema sea la autoridad y que
  se aplique a la petición (O8). Es H1b. El perfil de conformidad de H1a está
  en `SPECS/authorization-hrn.md` y delimita lo diferido.
- La intersección de límites (IAM ∧ recurso ∧ organización ∧ SCP-deny), las
  revocaciones activas y auditadas, y las superficies HTTP O2, O3, O4, O6, O7 y
  O9 son H1b. (O2 y O7 se cerraron después, dentro de H1b, en el sub-bloque
  **H1b-1**: §7.4 y §12. **O6** se cerró después, dentro de H1b, en el
  sub-bloque **H1b-3**: §7.6 y §14.)
- **TypeDB sigue `NOT_RUN`** en este entorno: `typedb` y `typedb-server` están
  ausentes. Ninguna comprobación que dependa de ellos se ha ejecutado ni se
  declara aprobada.

---

## 12. H1b-1 — el schema es real en las rutas de lectura (ejecutado)

Sub-bloque de H1b que cierra **O7** (el playground lee el schema inline) y
**O2** (`GET /api/v1/schemas/load` deja de ser un stub). Es la parte de H1b
verificable **sin credenciales ni TypeDB**: las dos rutas de lectura del
esquema, ni más.

### 12.1 Qué cambió

| Superficie | Antes | Ahora |
|---|---|---|
| `POST /api/v1/playground/evaluate` con `inline_schema` | el schema se aceptaba y se **descartaba** (warning + schema vacío); las políticas se validaban contra **nada** | el schema se decodifica con el codec `cedar-schema-fragments-v1` —el mismo de la persistencia— y las políticas se validan **contra ese schema**. Un contenido que no es un sobre se rechaza con `400` en vez de leerse como schema vacío |
| `GET /api/v1/schemas/load` | `{"status":"stub"}` | delega en el `LoadSchemaPort` cableado y devuelve `status` (`active` / `no_active_schema`), `version`, `schema_id`, `content` y el recuento de tipos declarados |

**La regla del esquema vacío, y por qué.** Un schema que no declara ni un tipo
de entidad ni una acción **no puede aprobar ni reprobar nada**: aplicarlo tal
cual haría que *toda* política que nombrase un tipo pareciera inválida. Eso es
un **falso rechazo**, no una validación. Así que el esquema se aplica cuando
declara algo y, cuando no declara nada, solo se comprueba la sintaxis — la
misma regla que ya aplica el use case `validate_policy` cuando no hay esquema
almacenado. El mutante M3 (§12.4) comprueba que esta guarda está viva: sin
ella, `G14` muere.

### 12.2 Puerta de calidad local (medida)

| Comprobación | Resultado |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS — `No issues found` |
| `cargo test --workspace --no-fail-fast` | PASS — **780 superadas, 0 fallidas, 64 ignoradas** (doctests) |
| `cargo build --release --workspace` | PASS — binario `sha256=52a69d5364882567e03691b60ae066f2da530f052d11b92f5745b2de5bb3529f` |

**CI remota: `SUCCESS`.** El workflow `Rust CI` remoto es la autoridad de
integración, y el run
[`40066417952`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40066417952)
terminó en **success** sobre el commit `a645a46`, que contiene H1a y H1b-1.
La evidencia local de la tabla anterior queda **confirmada** por la autoridad
de integración.

### 12.3 Regresiones permanentes promovidas

| Id | Ubicación | Garantía |
|---|---|---|
| G13 | `tests/http_contract.rs` | el playground **acepta** una política sobre el tipo que el schema inline declara (control: ninguna regla nueva introduce un rechazo falso) |
| G14 | `tests/http_contract.rs` | el playground **rechaza** una política sobre un tipo que el schema inline **no** declara, nombrando el tipo; y la misma política con `{}` no se reporta — el rechazo viene del schema, no del validador |
| G15 | `tests/http_contract.rs` | un `inline_schema` que no es un sobre `cedar-schema-fragments-v1` es un `400`, no un schema vacío silencioso |
| G16 | `tests/http_contract.rs` | `GET /api/v1/schemas/load` sin esquema almacenado responde `200` con `no_active_schema` y versión/contenido nulos, con un **estado declarado** (no un marcador) |
| G17 | `tests/http_contract.rs` | tras registrar el esquema IAM, `GET /api/v1/schemas/load` responde `active` con la versión y el **contenido**: un sobre `cedar-schema-fragments-v1` con fragmentos y tipos declarados |

Además, en el crate: 4 tests unitarios del cargador (sobre leído, contenido que
no es sobre, formato desconocido, sobre sin fragmentos) y 3 del validador
(tipo declarado aceptado, tipo no declarado rechazado, esquema vacío → solo
sintaxis).

### 12.4 Mutación focalizada (ejecutada, con muerte observada)

Los tres mutantes y las muertes están en §7.4. Se revirtieron después de
observarlas y la suite volvió a verde (`http_contract` 17/17).

### 12.5 Aceptación efímera (ejecutada y limpiada)

Ciclo `PREPARE → EXECUTE → ASSERT → EVIDENCE → CLEANUP` sobre el **binario
release construido desde el árbol de H1b-1**, ejecutado desde un directorio de
paquete que contiene `config/default.toml` (que es como lo lee el binario).

- Binario bajo prueba:
  `sha256=52a69d5364882567e03691b60ae066f2da530f052d11b92f5745b2de5bb3529f`
- Directorio de sesión efímero y puerto efímero: `/tmp/hodei-h1b1-uat/session`,
  puerto `39871`
- Script de aceptación: efímero, **fuera del repositorio**
  (`/tmp/hodei-h1b1-uat/`), retirado al cerrar el ciclo

| Id | Comprobación | Resultado observado |
|---|---|---|
| B1 | el binario arranca y sirve `/health` | PASS — `HTTP 200`, `status: healthy` |
| B2 | `GET /api/v1/schemas/load` con el esquema IAM registrado en el arranque | PASS — `HTTP 200`, `status: active`, `version: latest`, `schema_id: schema:latest`, **2** tipos de entidad, **6** tipos de acción, y un `content` que es un sobre `cedar-schema-fragments-v1` con **8** fragmentos. Antes de H1b-1 esta respuesta era `{"status":"stub"}` |
| B3 | playground con un sobre inline que declara `Iam::User` y una política sobre `Nope::Ghost` | PASS — `HTTP 200`, `decision: DENY`, `validation_errors: ["Policy 0: for policy \`policy0\`, unrecognized entity type \`Nope::Ghost\`"]`: el esquema inline **se está aplicando** |
| B4 | el mismo sobre con una política sobre el tipo **declarado** `Iam::User` | PASS — `HTTP 200`, `decision: ALLOW`, `validation_errors: []`: el tipo declarado no se rechaza |
| B5 | playground con un `inline_schema` que no es un sobre | PASS — `HTTP 400`, «the inline schema is not a 'cedar-schema-fragments-v1' envelope»: un contenido ilegible se **rechaza**, no se lee como esquema vacío |

**Limpieza verificada:** el proceso termina con `SIGTERM` («shut down
gracefully») y **no sobrevive ningún proceso huérfano** del binario. El
directorio de sesión efímero es el único estado que dejó la ejecución y queda
fuera del repositorio.

**Lo que esta aceptación no cubre.** El arranque de `default.toml` registra el
esquema IAM (`register_iam_on_startup = true`), así que el caso «no hay
esquema activo» no aparece aquí: lo cubre la regresión permanente `G16`, que
arranca el servicio sin registrarlo.

### 12.6 Lo que H1b-1 **no** cierra

- **O8 sigue abierto**: el motor de evaluación sigue **cargando** el schema sin
  **aplicarlo** a la petición (`crates/hodei-policies/src/features/evaluate_policies/use_case.rs:267-270`),
  y el evaluador del playground sigue decidiendo con `Authorizer::new()`, sin
  schema. H1b-1 arregla la **lectura** del schema; O8 pide que la **decisión**
  dependa de él, y eso exige que el schema sea la autoridad del namespace de
  acción (`Hodei::Action::"…"`) — es el bloque de gobernanza de H1b.
- S-001 **sigue sin cumplirse por completo**: el namespace de entidad
  `Hodei::*` y el namespace de acción `Hodei::Action::"…"` siguen pendientes
  por lo mismo.
- **TypeDB sigue `NOT_RUN`**: `typedb` y `typedb-server` están ausentes.
- O4 y O9 siguen abiertos, con su criterio de cierre en §7.2. **O6** se
  cerró después, en **H1b-3** (§7.6 y §14), y **O3** se cerró después, en
  **H1b-4** (§7.7 y §15).

---

## 13. H1b-2 — el schema gobierna la decisión del playground (ejecutado)

Sub-bloque de H1b que cierra la **mitad del playground** de **O8**: el schema
que el consumidor envía deja de ser un dato validado y pasa a ser la
**autoridad** sobre la decisión. La mitad del motor sigue abierta, con su causa
medida (§7.2).

### 13.1 Qué cambió

| Superficie | Antes | Ahora |
|---|---|---|
| verbo de la petición | el evaluador fabricaba la acción en la forma plana `Action::"X"`, sin mirar el schema | el verbo se resuelve **por id** contra las acciones que el schema declara. Un verbo que el schema no declara **deniega** nombrándolo; uno que el schema declara **dos veces** deniega nombrando **ambos** candidatos |
| namespace de la acción | lo decidía el evaluador | lo decide el **schema**: la misma política escrita contra `Hodei::Action::"read"` concede con un schema que declara ese namespace y **no concede** con uno que declara la acción plana |
| petición frente al schema | se evaluaba **sin** schema (`Request::new(..., None)`) | la petición **se construye con el schema**, para que Cedar compruebe que encaja. Una petición cuyo tipo el schema no declara **no conforms** y se **deniega con el motivo** |
| contexto de la petición | el use case lo convertía y el evaluador **lo descartaba** (`Context::empty()`) | el contexto **llega** a la evaluación: una política condicionada sobre un atributo concede o deniega según su valor |
| schema que no declara nada | se aplicaba en crudo al leer el schema | no se aplica: **no constriñe nada**. Aplicarlo en crudo sería un falso rechazo, no una validación |

**Por qué el «fail closed» es la única salida.** Un verbo que el mundo del
schema no declara **no existe**: concederlo sería inventar una autorización
sobre una acción que nadie ha declarado. Y una acción ambigua **no** se
resuelve eligiendo un candidato: Cedar permite que dos acciones *namespaced*
compartan id, así que elegir una sería evaluar la petición contra una acción
que el consumidor no pidió. En los dos casos la decisión es **denegar** y el
**motivo** viaja en el array `errors` que el endpoint ya expone — el handler no
necesitó cambios.

**El contexto es parte de la petición, no un adorno.** Sin propagarlo, un
schema que declare un tipo de contexto rechazaría la petición (falso rechazo) y
ninguna política condicionada podría coincidir nunca. Por eso el use case
conserva el contexto convertido y lo pasa al evaluador.

### 13.2 Puerta de calidad local (medida)

| Comprobación | Resultado |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS — `No issues found` |
| `cargo test --workspace --no-fail-fast` | PASS — **797 superadas, 0 fallidas, 64 ignoradas** (doctests), 20 suites |
| `cargo build --release --workspace` | PASS — binario `sha256=30b5d1cf73c33a2bb1f45696dfd4fa85594450c3e480fd25d0b7e041bd8dc163` |

**CI remota: `SUCCESS`.** El workflow `Rust CI` remoto es la autoridad de
integración, y el run
[`40066417952`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40066417952)
terminó en **success** sobre el commit `a645a46`, que contiene H1a, H1b-1 y
H1b-2. La evidencia local de la tabla anterior queda **confirmada** por la
autoridad de integración.

### 13.3 Regresiones permanentes promovidas

| Id | Ubicación | Garantía |
|---|---|---|
| G18 | `tests/http_contract.rs` | la decisión **sigue al schema**: el verbo declarado concede, el verbo **no** declarado deniega nombrándolo, y un schema que declara el tipo pero **ningún** verbo tampoco lo concede |
| G19 | `tests/http_contract.rs` | el **namespace lo decide el schema**: la misma política concede con el schema namespaced y deniega con el plano — porque entonces denota otra acción |
| G20 | `tests/http_contract.rs` | el **contexto llega a la evaluación**: con la IP coincidente concede y con otra deniega |
| G21 | `tests/http_contract.rs` | una petición cuyo **tipo de recurso no declara** el schema se **deniega** nombrando el tipo |
| G22 | `tests/http_contract.rs` | un schema que **no declara nada** no constriñe nada: la petición concede y no hay errores |
| G23 | `tests/http_contract.rs` | una acción que el schema declara **dos veces** se **deniega** nombrando **ambos** candidatos |

Además, en el crate: 7 tests unitarios de `internal/engine/authority.rs` (el
schema declara algo o no; la resolución plana, namespaced, no declarada y
ambigua) y 4 del evaluador (verbo no declarado, namespace del schema, petición
que no encaja, schema vacío). El total de la suite sube de 780 a **797**:
6 contratos + 7 unitarios de `authority.rs` + 4 del evaluador.

### 13.4 Mutación focalizada (ejecutada, con muerte observada)

Los seis mutantes y las muertes están en §7.5. Se revirtieron después de
observarlas y la suite volvió a verde (`http_contract` 23/23).

### 13.5 Aceptación efímera (ejecutada y limpiada)

Ciclo `PREPARE → EXECUTE → ASSERT → EVIDENCE → CLEANUP` sobre el **binario
release construido desde el árbol de H1b-2**, ejecutado desde un directorio de
paquete que contiene `config/default.toml` (que es como lo lee el binario).

- Binario bajo prueba:
  `sha256=30b5d1cf73c33a2bb1f45696dfd4fa85594450c3e480fd25d0b7e041bd8dc163`
- Directorio de sesión efímero y puerto efímero: `/tmp/hb2-uat/run`, puerto
  `48917`
- Script de aceptación: efímero, **fuera del repositorio**
  (`/tmp/hb2-uat/`), retirado al cerrar el ciclo

| Id | Comprobación | Resultado observado |
|---|---|---|
| U1 | schema plano, verbo `read` declarado | `ALLOW`, `errors: []` |
| U2 | schema plano, verbo `write` **no** declarado | `DENY`, `errors: ["the action \`write\` is not declared in the schema"]` |
| U3 | schema namespaced + política sobre `Hodei::Action::"read"` | `ALLOW`, `errors: []` |
| U4 | schema plano + la **misma** política namespaced | `DENY`, `errors: ["Policy 0: for policy \`policy0\`, unrecognized action \`Hodei::Action::\"read\"\`"]` |
| U5 | contexto coincidente (`ip = 10.0.0.1`) | `ALLOW`, `errors: []` |
| U6 | contexto **no** coincidente (`ip = 10.0.0.2`) | `DENY`, `errors: []` |
| U7 | recurso de un tipo que el schema no declara (`Storage::Document`) | `DENY`, `errors: ["the request does not conform to the schema: resource type \`Storage::Document\` is not declared in the schema"]` |
| U8 | schema **vacío** (`{}`) | `ALLOW`, `errors: []` — no constriñe nada |

**Limpieza verificada.** El `SIGTERM` inicial no alcanzó al binario —el `kill`
apuntó al subshell que lo había lanzado— y la **comprobación de huérfanos lo
detectó**: el proceso seguía escuchando en el puerto `48917` después de la
limpieza. Se terminó de forma explícita y se volvio a comprobar: **0 procesos
huérfanos**, puerto libre y **sin residuos** en `/tmp` (`/tmp/hb2-uat`,
`/tmp/hb2-probe` y `/tmp/hb2-mut` eliminados). Es exactamente para lo que
existe esa comprobación: un UAT que «termina en verde» y aun así deja un
proceso vivo.

**Lo que esta aceptación no cubre.** El arranque usa `register_iam_on_startup =
false` y un schema inline en cada petición: el caso «el motor aplica el
esquema **registrado**» (§13.6) no aparece aquí, y no aparece porque esa mitad
de O8 sigue bloqueada por falta de alcance de plataforma.

### 13.6 Lo que H1b-2 **no** cierra

- **La mitad del motor de O8 sigue abierta**: `evaluate_policies` sigue
  **cargando** el esquema registrado sin **aplicarlo** a la petición
  (`crates/hodei-policies/src/features/evaluate_policies/use_case.rs:267-270`).
  Su causa está medida: el esquema registrado es de alcance IAM y
  `POST /api/v1/policies/evaluate` es genérico, así que aplicarlo en crudo
  **denegaría toda petición fuera del mundo IAM** — los casos `G9`–`G12` solo
  sobreviven porque pasan `evaluation_mode: "NoSchema"`. Cerrarla exige una
  decisión de alcance de plataforma, no un cambio de código.
- S-001 **sigue sin cumplirse por completo**: el namespace de entidad
  `Hodei::*` y el namespace de acción `Hodei::Action::"ReadAsset"` siguen
  pendientes por lo mismo.
- **TypeDB sigue `NOT_RUN`**: `typedb` y `typedb-server` están ausentes.
- O4 y O9 siguen abiertos, con su criterio de cierre en §7.2. **O6** se
  cerró después, en **H1b-3** (§7.6 y §14), y **O3** se cerró después, en
  **H1b-4** (§7.7 y §15).
- **Observación colateral, no defecto corregido**: el cargador de configuración
  exige **todos** los campos cuando no existe `config/default.toml`, porque el
  `Default` derivado de `AppConfig` no lo aplica `serde` al deserializar. La
  UAT lo rodeó con un `config/default.toml` en su propio directorio de sesión.
  No se ha tocado: es un defecto preexistente fuera del alcance de este
  sub-bloque, y queda anotado aquí para que no se confunda con un cambio de
  H1b-2.

---

## 14. H1b-3 — la evaluación IAM consulta las políticas persistidas (ejecutado)

Sub-bloque de H1b que cierra **O6**: la evaluación de IAM deja de ser una
superficie decorativa y **decide sobre las políticas que están guardadas**. El
hueco real era más grande que el `TODO` que citaba la fila (§7.2), y por eso
el hito se **reshapea** antes de ejecutarse: la política persistida declara sus
principales, el finder **consulta de verdad** el mismo adaptador que escribe,
los resolvers **materializan** las entidades desde los HRNs, y la decisión se
expone por HTTP.

### 14.1 Qué cambió

| Superficie | Antes | Ahora |
|---|---|---|
| política persistida | se guardaba `{content, description}`: **nunca se guardaba a quién servía** | `create` guarda `attached_principals` (HRNs canónicos validados: servicio `iam`, tipo `User`, sin duplicados) y `get` los devuelve — la atribución **sobrevive al viaje de ida y vuelta** por la base de datos |
| consulta de políticas | el `PolicyFinderPort` lo implementaba un **stub en memoria** que no tocaba la base de datos | el `IamPolicyFinderPort` **consulta SurrealDB** (`SELECT * FROM policy WHERE $principal_hrn IN attached_principals`) sobre el **mismo adaptador que escribe**: las políticas que deciden son las que están guardadas |
| alcance de la consulta | ninguna: un conjunto vacío o un conjunto global | **atribución**: solo se consultan las políticas **adjuntas** al principal que se decide. Una política de otro principal **ni se consulta**, así que no puede conceder nada |
| entidades de la petición | los resolvers **no tenían implementación** | `HrnEntityResolver` **materializa** principal y recurso desde sus HRNs. El UID Cedar se deriva del HRN, así que la entidad identifica al principal aunque no lleve atributos |
| caso de uso | **nunca se construía** fuera de los tests | `EvaluateIamPoliciesUseCase` está **cableado** en el Composition Root y **decide** con Cedar, nombrando las **políticas determinantes** y las **políticas evaluadas** |
| superficie HTTP | **no existía** ruta de evaluación IAM | `POST /api/v1/iam/evaluate` (documentada en OpenAPI): principal, acción y recurso; un HRN malformado es `400`, no una decisión silenciosa |

**Por qué la atribución es el dato que faltaba.** Una política guardada sin
atribución no dice **a quién** sirve: consultarla para «el principal» no
tenía contenido, y devolver *toda* política convertiría cada política en un
permiso global — una política escrita para alice concedería a bob. Por eso
**la atribución se persiste, se valida y se filtra en la consulta**, y por eso
la regresión negativa (`G25`) es tan importante como la positiva: sin ella,
un fallo en el filtro se escondería detrás de una decisión correcta por
casualidad.

**La degradación, declarada y no escondida.** La plataforma no tiene todavía
perfil de identidad (no hay API que cree un principal), así que la entidad
**no lleva atributos**. La consecuencia es precisa: una política **condicionada
sobre un atributo** del principal o del recurso no puede coincidir. Una
política que restringe principal o recurso **por identidad** —que es la forma
que la atribución hace significativa— se evalúa correctamente. El resolver
lo dice en su propia documentación y hay un test que lo afirma.

### 14.2 Puerta de calidad local (medida)

| Comprobación | Resultado |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS — `No issues found` |
| `cargo test --workspace --no-fail-fast` | PASS — **806 superadas, 0 fallidas, 64 ignoradas** (doctests), 20 suites, exit 0 |
| `cargo build --release --workspace` | PASS — binario `sha256=b8689bbb040ce23fc5b9b3fd299b1aed7814da7ba3df0bab0b47d28821c91c65` |

**CI remota: `SUCCESS`.** El workflow `Rust CI` remoto es la autoridad de
integración, y el run
[`40066417952`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40066417952)
terminó en **success** sobre el commit `a645a46`, que contiene H1a, H1b-1,
H1b-2 y H1b-3. La evidencia local de la tabla anterior queda **confirmada** por
la autoridad de integración: por primera vez los cuatro sub-bloques tienen
evidencia de integración, no solo local.

**Una nota de método, no un defecto corregido.** Durante este ciclo se
midió el árbol de H1b-3 (HEAD) contra el árbol de trabajo con un `git
worktree` que **compartía el mismo `CARGO_TARGET_DIR`**. Eso no es seguro: los
miembros del workspace se referencian por ruta **relativa**, así que dos
checkouts del mismo repositorio producen el **mismo hash de unidad** y cargo
reutiliza artefactos compilados contra el otro árbol. Se observó en rojo: la
suite completa falló en `legacy_quarantine` («the drift report must exist») y
en `distribution_contract`, y `http_contract` corrió 23 pruebas en vez de 25
— binarios **obsoletos**, no fallos del código. La lección queda registrada
porque es reproducible: **un worktree necesita su propio `CARGO_TARGET_DIR`**.
Tras `cargo clean -p` de los paquetes del workspace, la suite completa volvió a
verde con 806 superadas.

El recuento de HEAD (797 superadas) es el de §13.2, medido en el ciclo de
H1b-2 sobre **el mismo árbol**: desde entonces HEAD solo añade el commit de
documentación `140480e`. El delta de **+9** se reconcilió contra el diff de
código, no contra una suposición: los atributos de prueba nuevos son 2 en el
adapter del finder y el resolver, 2 en `http_contract` (G24 y G25), 1 en el
`dto` de `create_policy`, 3 en su caso de uso y 1 en el `dto` nuevo de
`evaluate_iam_policies`. 797 + 9 = 806. Una **reejecución** de HEAD en un
target aislado se Lanzó, pero quedó bloqueada por una compilación concurrente de
otro proyecto en la misma máquina y **no se completó**: no se cuenta como
evidencia.

### 14.3 Regresiones permanentes promovidas

| Id | Ubicación | Garantía |
|---|---|---|
| G24 | `tests/http_contract.rs` | la evaluación IAM **decide sobre las políticas persistidas**: la política creada por la API y atribuida a un principal es **la que consulta** (`policy_ids_evaluated`) y **la que nombra** (`determining_policies`), y la atribución **sobrevive al viaje de ida y vuelta** por el store |
| G25 | `tests/http_contract.rs` | la atribución es un **límite**: una política guardada y atribuida a **otro** principal **ni se consulta** (`policy_ids_evaluated` vacío) y la denegación es **implícita**, no el efecto de las condiciones de una política ajena |

Además, en el crate IAM: 4 tests unitarios del finder y del resolver (finder
que solo devuelve lo adjunto, finder que **no** filtra a otro principal, y el
resolver que materializa el HRN del principal y del recurso y **declara la
degradación** de atributos), 3 tests del caso de uso de `create_policy`
(canonicalización de la atribución y los dos rechazos: HRN malformado y
principal que no es un `Iam::User`), 1 test de deserialización del comando de
`create_policy` y 1 test de deserialización del comando de evaluación (en el
`dto` nuevo). Dos de los tests del adapter anteriores
(`test_in_memory_policy_finder_with_policies` y `test_surreal_policy_finder`)
se **sustituyeron**: el segundo era el que **afirmaba** el stub — una prueba
que fijaba el defecto — y el primero cubría un caso que los nuevos cubren con
más precisión. 11 tests nuevos y 2 retirados: el delta neto es **+9**, que es
exactamente la diferencia entre las 797 superadas de HEAD y las 806 de este
árbol.

### 14.4 Mutación focalizada (ejecutada, con muerte observada)

Los mutantes y las muertes están en §7.6. Se revirtieron después de
observarlas; el adaptador quedó **byte a byte** como estaba
(`sha256=fa0c48914c37eac3f3c23061845fea8d787f19e10671b9ce96682754c754fac3`)
y la suite volvió a verde (`http_contract` 25/25, `cargo test --workspace` 806
superadas).

Lo que la campaña dejó claro, y que no era evidente antes: **`decision ==
false` no discrimina**. Con el filtro de atribución roto (M1) la decisión de
bob seguía siendo `false` — porque la política de alice no le concede —, así
que la garantía que realmente se rompió la detectan
`policy_ids_evaluated == []` y el motivo de denegación implícita, no el booleano.
Por eso `G25` afirma **las dos** cosas y no solo el `decision`.

### 14.5 Aceptación efímera (ejecutada y limpiada)

Ciclo `PREPARE → EXECUTE → ASSERT → EVIDENCE → CLEANUP` sobre el **binario
release construido desde el árbol de H1b-3**, ejecutado desde un directorio de
sesión que contiene `config/default.toml` (que es como lo lee el binario).

- Binario bajo prueba:
  `sha256=b8689bbb040ce23fc5b9b3fd299b1aed7814da7ba3df0bab0b47d28821c91c65`
- Directorio de sesión efímero y puerto efímero: `/tmp/hodei-h1b3-uat/session`,
  puerto `42841`
- Script de aceptación: efímero, **fuera del repositorio**
  (`/tmp/hodei-h1b3-uat/`), retirado al cerrar el ciclo

| Id | Comprobación | Resultado observado |
|---|---|---|
| B1 | el binario arranca y sirve `/health` | PASS — `HTTP 200`, `status: healthy` |
| U1 | `POST /api/v1/iam/policies` con `attached_principals` | PASS — `HTTP 200`, `attached_principals: ["hrn:aws:iam::account-a:User/alice"]`, `hrn: hrn:hodei:iam::default:Policy/uat-iam-policy` |
| U2 | `POST /api/v1/iam/policies/get` con ese HRN | PASS — `HTTP 200`, `attached_principals: ["hrn:aws:iam::account-a:User/alice"]`: la atribución **está en el store**, no solo en la respuesta de creación |
| U3 | `POST /api/v1/iam/evaluate` para el principal **adjunto** | PASS — `HTTP 200`, `{"decision":true,"reason":"Access granted","determining_policies":["uat-iam-policy"],"policy_ids_evaluated":["uat-iam-policy"]}` |
| U4 | `POST /api/v1/iam/evaluate` para un principal **distinto** | PASS — `HTTP 200`, `{"decision":false,"reason":"No IAM policies found for principal (implicit deny)","determining_policies":[],"policy_ids_evaluated":[]}`: la política de alice **ni se consultó** |
| U5 | `POST /api/v1/iam/evaluate` con HRN malformado | PASS — `HTTP 400`, `{"error":"Invalid principal HRN format","status":400}`: un HRN inválido es un error, no una decisión silenciosa |

**15 comprobaciones, 15 en verde** (incluidas las de forma de cada respuesta).

**Limpieza verificada:** el proceso termina con `SIGTERM` («shut down
gracefully»), la **comprobación de huérfanos no encuentra ningún proceso**
`hodei-artifacts-api` vivo y el puerto queda libre. El directorio de sesión
(`/tmp/hodei-h1b3-uat/`) se retiró al cerrar el ciclo.

**Lo que esta aceptación no cubre.** El arranque registra el esquema IAM
(`register_iam_on_startup = true`), así que la política validada contra él.
La ruta IAM **no aplica** ese schema a la petición (§14.6), y por eso esta
aceptación **no** demuestra nada sobre la aplicación del schema en la ruta
IAM: solo sobre la consulta de las políticas persistidas y la decisión que
sale de ellas.

### 14.6 Lo que H1b-3 **no** cierra

- **La mitad del motor de O8 sigue abierta**: `evaluate_policies` sigue
  **cargando** el esquema registrado sin **aplicarlo** a la petición
  (`crates/hodei-policies/src/features/evaluate_policies/use_case.rs:267-270`).
  Su causa está medida: el esquema registrado es de alcance IAM y
  `POST /api/v1/policies/evaluate` es genérico, así que aplicarlo en crudo
  **denegaría toda petición fuera del mundo IAM**. Cerrarla exige una
  decisión de alcance de plataforma, no un cambio de código (§13.6).
- **La ruta IAM sigue sin aplicar el schema, y eso es una decisión
  declarada.** `POST /api/v1/iam/evaluate` evalúa **sin** schema
  (`Request::new(..., None)`): el esquema registrado declara `Iam::User` e
  `Iam::Group`, pero la ruta acepta un recurso de cualquier mundo, así que
  aplicarlo en crudo sería un **falso rechazo** — denegaría peticiones
  legítimas de otros dominios. Es el mismo criterio que H1b-2 fijó para el
  playground (un schema que no declara lo aplicable no se aplica en crudo), y
  por eso se declara aquí en vez de dejarse implícito.
- **El `EvaluationRequest` del kernel no tiene campo `context`**
  (`crates/kernel/src/application/ports/authorization.rs:47`): el contexto de
  la petición **no viaja por el kernel**. La ruta IAM de H1b-3 no lo necesita
  (no hay contexto en su comando), pero la limitación sigue ahí y la hereda
  cualquier evaluación que se apoye en ese tipo del kernel.
- **Los resolvers no resuelven atributos**: `HrnEntityResolver` materializa
  solo el HRN (§14.1). Una política condicionada sobre un atributo **no puede
  coincidir** hasta que exista un perfil de identidad.
- **No hay revocación ni auditoría**: la atribución es una lista que se
  escribe al crear la política; no hay ciclo de vida de la atribución
  (cambiarla, retirarla, expirarla) ni rastro de quién la cambió.
- S-001 **sigue sin cumplirse por completo**: el namespace de entidad
  `Hodei::*` y el namespace de acción `Hodei::Action::"ReadAsset"` siguen
  pendientes por lo mismo.
- **TypeDB sigue `NOT_RUN`**: `typedb` y `typedb-server` están ausentes.
- O4 y O9 siguen abiertos, con su criterio de cierre en §7.2. **O3** se cerró
  después, en **H1b-4** (§7.7 y §15).
- **Observación colateral, no defecto corregido**: el cargador de configuración
  exige **todos** los campos cuando no existe `config/default.toml` (§13.6).
  La UAT lo rodeó con un `config/default.toml` en su propio directorio de
  sesión. No se ha tocado.

## 15. H1b-4 — el código que no se compila deja de existir (ejecutado)

Sub-bloque de H1b que cierra **O3**: `src/infrastructure/` **no era
infraestructura**, era un directorio de ficheros que rustc nunca veía. El
binario no contenía el `GetEffectiveScpsAdapter`; el repositorio sí contenía
su código, y esa diferencia es la que hace que un repositorio pueda parecer
más grande de lo que el producto es.

### 15.1 Qué cambió

| Superficie | Antes | Ahora |
|---|---|---|
| `src/infrastructure/` (3 ficheros, 155 líneas) | directorio de fuentes **fuera de la unidad de compilación**: `GetEffectiveScpsAdapter` y sus mocks **no existían en el binario** | **eliminado**. El árbol del crate de aplicación no contiene ningún `.rs` que no compile |
| invariante «todo `.rs` compila» | **inexistente**: rustc no puede avisar de un fichero que no es entrada de la compilación, así que el defecto era invisible para todo el tooling | **guard permanente** (`tests/module_tree.rs`): un resolutor de declaraciones `mod` desde las raíces del crate **falla** si algún fichero de `src/` queda fuera de la unidad de compilación |
| rama «declararlo y usarlo» de O3 | abierta en el enunciado de la fila | **descartada por medida**: el módulo importa `hodei_organizations`, que no es dependencia del paquete raíz (§7.2) |

**La decisión, con su comparación.** La fila de O3 ofrecía dos cierres, y la
elección no fue de gusto sino de medición:

| Opción | Veredicto | Razón |
|---|---|---|
| **Declarar** `pub mod infrastructure;` y usar el adaptador | **IMPOSIBLE** | el módulo importa `hodei_organizations`; el crate está comentado en `[workspace] members`, depende del crate legado `policies` (también comentado), y su único consumidor (`crates/hodei-authorizer`) está en cuarentena y vetado por `SPECS/authorization-hrn.md` § «Migración legado». Declararlo rompe la compilación con **10 errores `E0433`** (mutante **M2**, §7.7) |
| **Eliminar** el directorio | **ELEGIDA** | es la única lectura que no inventa evidencia, y es la que la propia §5 recommending desde el principio (`DELETE`). El código era de una rama en cuarentena y su función ya la cubre el slice vivo de IAM |
| Declarar y usar, **reactivando** los crates en cuarentena | **RECHAZADA** | contradice la especificación vigente y arrastra a H1b una decisión que le pertenece a otro ciclo |

**La prueba de que no era código: el binario.** El `sha256` del artefacto
release es **idéntico** antes y después de eliminar el módulo
(`b8689bbb040ce23fc5b9b3fd299b1aed7814da7ba3df0bab0b47d28821c91c65` en ambos
casos). Si aquellos ficheros hubieran aportado algo, el resumen lo habría
delatado. Su ausencia **es** la prueba de que el módulo no contribuyó ni con un
byte. El `dep-info` del build release lo confirma por el otro lado: el conjunto
compilado de la raíz del crate es **exactamente 14 ficheros**, y ninguno está
bajo `src/infrastructure/`.

### 15.2 Puerta de calidad local (medida)

| Comprobación | Resultado |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS — `No issues found` |
| `cargo test --workspace --no-fail-fast` | PASS — **809 superadas, 0 fallidas, 64 ignoradas** (doctests), 21 suites, exit 0 |
| `cargo build --release --workspace` | PASS — binario `sha256=b8689bbb040ce23fc5b9b3fd299b1aed7814da7ba3df0bab0b47d28821c91c65`, **idéntico** al anterior al cambio |

El delta es **+3** sobre las 806 superadas de H1b-3 (§14.2): exactamente las
tres pruebas nuevas, y ninguna suite existente cambió de recuento.

**CI remota: `SUCCESS` para H1b-4.** El run
[`40067967678`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40067967678)
(`success`, commit `d231b24`) cubre este sub-bloque: es el commit que contiene
el guard `tests/module_tree.rs` y la eliminación de `src/infrastructure/`. El
run previo [`40066417952`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40066417952)
(`success`, commit `a645a46`) cubría H1a, H1b-1, H1b-2 y H1b-3. Con ambos, los
cinco sub-bloques de H1b tienen veredicto de integración y no solo evidencia
local.

### 15.3 Regresiones permanentes promovidas

| Id | Ubicación | Garantía |
|---|---|---|
| T1 | `tests/module_tree.rs` | **todo** `.rs` de `src/` pertenece a la unidad de compilación: se alcanza desde `lib.rs`/`main.rs` siguiendo declaraciones `mod`. Un fichero no alcanzado es código que no existe como producto |
| T2 | `tests/module_tree.rs` | **negativo**: el resolutor **sí** detecta un módulo no declarado (fichero suelto y directorio con `mod.rs`), así que `T1` no puede pasar en vacío |
| T3 | `tests/module_tree.rs` | **negativo**: un fichero declarado dentro de un módulo inline (`mod inline { mod helper; }`) **no** se reporta, así que el layout legítimo de Rust 2018 no rompe el guard |

El guard es **estructural**, no una aserción sobre una ruta: no depende de
`src/infrastructure/` ni de ningún nombre concreto, así que protege el defecto
**de clase** —«código que no se compila»— y no el síntoma concreto que lo
acompañaba. Eso importa porque el mismo resolutor, aplicado al workspace
entero, encontró **O10** (§7.2): cuatro huérfanos en `crates/hodei-iam`.
importa porque el mismo resolutor, aplicado al workspace entero, encontró
**O10** (§7.2): cuatro huérfanos en `crates/hodei-iam`.

**Por qué el guard es estrecho a propósito.** `T1` cubre el **crate de
aplicación** (`src/`), que es donde vivía O3. Ampliarlo a los crates del
workspace **fallaría hoy** por los cuatro ficheros de O10, y cerrarlos exige
una decisión de producto por fichero (§7.2). Un guard que nace rojo no es un
guard: es una tarea pendiente disfrazada de garantía.

### 15.4 Mutación focalizada (ejecutada, con muerte observada)

Los mutantes y las muertes están en §7.7. Los dos murieron y se revirtieron:

| Mutante | Muerte observada |
|---|---|
| **M1** — el módulo `src/infrastructure/` se recrea (3 ficheros) sin declararlo | `T1` FAILED, nombrando los tres ficheros: el defecto original, reintroducido |
| **M2** — `pub mod infrastructure;` se declara en `src/lib.rs` | `cargo check` FAILED con **10 errores `E0433`** (`unresolved module or unlinked crate hodei_organizations`): la rama «declararlo» de O3 es **imposible**, no difícil |

**M2 no es un mutante de prueba, es una medición.** Su muerte no vale por lo
que la regresión detecta, sino por lo que **demuestra**: la fila de O3 ofrecía
dos cierres y solo uno era alcanzable.

### 15.5 Aceptación efímera (ejecutada y limpiada)

Ciclo `PREPARE → EXECUTE → ASSERT → EVIDENCE → CLEANUP` sobre el **binario
release construido desde el árbol de H1b-4**, ejecutado desde un directorio de
sesión efímero **fuera del repositorio** (`/tmp/hodei-h1b4-uat/`), retirado al
cerrar el ciclo.

- Binario bajo prueba:
  `sha256=b8689bbb040ce23fc5b9b3fd299b1aed7814da7ba3df0bab0b47d28821c91c65`
- Puerto efímero y base de datos efímera (directorio RocksDB desechable)

| Id | Comprobación | Resultado observado |
|---|---|---|
| B1 | el binario release arranca y sirve `/health` | PASS — `HTTP 200`, `{"status":"healthy","version":"0.1.0","service":"hodei-artifacts-api",...}` |
| B2 | el binario **arranca desde la raíz del paquete** | PASS — el cargador exige `config/default.toml` y el binario lo resuelve **relativo a su directorio de trabajo**; es el contrato del paquete distribuido |
| B3 | el `dep-info` del build release no contiene `src/infrastructure` | PASS — conjunto compilado de la raíz = **14 ficheros**, ninguno bajo `src/infrastructure/` |
| B4 | el `sha256` del binario no cambia al eliminar el módulo | PASS — **idéntico** antes y después (`b8689bbb…`): el módulo no aportaba ni un byte |

**Dos correcciones del propio UAT, antes de darlo por bueno.** La primera
versión del chequeo de huérfanos usaba `pgrep -f`, que casaba con el **shell
padre** (su línea de comandos contenía la cadena): un falso positivo del
chequeo, no un proceso huérfano. La segunda usaba `pgrep -x`, que **avisa** de
que un nombre de más de 15 caracteres no casa nunca — es decir, un chequeo
**estructuralmente vacío**: habría dado verde siempre. Se sustituyó por
`ps -eo pid=,comm` con el nombre truncado de Linux, y **su no-vacuidad se
probó dentro del propio ciclo**: con el servicio vivo el filtro encuentra
**1** proceso, y tras el `SIGTERM` encuentra **0**.

**Limpieza verificada:** el proceso termina con `SIGTERM`, el puerto queda
libre, el filtro de huérfanos no encuentra ningún proceso
`hodei-artifacts-api` vivo, y el directorio de sesión (`/tmp/hodei-h1b4-uat/`)
se retiró al cerrar el ciclo.

### 15.6 Lo que H1b-4 **no** cierra

- **O4 sigue abierto**: `Cargo.lock` continúa en `.gitignore` (§7.2). Su cierre
  exige una decisión del maintainer, porque el borrado del lock fue
  deliberado (commit `eb42752`).
- **O9 sigue abierto**: el CORS sigue permisivo en `src/router.rs:52`. La
  rebanada de O9 —CORS por origen— sí es separable y verificable localmente
  (el router se construye en proceso), pero es **mayor** que O3 y exige decidir
  el origen de la lista de orígenes permitidos: por eso no se quiso competir
  con O3 en el mismo ciclo.
- **O8 (mitad del motor) sigue abierto**, con su causa medida (§13.6, §14.6).
- **O10 queda abierto** (§7.2): cuatro ficheros de `crates/hodei-iam` fuera de
  la unidad de compilación. Se **registraron** con su medida y su criterio de
  cierre por fichero; no se cerraron, porque son trabajo sin cablear de un
  crate **vivo** y borrarlos destruiría trabajo que un slice futuro necesita.
- **La guarda no cubre el workspace entero**: solo el crate de aplicación.
  Cubrirlo exige cerrar O10 antes (§15.3).
- **TypeDB sigue `NOT_RUN`**: `typedb` y `typedb-server` están ausentes.
- S-001 **sigue sin cumplirse por completo**: O3 no formaba parte de S-001, y
  el namespace de entidad `Hodei::*` y el de acción `Hodei::Action::"ReadAsset"`
  siguen pendientes por lo mismo.

---

**Condición inicial (sigue vigente):** no aceptar labels de README ni el count
de tests de Makefile como evidencia; medir ejecutando cargo real y clientes
reales. El censo de este documento se obtuvo ejecutando la suite completa
(`cargo test --workspace --no-fail-fast`, exit 0) y un binario real (no un
mock), y cada corrección de §7.1 tiene un mutante que demuestra que su prueba
discrimina.
