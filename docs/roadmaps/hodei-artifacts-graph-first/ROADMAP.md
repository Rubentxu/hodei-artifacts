# ROADMAP — hitos largos, valor entregado y releases en remoto

**Fuente de verdad nueva:** mantener este archivo como plan vivo, enlazado desde `docs/prd/8-roadmap.md` (histórico). Los hitos son secuenciales para integración; dentro se admite paralelismo tras congelar contratos. Estado inicial: **TODOS PENDING**. Ninguna fecha fija ni tag reservado. **Cada Hx termina en release Git remota y evidencia de consumidor**, o queda `BLOCKED` sin fingir cierre. No pasar a H(x+1) sin reparar/revertir fallos del anterior; bloques largos de entrega, no infinitos tickets de infraestructura. Matriz `Nexus CE` se incrementa por formato sin declarar paridad total.

## H0 — Reconciliar repositorio y sanear puerta de calidad
**Valor:** ejecutar de forma reproducible código y CLI activos, sin tests falsamente verdes ni configuración legado rota.

- [ ] Inventariar estado remoto/branches/tags/issues, HEAD y working tree; mapear `Cargo.toml` vs crates; ejecutar `cargo metadata`, `cargo check`, `cargo fmt`, `clippy`, runner real. Registrar `NOT_RUN` con causa en herramientas ausentes; no tomar contador de Makefile como dato.
- [ ] Clasificar cada crate/feature: KEEP/PORT/QUARANTINE/REPLACE/DELETE y consumidores; mapa de importación a Rust 2024 actual; aislar o adaptar todo uso roto de `shared`, `mongodb`, `aws-*` no declarados sin habilitar builds ficticios.
- [ ] Corregir `Makefile` que llama crates fuera de workspace; conciliar `.github/workflows/rust-ci.yml` (`-p policies`), sin añadir segundo CI; mantener autoridad local existente.
- [ ] Exportar datos existentes de Surreal/RocksDB si se necesitan; manifest de cambios y rollback; no destruir datos ni credenciales.
- [ ] Corregir falsos verdes en tests existentes que capturan error y solo hacen `println!`; tener health real y configuración mínima arrancable o aislar ruta rota explícitamente; proteger comportamiento del API legacy si existe consumidor.
- [ ] Integrar enlace en roadmap histórico, documentar baseline y licencia EPL/MIT/CE.
**Aceptación efímera:** arranque desde build/distribuible actual + endpoint health y fallo por config inválida; un negativo de runner que demuestra exit!=0. **Permanente:** compilation/CLI health pequeño, parser/config e invariantes. **AAT:** procesos/recursos limpios. **Release:** baseline recuperable, no etiqueta GA ni claims Nexus.
**Docs:** `BASELINE.md`, `TESTING.md`, `TRACEABILITY.md`; existente `docs/testing-organization.md`, `docs/prd/8-roadmap.md`.
**Equipos:** auditor código y build, auditor docs/contratos y testing (superficies separadas); integrador único root Cargo/Makefile/release.

## H1 — Identidad HRN v2, Cedar y gobernanza multitenant
**Valor:** gestión de recursos y permisos correcta entre organizaciones, API IAM y playground vigente.

### H1a — Núcleo de identidad y decisión (unidad entregable e independiente)
**Valor:** dos recursos con el mismo nombre, en cuentas distintas, se deciden de forma distinta y aislada, y el consumidor puede pedir esa decisión por HTTP. Es el mínimo que hace verificable la garantía P0 de aislamiento.

**Estado (2026-10-24): H1a CERRADA, con evidencia local; CI remota pendiente de publicación.** Los seis puntos de H1a están implementados y verificados: puerta de calidad local en verde (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace` con 768 superadas y 0 fallidas), dos mutantes que mueren (UID Cedar sin scope → `G12`+`A2` FAILED; snapshot compartido → `A1` FAILED) y aceptación efímera A5–A8 en verde sobre el binario release `sha256=06d9cb97…`, con limpieza verificada (SIGTERM, 0 procesos huérfanos). La **CI remota sigue siendo la autoridad de integración** y queda `PENDING`: no hay push sin autorización, así que todavía no existe un run que citar. Detalle medido en `BASELINE.md` §7.3, §9 y §11.

- [x] HRN v2 tipado con partition/service/region/account/kind/opaque-id, round-trip canónico, escapes, no reciclaje; el HRN v1 se conserva como **alias** (región vacía) y el formato en el cable no cambia para los HRN existentes.
- [x] Cedar `EntityUid` con el **scope completo** (partición/servicio/región/cuenta/tipo/id) → sin colisión intertenant; construcción solo con APIs Cedar tipadas, nunca interpolando texto de HRN.
- [x] **Snapshot inmutable por petición**: el motor no conserva policies/entities mutables compartidas entre solicitudes (cierra el riesgo P0 #3 de state bleed).
- [x] `POST /api/v1/policies/evaluate` **real** (cierra O1): invoca el puerto ya cableado y devuelve Allow/Deny con razones, políticas determinantes y diagnósticos; `forbid` gana a `permit` y el default es deny (fail closed).
- [x] Una **única forma del UID de acción** en las tres rutas que lo construyen (motor, playground, `ActionTrait::action_name()`), para que el UID de acción coincida con el del esquema IAM (cierra O5).
- [x] Aislamiento verificable de punta a punta sobre el artefacto distribuido: mismo nombre en dos cuentas sin cruce; dos peticiones concurrentes con políticas opuestas no se contaminan.
**Aceptación efímera:** mismo nombre `alice` en dos cuentas sin cruce; `forbid` gana a `permit`; HRN malformado → 400; dos requests concurrentes opuestos. **Permanente:** propiedades de HRN v2 + alias v1, UID Cedar con scope por cuenta, contrato HTTP de `evaluate` (Allow/Deny/400) y no contaminación entre peticiones concurrentes. **Release:** `POST /api/v1/policies/evaluate` usable sin exposición cross tenant.
**Docs:** `SPECS/authorization-hrn.md` (perfil H1a), `ADR/003-cedar-hrn.md`, `ADR/011-h1a-identity-and-decision-core.md`, `BASELINE.md` §7.3/§9/§11.

### H1b — Gobernanza, esquema como autoridad y superficies pendientes (H1 original, diferido)
**Valor:** la decisión pasa a depender de un esquema activo, de límites de tenant y de credenciales reales.

- [ ] Cedar **compilado/validado con schema** (no solo schema cargado): el schema se aplica a la petición y pasa a ser la **autoridad** del namespace de acciones → `Hodei::Action::"ReadAsset"` (S-001) (cierra O8).
- [ ] Separar autenticación, principal efectivo, recursos y contexto confiable; eliminaciones de `"system"` en casos de uso futuros; invalidación de credenciales.
- [ ] IAM permit + resource permit, deny explícito, tenant boundary y SCP como intersección de límites (jamás SCP-`permit` como grant); fail closed en timeout/error; revocaciones activas y auditadas.
- [ ] Superficies que H1a deja explícitamente abiertas: `GET /api/v1/schemas/load` real (O2), cargador de schema del playground real (O7), CORS por origen configurado (O9), `Cargo.lock` versionado (O4), `src/infrastructure` declarado o eliminado (O3), adaptador de consulta IAM (O6).
- [ ] Reactivar `hodei-organizations` y `hodei-authorizer` solo por nuevas interfaces seguras y sin romper componentes activos; migrar SurrealDB en slice relevante cuando TypeDB H2 disponible o impl temporal sin pérdida de datos verificable. **Dependencia:** no declarar persistencia TypeDB real hasta H2; H1 entrega autorización en memoria/adaptador aislado + contrato y API donde ya persista seguro.
**Aceptación efímera:** SCP solo-deny + error de esquema; revocación activa; error de motor → deny. **Permanente:** contrato de schema aplicado y de intersección de límites. **Release:** IAM/Policy Playground gobernado.
**Docs:** `SPECS/authorization-hrn.md`, `ADR/003-cedar-hrn.md`, `docs/cedar-policies-manual.md`, `docs/arquitectura/ARQUITECTURA-FINAL-CORRECTA.md`.
**Equipos:** HRN+serialización y Cedar+policies en paralelo tras congelar identidad; un dueño del `kernel` evita colisiones.

**Por qué H1 se parte en H1a/H1b (racional trazable, ciclo 2026-10-24).** H1 mezclaba cuatro cosas con dependencias distintas: identidad, motor de decisión, gobernanza (SCP/tenant/revocación) y superficies HTTP pendientes. Comparadas tres formas viables de cerrar el hito: **(A)** cerrar H1 entero en un ciclo — descartada: parte de la gobernanza depende de credenciales reales y de TypeDB (H2), y entregarla a medias deja un estado indistinguible de "hecho"; **(B)** cerrar solo la identidad (HRN v2 + UID con scope) — descartada por insuficiente: el núcleo de decisión seguiría siendo un stub HTTP (`O1`), así que la garantía de aislamiento no sería observable por el consumidor que la pide, y el estado mutable compartido (`P0` #3) seguiría invalidando cualquier afirmación de concurrencia; **(C)** cerrar identidad **y** núcleo de decisión juntos (elegida): es el único corte que deja una garantía P0 verificable de extremo a extremo por la vía que el consumidor usa, y elimina de paso el state bleed que falsearía la prueba de concurrencia. Consecuencia asumida: el objetivo de S-001 (`Hodei::Action::"ReadAsset"`) **no** entra en H1a, porque un namespace de acción derivado de un tipo conocido exige que el schema sea la autoridad y que se aplique a la petición (`O8`); esas dos cosas caen juntas en H1b. En H1a las tres rutas que construyen el UID de acción se unifican en la forma plana `Action::"X"`, que es la que el schema IAM registrado ya usa. Detalle en `ADR/011-h1a-identity-and-decision-core.md`.

## H2 — TypeDB Knowledge Kernel + Raw Hosted + CAS transaccional
**Valor:** usuario crea repositorio Raw, sube y descarga bytes reales con identidad/ACL/integridad; grafo tipado consultable.

- [ ] Introducir driver TypeDB 3 pinneado tras validar versión; `schema` PERA versionado en migraciones, `@key`, cardinalidades, relaciones entre Tenant/Repository/PackageVersion/Asset/Blob/Actor; tests de migración semántica. Sin Surreal/PostgreSQL.
- [ ] Modelo funcional de `RepositoryKind`, `BlobState`, `AssetState` y comandos/eventos; separar `decide/evolve` y efectos externos; reemplazar acoplamiento `kernel` a Cedar/Surreal en tipos puros.
- [ ] CAS FS inicial: temp/stream/digest/promoción metadatos TypeDB/visibilidad, crash-reconcile, `HEAD`, `GET`, `PUT`, `DELETE` según política, ETag/checksum, Range si habilitado, streaming con backpressure y limites. Clave CAS por digest sin bypass de autorización.
- [ ] Control de concurrencia de publicación, idempotencia, cuotas básicas y rutas protegidas frente a traversal. Protección contra blobs huérfanos y GC temprana incorrecta.
- [ ] Índice de resolución por `(tenant,repo,asset-path)` dentro TypeDB y caché Moka con política fail-closed cuando afecta a autorización; un test de cuello de botella vs umbrales medidos.
- [ ] Entidades derivadas/índices en TypeDB con actualizaciones atómicas donde proceda; evento audit y cola de tareas durables tipo TypeDB con ACK idempotente y lease reconciliable **solo si requerida por la vertical**, no Event Store universal.
**Aceptación efímera:** curl Raw con streaming grande, checksum byte a byte, crash entre CAS y metadata, GET denegado cross-tenant, read-after-write. **Permanente:** invariantes CAS, integración TypeDB + FS, transacción de publicación y recuperación. **AAT:** p95/RSS y no OOM en archivo grande. **Release:** registry Raw probado desde binario empaquetado.
**Docs:** `SPECS/ontology.md`, `SPECS/storage-and-runtime.md`, `SPECS/functional-architecture.md`, `ADR/001-typedb-only.md`, `ADR/002-functional-hexagonal.md`, `ADR/004-cas-transactions.md`.
**Equipos:** modelo TypeDB/DB e implementación FS/CAS independientes tras contratos; integrador hace Raw HTTP.

## H3 — Maven 2 Hosted / Proxy / Group: compatibilidad observable Nexus
**Valor:** `mvn`/`gradle` config estándar trabajan con Hodei; sin plugin propietario.

- [ ] Capturar baseline frente a Nexus CE versión pinneada: workflows `mvn dependency:get`, `deploy`, `verify`, curl HEAD/GET/PUT, HTTP status/headers, metadatos, concurrencia; respetar límites legales, no traducir código Java automáticamente.
- [ ] Hosted Maven: GAV/classifier/extension, POM, checksum/ETag, snapshot timestamped, metadata, redeploy policy, path encoding y XML seguro; no materializar archivo entero.
- [ ] Proxy Maven: GET/HEAD, autenticación remota secreta, TTL/revalidación/negative cache; singleflight por clave; fallback a upstream y propagación correcta de errores.
- [ ] Group: orden determinista, prohibición de ciclos, independencia tenant, restricción de miembros, invalidación segura de caché; misma semántica visible en todas las vías.
- [ ] Inventario en TypeDB de los POM, paquetes, assets, upstream origin y referencias; no construir grafo resuelto inventando lo que no se observó.
**Aceptación efímera:** `mvn -B` proyecto `example` descarga y usa dependencias; deploy/re-resolve; proxy origin off & miss/hit; grupo con precedencia; 403/404 diferenciados según política. **Permanente:** data-driven paths/snapshots/metadata/headers, integración selectiva proxy; pruebas de regreso de bug. **Release:** cliente Maven/Gradle real, matriz de paridad por caso.
**Docs:** `SPECS/protocol-compatibility.md`, `SPECS/storage-and-runtime.md`, `ADR/005-nexus-contracts.md`.
**Equipos:** Hosted / Proxy / Group en paralelo tras fijar `MavenAssetKey` y protocolo/persistencia.

## H4 — OCI / Docker Registry y resiliencia de distribución
**Valor:** `podman pull/push` real para imágenes OCI, multiplataforma y layers.

- [ ] OCI Distribution API v2/OCI Distribution Spec: `/v2/`, blobs, uploads resumibles, digest, manifest/index y tags mutables con actualización atómica; `HEAD`, `Range`, headers y auth challenge correctos.
- [ ] Layers por CAS y referencias semánticas, GC seguro (tag → manifest → child manifest/config/layers); concurrencia de push y digest mismatch, media types, invariantes manifiesto y distribución de imagen privada.
- [ ] Objetos FS/S3 vía puerto de storage, proteger contra URLs presignadas que bypass Cedar; CDN solo para bytes autorizados; operación remota tras autorización.
- [ ] Backup TypeDB y CAS, export/import/restore, reconciliación y métricas iniciales; no prometer HA TypeDB CE.
**Aceptación efímera:** `podman push/pull` imagen multifase sobre registro fresco, 401/403 y digest incorrecto, interrupción de upload/reanudación; restaurar una imagen. **Permanente:** OCI schema/digest/retención y recuperación de referencias. **AAT:** throughput con blob grande, pico RSS y limpieza. **Release:** OCI registry utilizable en CI.
**Docs:** `SPECS/protocol-compatibility.md`, `SPECS/storage-and-runtime.md`, `SPECS/operations.md`, `ADR/006-performance-path.md`.
**Equipos:** OCI adapter / reconciler / infra S3 en paralelo, después integración.

## H5 — Supply-chain Graph: SBOM, evidencias, procedencia y firmas
**Valor:** subir SBOM SPDX/CycloneDX y consultar dependencias reales por artefacto/versión.

- [ ] Modelo semántico normalizado PURL ≠ HRN ≠ digest; desambiguación de versiones, rutas y contextos de resolución.
- [ ] Ingestar CycloneDX y SPDX (elegir revisiones soportadas explícitas), conservar documento original inmutable en CAS, firmas/evidence/fecha/fuente. Afirmaciones no se convierten en hechos automáticamente.
- [ ] Relaciones tipadas `described-by`, `depends-on`, `built-from`, `attests`, role participants; walk inverso y transitivo acotado con protección anti-ciclos, no loops sin coste.
- [ ] Deduplicación idempotente con clave (document digest + parser version + tenant scope); reingesta; invalidación de proyecciones semánticas.
- [ ] UI/API de árbol y trazabilidad sin exponer grafo intertenant.
**Aceptación efímera:** subir SBOM pequeño y otro con ciclos/transitivas, comprobar afectación inversa y permisos; invalid SBOM/identifier spoof. **Permanente:** propiedades de identidad/procedencia/relaciones, TypeDB integration con queries reales. **Release:** endpoint de conocimiento consultable.
**Docs:** `SPECS/ontology.md`, `SPECS/supply-chain-security.md`, `ADR/007-evidence-graph.md`.
**Equipos:** parser CycloneDX / parser SPDX / consultas TypeQL y API, contratos de evidencia fijados por arquitecto.

## H6 — Vulnerabilidades externas, VEX, política inteligente y cuarentena
**Valor:** correlación demostrable de advisories/vulnerabilidades, ver riesgo e impedir promociones inseguras.

- [ ] Conectores OSV/GHSA/NVD/KEV/EPSS con origen, licencia/ratelimit, freshness, revisión de feed, aliases comprobados; importar Trivy/Grype y resultados escáner sin ejecutar inputs inseguros.
- [ ] `Finding` vs `Advisory` vs `Vulnerability` vs `Assessment`; VEX/CSAF con estado afectación y justificación; corrección de hallazgos sin eliminar su historia, riesgo calculado identificando versión de algoritmo.
- [ ] Impact analysis incremental o queries delimitadas en TypeDB; límite de complejidad; dashboard/paginación.
- [ ] Cedar policy on Promotion/Download con snapshot de evidencia confiable y freshness obligatoria; revocación/fail-closed; excepción temporal versionada y auditada.
**Aceptación efímera:** mock feed controlado + 2 proyectos comparten paquete afectado; alias conflictivo no se fusiona; advisory corregido; policy denies a vulnerabilidad declarada; tiempo de feed a índice. **Permanente:** integridad correlación, test negativo policy y reintentos. **Release:** riesgo visible y gate de promoción seguro.
**Docs:** `SPECS/supply-chain-security.md`, `SPECS/authorization-hrn.md`, `ADR/003-cedar-hrn.md`, `ADR/007-evidence-graph.md`.
**Equipos:** conectores feeds distintos vs motor impacto vs políticas, integración unida por tipado común.

## H7 — Dependency Accelerator orientado a workers efímeros
**Valor:** menor preparación de dependencias en CI sobre caché fría, sin modificar Maven/Gradle ni falsear resolución.

- [ ] Capturar grafo real resuelto con `pom`, padres, BOM, perfiles, plugins, goals y dependencias transitivas; la clave incluye entorno/JDK/mirrors/flags. No usar `pom.xml` solo como identidad de contenido.
- [ ] Snapshot inmutable versionado + lista digests/contenidos + procedencia + política; delta y planner puro `CacheStrategy` (standard/prefetch/bundle/delta/shared-volume).
- [ ] CLI `hodei deps hydrate` opt-in: sin plugin Maven inicialmente; prepara caché aislada segura con semántica `_remote.repositories`, verificación de digest; fallback al resolver estándar para faltantes.
- [ ] Prefetch y singleflight con presupuesto; evitar re-zipping JAR sin demostrar rendimiento. Compatible con volúmenes prewarm Podman Fabric pero sin bloquear avance por integración externa.
- [ ] Benchmark frío/caliente/múltiples proyectos vs Maven estándar, medir p50/p95 wall-clock **de fase dependencia** y pipeline total; decidir si bundle vale su coste. No objetivos ficticiamente alcanzados.
**Aceptación efímera:** nuevo worker/caché vacía vs caliente, POM inalterado con perfil modificado invalida snapshot; revocación invalida bundle; descarga interrumpida; 1 test de integración real con Maven. **Permanente:** plan pure/data-driven, integridad manifest, invalidaciones; smoke CLI. **AAT:** beneficio >=25% fase de dependencias fría como objetivo de decisión, no contrato preaprobado. **Release:** CLI usable con mediciones.
**Docs:** `SPECS/dependency-accelerator.md`, `ADR/008-accelerator-optional.md`.
**Equipos:** capture/resolver adapter, snapshot graph, CLI transport, benchmarks independientes.

## H8 — Ecosistemas ampliados + gobierno de compatibilidad CE
**Valor:** npm y PyPI reales, APT siguiente si dispone baseline; paridad CE en crecimiento explícito.

- [ ] npm Hosted/Proxy/Group cuando el protocolo lo permita: packument, tarball, dist-tags, integrity, auth y cambios concurrentes; prueba npm CLI real.
- [ ] PyPI Simple API HTML y/o JSON según baseline, wheel/sdist, upload twine si se soporta, package immutability; pip real.
- [ ] APT deb/Release/InRelease/GPG con verificación y firmas donde aplique; el código público Nexus ofrece referencia adicional.
- [ ] Evaluar ampliación restante según matriz oficial CE (Helm, NuGet, Go, RubyGems...) como sub-hitos separados y releases sucesivos; no incluir artificialmente más formatos para declarar completitud.
- [ ] Política uniforme Cedar, CAS, cuotas, audit y grafo para cada formato; API OpenAPI versionada, errores consistentes.
**Aceptación efímera:** npm install/publish, pip install, `apt update` en sandbox si APT incluido; errores 401/403/404/digest. **Permanente:** protocolo data-driven/compat por formato. **Release:** por cada lote de formatos realmente soportados.
**Docs:** `SPECS/protocol-compatibility.md`, `SPECS/api-operations.md`, `ADR/005-nexus-contracts.md`.
**Equipos:** adaptadores npm/PyPI/APT en paralelo con shared contract y un integrador de seguridad.

## H9 — Operación cloud, migración Nexus y certificación de producto
**Valor:** instalación y upgrade, importación Nexus por batches, recuperación, observabilidad y reportes de compatibilidad claros.

- [ ] Operación single-node TypeDB CE + Rust stateless + S3, rollback, export/import/restore, reconciliación y pruebas de pérdida de nodo; documentar limitación de HA sin prometer multinodo.
- [ ] Importar repositorios y artefactos Nexus por protocolos/API/formatos públicamente disponibles; no copiar componentes comerciales ni asumir acceso a código CE completo; mapa de identidades, huellas y reconciliación.
- [ ] OTel/tracing/metrics, budgets de latencia/RSS, límites de consulta recursiva, colas, backpressure y operaciones de rescate.
- [ ] Contenedores/Helm/distribución versionados, SDK/CLI, SLSA/SBOM de release, SHA256SUMS y manifiesto completo, migración/rollback ensayados; no `sources.zip` asset.
- [ ] Matriz Nexus CE con `PASS/PARTIAL/UNSUPPORTED/NOT_RUN` por versión y protocolo; criterios SLO documentados, cierre de deuda crítica; no GA sin seguridad, recuperación y compat publicados.
**Aceptación efímera:** instalación limpia real, upgrade/rollback, export/import y bytes/digests restaurados, cliente de cada formato soportado; pérdida de componente TypeDB por fallo inducido. **Permanente:** invariantes restore, empaquetado, ownership y manifest. **AAT:** cargas concurrentes, recuperación e integridad; no ocultar límite HA.
**Docs:** `SPECS/operations.md`, `SPECS/performance.md`, `RELEASE.md`, `SPECS/protocol-compatibility.md`.
**Equipos:** migración/importer, Ops/K8s, performance benchmark, security release audit; RC/merge bajo integrador único.

## Dependencias críticas
`H0 → H1 → H2 → H3`; `H2 → H4`; `H2 → H5 → H6`; `H3 + H5 → H7`; `H3 + H4 → H8`; `H4 + H6 + H7 + H8 → H9`. Releases de cada hito integrados secuencialmente; investigación y prototipos independientes sí pueden correr antes.

## Cierre común en TODOS los hitos
1. Comportamiento demostrable a consumidor; diff real y docs técnicas actualizadas, sin documentación inflada.
2. Pruebas de impacto + unidad/propiedad + integración puntual + 1–3 UAT/E2E/AAT efímeras (según riesgo) sobre artefacto actual, resultado asociado a SHA/digest, guardando conteo `RUN/PASS/FAIL/SKIP` real y coste.
3. Decisión `PROMOTE/REPLACE/RETIRE/DEFER` por garantía; retirar **antes de cerrar el bloque** todos los scripts/fixtures UAT/E2E/AAT efímeros y limpiar servicios/containers. Reglas permanentes críticas conservadas.
4. Rebase/merge sin divergencia, árbol limpio, release y publicación remota con versión incrementada y assets completos; si bloqueado indicar razón específica. Nunca declarar publicado sin inspeccionar remoto.
5. Una incidencia derivada imprescindible se añade al mismo roadmap con `why/acceptance/dependency`; no cambiar prioridad caprichosamente ni abrir ciclo infinito sin entrega.
