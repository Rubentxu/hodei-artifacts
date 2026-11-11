# Estado de la arquitectura

**Hito vigente:** M0 — Convergencia (del roadmap `hodei-artifacts-trustgraph-v3`).
**Generación de documento que manda:** PRD v3 y la UAT de convergencia que lo acompaña.
**Fecha:** 2026-11-11.

Este documento dice la verdad del repositorio: qué se construye hoy, qué es
material heredado, y qué capacidades existen de verdad. Reemplaza a las
generaciones anteriores de material de arquitectura, que describen un producto
que este repositorio ya no es.

La regla que lo gobierna es simple: **una capacidad no está completa porque
esté documentada, sino porque existe su ruta real y su prueba.** Todo lo que
no llega a ese listón está etiquetado, y la etiqueta dice en qué estado está.

## 1. Etiquetas

| Etiqueta | Significado |
| --- | --- |
| `implementada` | Existe la ruta, existe la prueba, y la prueba se ejecuta en la CI. |
| `experimental` | Existe y funciona, pero no cumple todavia el contrato de seguridad o de producto de un hito completo. |
| `planificada` | Especificada en el roadmap, sin implementar. |
| `legacy` | Heredado de una generación anterior. No se construye, no se despliega, no se promete. |
| `deprecada` | Existió y su reemplazo está en marcha. |
| `referencia` | Material de estudio o de diseño. No es código. |

## 2. Crates

### 2.1 Miembros del workspace — `implementada` salvo donde se indica

| Crate | Papel | Estado |
| --- | --- | --- |
| `crates/kernel` | Núcleo compartido: identidad HRN v2, tipos de dominio, puertos. | `implementada` |
| `crates/hodei-iam` | IAM: usuarios, grupos, políticas; persistencia en SurrealDB. | `implementada` |
| `crates/hodei-policies` | Motor Cedar: schema, validación, evaluación, registro. | `implementada` |
| `crates/hodei-evidence` | Contexto de evidencia y registro de eventos (M1): los contratos, sin adapter. | `implementada` |
| `crates/hodei-evidence-storage` | Los adaptadores de la evidencia sobre el almacen que ya existe. | `implementada` |
| `.` (`hodei-artifacts-api`) | API HTTP: composición, arranque, rutas, auditoría. | `implementada` |

El kernel no depende de ningún mecanismo concreto: ni SurrealDB, ni Axum, ni
HTTP. Eso no es una aspiración, lo comprueba una comprobación de forma
arquitectónica.

### 2.2 Crates en el árbol que **no** son miembros — material heredado

Ninguno se compila, ninguno se despliega, ninguno se enlaza. Todos están
excluidos del workspace de forma explícita (`exclude` en el manifiesto raíz),
para que nadie los confunda con código vivo.

| Crate | Líneas | Clasificación | Para qué sirve |
| --- | --- | --- | --- |
| `crates/distribution` | 24 455 | `legacy` | OCI, Maven, npm, Docker. La base de M9, si M9 llega. |
| `crates/search` | 15 850 | `legacy` | Búsqueda de texto completo. |
| `crates/artifact` | 14 188 | `legacy` | Subida por trozos, metadatos, versionado. Material para M1 y M9. |
| `crates/hodei-organizations` | 7 924 | `legacy` | SCP y organizaciones. Sustituido por el modelo de cuenta por defecto. |
| `crates/security` | 5 546 | `legacy` | SBOM, vulnerabilidades, firma. Material para M2 y M4. |
| `crates/repository` | 5 113 | `legacy` | CRUD de repositorios. |
| `crates/hodei-authorizer` | 3 077 | `legacy` | Servicio de autorización anterior, superado por el gate Cedar. |
| `crates/policies` | 3 315 | `legacy` | Crate de políticas anterior. No compila contra el kernel actual. |
| `crates/supply-chain` | 962 | `legacy` | SBOM de cadena de suministro. Material para M2. |

**Decisión:** no se borran todavía. Son la única fuente de las ideas de M1,
M2, M4 y M9, y borrarlas tiraría ese material. Cuando una idea se promueva,
el código se trae de aquí y el crate viejo se elimina con su línea.

## 3. Capacidades

### 3.1 Implementadas — con su ruta y su prueba

| Capacidad | Ruta | Dónde se demuestra |
| --- | --- | --- |
| Salud | `GET /health`, `/health/ready`, `/health/live` | `tests/repository_read.rs` |
| Repositorios: alta y lectura | `GET|POST /api/v1/repositories` | `tests/repository_create.rs`, `tests/repository_read.rs` |
| Políticas IAM: alta, listado, lectura, modificación, baja | `/api/v1/iam/policies*` | `tests/iam_policy_timestamps.rs`, `tests/iam_audit.rs` |
| Evaluación de políticas IAM | `POST /api/v1/iam/evaluate` | `tests/policy_circuits.rs` |
| Validación y evaluación de políticas | `/api/v1/policies/validate`, `/api/v1/policies/evaluate` | `tests/policy_circuits.rs` |
| Schema: construir, cargar, registrar IAM | `/api/v1/schemas/*` | `tests/policy_circuits.rs` |
| Playground de políticas | `POST /api/v1/playground/evaluate` | `crates/hodei-policies` |
| Autenticación: secreto compartido y JWKS/RS256 | toda ruta protegida | `tests/production_auth.rs`, `tests/repository_restart_contract.rs` |
| Autorización Cedar con bundle declarado | superficie de repositorios | `tests/policy_circuits.rs` |
| Auditoría de seguridad | eventos de las mutaciones | `tests/audit_events.rs`, `tests/iam_audit.rs` |
| Comando operador del índice de políticas | `ensure-iam-index` | `tests/iam_account_index.rs` |

### 3.2 Experimentales

| Capacidad | Por qué es experimental |
| --- | --- |
| Frontend (`frontend/`) | Tiene componentes, páginas y 37 pruebas de componente que ahora sí se ejecutan, pero llama a un contrato OpenAPI que describe superficies que el backend actual no expone: usuarios, búsqueda, artefactos y tokens. Su navegación no está alineada con la API viva. |
| Playground de políticas | Admite varias gramáticas de acción y está por detrás de la superficie administrativa. |

### 3.3 Planificadas — en el roadmap, sin implementar

Evidence Graph (M1), Trust Capsule (M2), Trust Gate (M3), Risk Graph (M4),
Reachability (M5), Reactive Security (M6), Simulation (M7),
AI Investigator (M8), Trusted Artifact Hub (M9).

### 3.4 Lo que este repositorio **no** es

La generación anterior de material lo describía como un "registry de
artefactos multi-formato con control de acceso" y una "plataforma de gestión
de organizaciones". **Ninguna de las dos cosas es cierta hoy.**

- No hay almacenamiento de blobs. El registro de artefactos por digest es M1.
- No hay OCI, ni Maven, ni npm. Eso es M9, y su material está en
  `crates/distribution`, sin integrar.
- No hay organizaciones ni SCP. El modelo vivo es la cuenta por defecto.
- No hay grafo, ni evidencia, ni decisión de promoción. Son M1, M2 y M3.

## 4. Documentación

| Documento | Clasificación |
| --- | --- |
| Este documento | vigente |
| `docs/security/*.md` | vigente: describe el comportamiento comprobado |
| `docs/ci-strategy.md` | vigente |
| `docs/cedar-policies-manual.md` | vigente |
| `docs/prd/` (10 documentos) | `deprecada`: el PRD v3 lo sustituye |
| `docs/prd.md` | `deprecada`: el PRD v3 lo sustituye |
| `docs/arquitectura/` (10 documentos) | `legacy`: siete de ellos se llaman "final", "definitiva" o "corregida" y se contradicen entre sí. Este documento los sustituye a todos |
| `docs/architecture.md` | `legacy` |
| `docs/historias-usuario.md`, `docs/DESIGN_REST_API_AND_PLAYGROUND.md`, `docs/PLAYGROUND_EXAMPLES.md` | `referencia` |
| `docs/api/`, `docs/openapi/`, `docs/postman/` | `referencia`: artefactos generados |
| `docs/testing-organization.md` | `deprecada`: describe una organización de pruebas que ya no es la de este repositorio |

## 5. Costuras del tiempo y de la identidad

Un momento y un identificador no son entradas: son hechos sobre la maquina.
Cuando el codigo los lee directamente, el resultado depende de cuando se
ejecuto, y dos ejecuciones de la misma operacion no se pueden comparar. Eso
importa donde el resultado es evidencia: una traza cuyo momento no se puede
reproducir no se puede comprobar.

Por eso el reloj y el generador de identificadores son **costuras**
(`kernel::application::ports::clock`): el cableado de produccion lee la
maquina, y una prueba los fija para que la traza que compara sea un hecho y
no un recuerdo. La traza de auditoria los usa ya para su identificador, su
momento y su correlacion.

## 6. El contexto de evidencia

`crates/hodei-evidence` es el primer contexto de M1. Contiene los contratos
y nada mas: no hay adapter, ni almacen, ni HTTP, ni una sola dependencia de
un motor de base de datos (una comprobacion de forma lo verifica ✓).

Dos reglas dan forma a los contratos:

- **El contenido se referencia, no se copia.** Un registro de evidencia
  guarda el digest y la localizacion de su contenido, y no el contenido. Eso
  hace que el manejo de secretos sea estructural y no una promesa: un
  registro no tiene donde llevar un token.
- **La envoltura de evento es inmutable por construccion.** Los campos son
  privados y no hay manera de cambiarlos; una correccion es otro evento, no
  una edicion. Un doctest `compile_fail` lo demuestra.

La clave de idempotencia es **estructurada**, no un texto que haya que
desmontar: sus cuatro hechos --productor, identidad externa, digest del
contenido y version mayor del normalizador-- son campos ✓. Por eso puede
distinguir una entrega repetida de una revision nueva, y por eso la revision
no sobrescribe la historia.

## 7. El ledger de eventos

`crates/hodei-evidence-storage` implementa el puerto del ledger sobre el
almacen que el despliegue ya usa. El detalle que lo define: **el
identificador de un registro del ledger es la posicion que ocupa en su
corriente** (`event_ledger:corriente~posicion`).

De ahi salen tres cosas a la vez:

- **La unicidad de la posicion es atomica**: es la clave primaria, asi que
  dos escritores no pueden tomar la misma posicion.
- **No hace falta ningun indice**, y eso importa mas de lo que parece:
  definir un indice en el motor local mantiene bloqueado el directorio de
  la base de datos por el proceso que lo define, de modo que un servicio que
  declarara un indice en su camino no podria reiniciar nunca. Con la
  posicion como identificador, el servicio no declara nada.
- **El orden de una corriente es el orden de sus identificadores**.

El precio se dice con su nombre: leer una corriente es un recorrido de la
tabla, no una busqueda por indice. A la escala en la que trabaja M1 es el
canje correcto, y la medida a escala queda para la UAT de M1 en vez de
darse por supuesta.

## 8. El nucleo no depende de mecanismos concretos

El kernel no depende de SurrealDB, ni de Axum, ni de ningun cliente HTTP.
La persistencia real vive en `crates/hodei-iam/src/infrastructure/surreal/`.

El kernel tenia un adaptador de SurrealDB de 403 lineas que era un
**placeholder**: su `connect()` no conectaba, devolvia `Ok`, y seis tests
"de integracion" solo comprobaban su propia contabilidad. No lo usaba nadie
y arrastraba la dependencia concreta al nucleo protegido, asi que se
retiro. La cobertura real de la base de datos esta en los tres tests de
integracion de `hodei-iam`, que si hablan con SurrealDB.

## 7. Costuras de la configuracion

La validacion de la configuracion es **pura**: mira la configuracion y nada
mas. No toca el sistema de ficheros ni la red, de modo que la misma
configuracion valida siempre igual y validarla no deja directorios atras.

Si la maquina puede honrar la configuracion es otra pregunta, y la contesta
el **preflight de almacenamiento**, que se invoca aparte al arrancar y
reporta los fallos del almacenamiento como lo que son: un problema de la
maquina, no de la configuracion.

## 10. Las comprobaciones de forma

Una arquitectura que solo existe en un documento se deshace en el primer
commit que nadie revisa con cuidado. `tests/architecture_fitness.rs` lee el
repositorio y falla cuando aparece una forma que el diseño prohibe:

| Comprobacion | Que prohibe |
| --- | --- |
| `the_active_code_does_not_downcast` | Bajar a una implementacion concreta por downcast. |
| `a_protected_crate_does_not_depend_on_a_concrete_mechanism` | Que el nucleo protegido o los contratos de evidencia nombren un motor de base de datos o un framework de HTTP. |
| `the_active_code_does_not_import_another_context_internals` | Importar lo interno de otro contexto. |
| `there_is_no_success_of_filler` | Un `todo!()`, o un fichero de pruebas que se declara marcador de posicion sin pruebas dentro. |

La comprobacion del downcast trae consigo la prueba de que ella misma
falla cuando encuentra un downcast, y la de las dependencias, la de que
reconoce una dependencia prohibida y no confunde un comentario con una. Una
comprobacion que no puede fallar no es una comprobacion.

`tests/ci_suites.rs` hace lo mismo con la segmentacion de la CI: cada target
de prueba tiene que estar en exactamente una suite, ninguna suite puede
nombrar un target que no existe, y el inventario de lo descubierto se
imprime en el log del job.

## 11. Lo que este documento no dice

No dice qué viene después en detalle: eso está en el roadmap v3 y en el
backlog de implementación que lo acompaña. No dice que el repositorio esté
bien: dice qué está vivo y qué no, que es lo que un revisor necesita para no
tomarse por bueno un documento heredado.
