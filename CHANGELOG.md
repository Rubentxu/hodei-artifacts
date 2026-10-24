# Changelog

Todas las mudanças relevantes de este proyecto se registran en este archivo.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y
el proyecto adopta [SemVer](https://semver.org/lang/es/).

Este proyecto está en estado **pre-GA**. Las versiones publicadas hasta la
fecha son *baselines reconciliados*: no declaran paridad con Nexus CE ni
compatibilidad de protocolo completa.

## [0.2.0] - 2026-10-24

Release del roadmap **Hodei Artifacts Graph-First** tras los hitos **H1a** y
**H1b-1**, **H1b-2**, **H1b-3** y **H1b-4**. **No es GA** y **no declara
paridad con Nexus CE**.

### Añadido

- HRN v2 tipado (`partition/service/region/account/kind/opaque-id`) con
  round-trip canónico, escapes y no reciclaje. El HRN v1 se conserva como
  **alias** (región vacía): el formato en el cable no cambia para los HRN
  existentes.
- `EntityUid` de Cedar con el **scope completo** (partición/servicio/región/
  cuenta/tipo/id), construido solo con APIs Cedar tipadas. Dos homónimos de
  cuentas distintas son **entidades distintas**.
- **Snapshot inmutable por petición**: el motor no conserva `policies` ni
  `entities` mutables compartidas entre solicitudes, así que dos peticiones
  concurrentes con políticas opuestas no se contaminan.
- `POST /api/v1/policies/evaluate` **real**: invoca el puerto cableado y
  devuelve `Allow`/`Deny` con razones, políticas determinantes y
  diagnósticos. `forbid` gana a `permit` y el default es deny.
- `GET /api/v1/schemas/load` **real**: estado (`active` / `no_active_schema`),
  versión, identificador y contenido del esquema activo.
- `POST /api/v1/playground/evaluate` gobernado por el **esquema que el
  consumidor envía**: el verbo se resuelve por `id` contra las acciones
  declaradas, el **namespace lo decide el esquema** y no la petición, el
  contexto llega a la evaluación y una petición que no encaja se deniega
  nombrando el motivo.
- `POST /api/v1/iam/policies` y `POST /api/v1/iam/policies/get`: la política
  persistida **declara sus principales** (`attached_principals`) y la
  atribución sobrevive al viaje de ida y vuelta por la base de datos.
- `POST /api/v1/iam/evaluate`: la evaluación IAM **consulta de verdad** las
  políticas guardadas, y solo las que están atribuidas al principal que se
  decide.
- `tests/module_tree.rs`: guard estructural que resuelve las declaraciones
  `mod` desde las raíces del crate y **falla** si algún `.rs` de `src/` queda
  fuera de la unidad de compilación. Es un invariante, no una aserción sobre
  una ruta, y lleva dos casos negativos que demuestran su poder discriminante.
- `docs/roadmaps/hodei-artifacts-graph-first/`: perfiles de conformidad
  `H1a`/`H1b-1`/`H1b-2`/`H1b-3` en `SPECS/authorization-hrn.md`,
  `ADR/011-h1a-identity-and-decision-core.md`, y las secciones medidas
  `BASELINE.md` §7.3–§7.7 y §9–§15.

### Corregido

- **O1**: `POST /api/v1/policies/evaluate` devolvía un stub fijo.
- **O2**: `GET /api/v1/schemas/load` devolvía `{"status":"stub"}`.
- **O5**: `ActionTrait::action_name()` producía `Iam::Action::"X"` mientras
  que el motor construía `Action::"X"`. Ahora las tres rutas que construyen
  el UID de acción producen la misma forma, y coincide con la del esquema
  IAM registrado.
- **O6**: la evaluación IAM no consultaba las políticas persistidas. El hueco
  real eran **cinco capas** (finder stub sin base de datos, caso de uso nunca
  construido, puerto no cableado, sin ruta HTTP, resolvers sin implementación)
  más una sexta: el registro persistido **no guardaba atribución**, así que
  una política guardada no decía a quién servía.
- **O7**: el `inline_schema` del playground se **descartaba**. Ahora se
  decodifica con el mismo codec de persistencia (`cedar-schema-fragments-v1`)
  y se aplica: una política sobre un tipo declarado se acepta y sobre un tipo
  no declarado se reporta. Un `inline_schema` que no es un sobre se rechaza
  con `400` en vez de leerse como esquema vacío.
- **O3**: `src/infrastructure/` era un directorio de fuentes que **rustc nunca
  compilaba**, así que el `GetEffectiveScpsAdapter` que contenía no existía
  en el binario. Eliminado (3 ficheros, 155 líneas). La rama alternativa
  —declararlo y usarlo— se descartó por medida, no por gusto: el módulo
  importa `hodei_organizations`, que no es dependencia del paquete raíz, y
  cuyo crate está en cuarentena junto con su único consumidor.
- Los falsos verdes del esquema en el playground: el **namespace** lo decidía
  la petición, el **contexto se descartaba**, un verbo no declarado caía a la
  forma plana y una acción ambigua se resolvía al primer candidato. Ahora
  cada caso deniega con su motivo.
- El **estado mutable compartido** entre decisiones concurrentes (riesgo P0
  #3) queda cerrado por el snapshot inmutable por petición.

### Verificación

- Puerta de calidad medida sobre el árbol publicado: `cargo fmt --all --
  --check` (PASS), `cargo clippy --workspace --all-targets -- -D warnings`
  (PASS, `No issues found`), `cargo test --workspace --no-fail-fast` (PASS) —
  **809 pruebas superadas, 0 fallidas, 64 ignoradas (sólo doctests)**,
  21 suites — y `cargo build --release --workspace` (PASS, binario
  `sha256=6c1d971117fdff6f8718f040a154d3ae89987e1beabe15c5fa12ba08e6ce8d2b`).
- Cada corrección está comprobada por **mutación con muerte observada**:
  el UID de Cedar sin scope, el snapshot compartido, el esquema inline
  descartado, el esquema vacío aplicado en crudo, el handler de carga
  devuelto al stub, el namespace tomado de la petición, el contexto
  descartado, el verbo no declarado, la acción ambigua, el finder que
  devuelve toda política, el finder IAM que no devuelve ninguna, la
  atribución no persistida, el módulo huérfano recreado y el módulo declarado.
- La aceptación efímera se ejecuta sobre la **distribución desempaquetada**,
  desde el directorio del paquete y no desde el árbol del repositorio.
- La **CI remota** es la autoridad de integración: run
  [`40067967678`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40067967678)
  (`success`, commit `d231b24`, el que contiene H1b-4) y run
  [`40068287455`](https://github.com/Rubentxu/hodei-artifacts/actions/runs/40068287455)
  (`success`, commit `bf43f5b`). El run del commit de este release se cita en
  el `release-manifest.json` del propio release.

### Limitaciones conocidas

- `Cargo.lock` está en `.gitignore`, por lo que la clave de caché de la CI es
  constante y la caché nunca se invalida (O4). El binario publicado **no es
  reproducible bit a bit entre hosts**: el `SHA256SUMS` del release identifica
  el artefacto exacto que se publicó, no una reconstrucción garantizada. El
  borrado del `Cargo.lock` fue deliberado y su reversión exige una decisión
  del maintainer.
- CORS permisivo en `src/router.rs:52` (O9). La rebanada de O9 —CORS por
  origen— sí es separable y verificable localmente, pero exige decidir el
  origen de la lista de orígenes permitidos.
- La **mitad del motor de O8** sigue abierta: el motor carga el esquema
  registrado, que es de alcance IAM, mientras que
  `POST /api/v1/policies/evaluate` es genérico. Aplicarlo en crudo denegaría
  toda petición fuera del mundo IAM, así que cerrarla exige una decisión de
  alcance de plataforma, no un cambio de código. Por la misma razón, la ruta
  IAM **no aplica** el esquema.
- La **gobernanza** de H1b sigue abierta: SCP como intersección de límites
  (riesgo P0 #2), revocaciones activas y auditadas, credenciales reales.
- **O10**: cuatro ficheros de `crates/hodei-iam` quedan fuera de la unidad de
  compilación (`features/add_user_to_group/factories.rs`,
  `features/add_user_to_group/mocks.rs`,
  `features/create_policy/validation_port.rs` e
  `internal/domain/events.rs`). Son trabajo sin cablear de un crate vivo, con
  criterio de cierre **por fichero** registrado en `BASELINE.md` §7.2. El
  guard `tests/module_tree.rs` protege solo el crate de aplicación.
- S-001 **sigue sin cumplirse por completo**: el namespace de entidad
  `Hodei::*` y el de acción `Hodei::Action::"ReadAsset"` siguen pendientes.
- `docker`, `typedb` y `typedb-server` no están disponibles en el host de
  publicación: las comprobaciones que los requieren quedan `NOT_RUN`, no
  `PASS`. No existe todavía esquema TypeQL.

## [0.1.0] - 2026-10-24

Primer release del baseline reconciliado tras el hito **H0** del roadmap
Graph-First. **No es GA** y **no declara paridad con Nexus CE**.

### Añadido

- `schema_codec` (`cedar-schema-fragments-v1`): el esquema Cedar se serializa
  como `SchemaFragment` en JSON, que reconstruye el esquema sin pérdida. El
  esquema persistido ya no se guarda con su representación `Debug`.
- `tests/binary_contract.rs`: contrato a nivel de proceso sobre el binario
  real. P1/P2 arrancan el binario, lo conectan por socket y comprueban que
  sirve `/health` con la configuración tomada del entorno. P3 (negativo)
  comprueba que una configuración inválida produce código de salida distinto
  de cero y un mensaje explícito.
- `tests/bootstrap_integration.rs` (B1–B7): sustituye al fichero de test de
  1 byte que no contenía ninguna prueba. Incluye los casos negativos B3, B4 y
  B7.
- `tests/schema_persistence.rs` (S1–S3): el esquema persistido tiene que
  volver a ser encontrado por el lector que lo aplica. S3 es la prueba
  discriminante: una política sobre una acción declarada es válida y una
  política sobre una acción o un tipo de entidad no declarados es rechazada.
- `tests/distribution_contract.rs` (P4–P5): el binario desempaquetado arranca
  solo con la configuración que se distribuye y sirve `/health` (P4); sin esa
  configuración se niega a arrancar con un error explícito (P5, negativo).
- `config/default.toml`: configuración base que el binario lee realmente
  (`config/default.toml`), frente a `config/development.toml` y
  `config/release.toml`, que son perfiles por modo de ejecución.
- `scripts/package-release.sh`: ensambla la distribución de forma
  reproducible y **se niega a construir** un paquete sin la configuración que
  el binario lee.
- `CHANGELOG.md`: este fichero.
- `docs/roadmaps/hodei-artifacts-graph-first/BASELINE.md`: censo medido del
  repositorio, clasificación por crate, defectos corregidos (D1–D6) y defectos
  abiertos (O1–O9) con referencia `file:line`.
- `docs/roadmaps/hodei-artifacts-graph-first/`: paquete de roadmap completo
  (specs, ADRs, `ROADMAP.md`, `RELEASE.md`, `TRACEABILITY.md`,
  `compatibility-matrix.csv`).

### Corregido

- La precedencia `HODEI_<SECTION>__<KEY>` documentada era **inerte**: config-rs
  0.14.1 buscaba el prefijo como `HODEI__` al no declarar el separador de
  prefijo, por lo que toda variable de entorno se descartaba en silencio y el
  servicio arrancaba siempre con los valores del fichero TOML. Corregido con
  `.prefix_separator("_").separator("__")` y `try_parsing(true)`.
- El esquema Cedar se reconstruía **vacío** en cada carga, de modo que un
  contenido almacenado que no era un esquema se aceptaba y se descartaba. Ahora
  la carga devuelve un error explícito y el esquema almacenado se aplica de
  verdad al validar políticas.
- Las suites `tests/e2e/` y `tests/integration/` **nunca se compilaron**: el
  paquete raíz sólo publicaba el binario, así que sus rutas `hodei_artifacts_api::…`
  no existían. La suite era verde sin comprobar nada. Las suites heredadas
  quedan en cuarentena en `tests/legacy/` (renombradas, contenido intacto) y
  `tests/legacy_quarantine.rs` documenta y vigila esa decisión.
- `tests/test_register_iam_schema_integration.rs` era un fichero de 1 byte (un
  salto de línea) que Cargo compilaba como un binario de test sin ninguna
  prueba.
- `Makefile` y `.github/workflows/rust-ci.yml` se reconciliaron con los
  miembros reales del workspace, de modo que ninguna puerta puede reportar
  verde sobre un crate que no se compila.
- El esquema Cedar persistido era **invisible para el lector** cuando el
  registro no nombraba versión: se guardaba con `version: null` y el lector
  selecciona el registro por su versión, así que el esquema quedaba en la
  base y ningún consumidor lo encontraba. La validación de políticas
  **degradaba a una comprobación de sintaxis** y el servicio aceptaba
  políticas sobre acciones y tipos de entidad no declarados. El registro
  guarda ahora su versión como id, con el id `latest` para los que no la
  nombran.
- La distribución publicada **no arrancaba**: incluía `config/example.toml`,
  un fichero que el binario nunca lee, y la configuración se deserializa de
  forma estricta. La aceptación efímera anterior no lo detectó porque se
  había ejecutado desde la raíz del repositorio, donde sí hay un fichero de
  configuración legible.

### Verificación

- Puerta de calidad medida sobre el árbol publicado: `cargo metadata` (0),
  `cargo check --workspace --all-targets` (0), `cargo fmt --all -- --check` (0),
  `cargo clippy --workspace --all-targets -- -D warnings` (0),
  `cargo test --workspace --no-fail-fast` (0) — **746 pruebas superadas, 0
  fallidas, 64 ignoradas (sólo doctests)**.
- Cada corrección está comprobada por mutación: el validador con esquema
  vacío, el cargador con esquema vacío, el `prefix_separator` ausente, el
  error de configuración con salida 0, el esquema guardado sin versión, la
  configuración distribuida ausente y la configuración opcional hacen fallar
  un test nombrado.
- La aceptación efímera se ejecuta sobre la **distribución desempaquetada**,
  desde el directorio del paquete y no desde el árbol del repositorio.
- El almacén RocksDB existente se sondeó sobre una copia y **no contiene datos
  de usuario** (`INFO FOR ROOT` no declara ningún namespace), por lo que no
  hubo que exportar nada y no se destruyó nada.

### Limitaciones conocidas

- `POST /api/v1/policies/evaluate` devuelve un stub fijo y
  `GET /api/v1/schemas/load` devuelve `{"status":"stub"}` (defectos O1 y O2).
- `src/infrastructure/` es un módulo que no se compila (O3).
- `Cargo.lock` está en `.gitignore`, por lo que la clave de caché de la CI es
  constante y la caché nunca se invalida (O4).
- `ActionTrait::action_name()` produce `Iam::Action::"X"` mientras que el
  motor de evaluación construye `Action::"X"` (O5).
- CORS permisivo en `src/router.rs` (O9).
- `cargo-nextest`, `docker`, `typedb` y `typedb-server` no están disponibles
  en el host de publicación: las comprobaciones que los requieren quedan
  `NOT_RUN`, no `PASS`.
- No existe todavía esquema TypeQL: TypeDB es trabajo de H1 en adelante.
- `Cargo.lock` no se versiona (O4), de modo que el binario publicado no es
  reproducible bit a bit entre hosts: el `SHA256SUMS` del release identifica
  el artefacto exacto que se publicó, no una reconstrucción garantizada.

[0.2.0]: https://github.com/Rubentxu/hodei-artifacts/releases/tag/v0.2.0
[0.1.0]: https://github.com/Rubentxu/hodei-artifacts/releases/tag/v0.1.0
