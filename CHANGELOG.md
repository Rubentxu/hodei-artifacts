# Changelog

Todas las mudanças relevantes de este proyecto se registran en este archivo.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y
el proyecto adopta [SemVer](https://semver.org/lang/es/).

Este proyecto está en estado **pre-GA**. Las versiones publicadas hasta la
fecha son *baselines reconciliados*: no declaran paridad con Nexus CE ni
compatibilidad de protocolo completa.

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

[0.1.0]: https://github.com/Rubentxu/hodei-artifacts/releases/tag/v0.1.0
