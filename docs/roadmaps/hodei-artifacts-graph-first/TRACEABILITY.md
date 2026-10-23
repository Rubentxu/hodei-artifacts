# Trazabilidad a documentos existentes (no sustituir)

| Documento real de `hodei-artifacts` | Decisión de reutilización | Nuevo contrato |
|---|---|---|
| `docs/prd/8-roadmap.md` | historial y backlog; conservar, añadir enlace a este roadmap | `ROADMAP.md` |
| `docs/prd/3-requirements.md` | contrastar requisitos por capacidad y protocol | `SPECS/protocols.md` |
| `docs/prd/4-bounded-contexts-and-crates-structure.md` | bounded contexts y VSA; no imponer 12 archivos por feature | `SPECS/functional-architecture.md` |
| `docs/prd/5-technical-architecture.md` | topología legado; seleccionar decisiones válidas | `SPECS/runtime-storage.md` |
| `docs/architecture.md` | principios hexagonales, segregación, nextest; revisar invariantes contra código | `SPECS/functional-architecture.md` |
| `docs/domain/domain-model-in-bounded-context.md` | distribución histórica del dominio, duplicación SBOM | `SPECS/ontology.md` |
| `docs/domain/diagrama-domain.md` | relaciones semánticas previas a reconciliar | `SPECS/ontology.md` |
| `docs/arquitectura/ARQUITECTURA-FINAL-CORRECTA.md` | autoría de esquemas Cedar y puertos propios de contexto | `SPECS/authorization-hrn.md` |
| `docs/arquitectura/schema-management-architecture.md` | leer antes de sustituir motor de políticas | `SPECS/authorization-hrn.md` |
| `docs/cedar-policies-manual.md` | reglas actuales a traducir/testear | `SPECS/authorization-hrn.md` |
| `docs/testing-organization.md` | inventario de harness y runner existente; adaptar, no borrar cobertura | `TESTING.md` |
| `docs/DESIGN_REST_API_AND_PLAYGROUND.md` | contratos playground; conservar endpoints cuando sean públicos | `SPECS/api-operations.md` |
| `docs/openapi/` | esquemas existentes, revisar diff de API | `SPECS/api-operations.md` |
| `docs/prd.md` | historia de producto sin tomar como estado implementado | `ROADMAP.md` |

**Código de fuente de verdad:** `Cargo.toml`, `Makefile`, `src/bootstrap.rs`, `src/main.rs`, `crates/{kernel,hodei-iam,hodei-policies,hodei-organizations,hodei-authorizer,repository,artifact,distribution,supply-chain,security,search}`. Priorizar código compilado y contratos observados.

**Regla de divergencia:** registrar decisión dentro de ADR del paquete, enlace a documento anterior, razón y ruta de migración; nunca mantener dos especificaciones activas contradictorias.
