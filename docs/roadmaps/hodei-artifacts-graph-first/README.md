# Hodei Artifacts — Graph-first Rust / TypeDB, plan integrable

**Estado:** PROPUESTA DE IMPLEMENTACIÓN, no trabajo ejecutado. 2026-10-22.
**Baseline consultada:** `Rubentxu/hodei-artifacts` `main` @ `2626e35b7c12375af0a1f23342eee4b4619bfc3c` (2025-10-11); read-only, no compilación real. Si el remoto ha avanzado, reconciliar antes de aplicar.

**Objetivo:** recuperar valor demostrado del monorepo Hodei y convertirlo por vertical slices en un registry interoperable con Nexus Repository Community Edition (Nexus CE) y un grafo de conocimiento tipado TypeDB 3, manteniendo núcleo funcional puro (Rust idiomático estilo Haskell), Cedar, recursos HRN v2, CAS/FS/S3 y SBOM/vulnerabilidades con evidencia. No copiar arquitectura Java. No hay segundo SQL, Event Sourcing universal ni microservicios de entrada.

## Entrada para el agente

1. Leer `AGENT-PROMPT.md`, `BASELINE.md`, `ROADMAP.md`, `TESTING.md` y ADRs. Abrir `../` de este paquete solo para especificaciones listadas en `TRACEABILITY.md`; comprobar el código real.
2. Leer de la skill externa **de verdad** `Rubentxu/agent-skill/skills/delivery-first-testing/SKILL.md` y sus lentes `ready` afectadas. No dar por instalada una skill porque se mencione aquí.
3. Revisar `Cargo.toml` y `Makefile`: hoy `make test` referencia crates excluidos; el binario raíz se autodenomina stub; existen archivos obsoletos dependientes del crate `shared` inexistente. No reactivar crates en bloque.
4. Empezar por H0, integrar el siguiente hito solo cuando sus garantías estén verificadas. Cada bloque produce release real + merge/push remoto, pero **solo en el entorno del agente con permisos**. Este paquete no realiza cambios Git.
5. Mantener `docs/prd/8-roadmap.md` como índice/histórico y añadir el vínculo hacia este roadmap; no sustituirlo ni declarar completadas sus casillas retrospectivamente.

## Contenido
- `ROADMAP.md`: 10 hitos largos H0–H9 con capacidades y dependencias.
- `SPECS/`: contratos completos, invariantes, protocolos, rendimiento, restauración, seguridad, supply chain, caché y migraciones.
- `ADR/`: decisiones y alternativas explícitas.
- `ACCEPTANCE.md` + `TESTING.md`: matriz de garantías, pruebas minimizadas y aceptación real efímera.
- `TEAM-PLAN.md`, `RELEASE.md`: trabajo paralelo y cierre por release.
- `TRACEABILITY.md`, `BASELINE.md`, `RISK-REGISTER.md`, `GLOSSARY.md`, `SOURCES.md`.
- `roadmap.json`: representación programática con hitos y criterios.

## Exclusiones claras
- No se promete compatibilidad completa Nexus CE desde los primeros releases; matrices de paridad por versión y formato.
- No se presume que TypeDB Community tenga HA multinodo; documentar RPO/RTO y recuperación real.
- No son fuentes de verdad las listas del README antiguo, ni tests o ejemplos no ejecutados.
- Sin PostgreSQL, SurrealDB, Kafka ni Event Sourcing universal salvo ADR posterior sustentado en mediciones.
- REA para comportamiento binario/protocolar opaco; CogniCode para mapa estructural y llamadas del código Java/Rust.
