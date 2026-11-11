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

## 5. Costuras de la configuracion

La validacion de la configuracion es **pura**: mira la configuracion y nada
mas. No toca el sistema de ficheros ni la red, de modo que la misma
configuracion valida siempre igual y validarla no deja directorios atras.

Si la maquina puede honrar la configuracion es otra pregunta, y la contesta
el **preflight de almacenamiento**, que se invoca aparte al arrancar y
reporta los fallos del almacenamiento como lo que son: un problema de la
maquina, no de la configuracion.

## 6. Lo que este documento no dice

No dice qué viene después en detalle: eso está en el roadmap v3 y en el
backlog de implementación que lo acompaña. No dice que el repositorio esté
bien: dice qué está vivo y qué no, que es lo que un revisor necesita para no
tomarse por bueno un documento heredado.
