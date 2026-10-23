# Agent Teams: optimizar tiempo sin carreras sobre dominios

- **Integrator/Release captain (único):** decide cambios de `Cargo.toml` raíz/lockfile, `kernel::Hrn`, migraciones TypeDB definitivas, semver, ramas, merge, tags, releases, manifests. Verifica que los equipos hayan trabajado sobre el mismo HEAD base.
- **Domain/TypeDB:** ontología PERA, TypeQL, índices, transacciones, migración; dueño único del schema por hito.
- **Protocol adapters:** Raw/Maven/OCI/npm/PyPI independientes una vez fijadas `AssetKey`, `BlobRef`, `RepositoryKind` y puertos. Pueden trabajar paralelo en crates/carpetas disjuntos.
- **Security:** Cedar/HRN, IAM/SCP, trust/evidence, tests negativos y validación; no modifica ontología sin RFC al integrador.
- **Supply Chain:** parser CycloneDX, parser SPDX, importers OSV/GHSA/scanner, impacto por grafo; dividir por adaptador, no duplicar `Finding`.
- **QA/Performance:** contratos observables Nexus, matrices, harness efímero, benchmark externo con artefactos inmutables; no añade tests permanentes sin justificar garantía.
- **Ops/Integration:** CAS S3, backup TypeDB, deploy/restore, releases. No comparte contenedores/DB mutable con otros agentes.

**Ritmo:** en cada hito congelar contratos mínimos y asignar áreas exclusivas; agentes producen commits o patches + `evidence/risks`; el integrador revisa diffs, resuelve conflictos y ejecuta verificación integrada. Ningún agente cambia silenciosamente un contrato de otro. Dependencias nuevas van por petición al integrador. Los bloqueos de tooling se investigan primero; no abrir worktrees sin acceso a tests o especificación.

**Ejemplo H3 paralelo:** Team A Hosted Maven; Team B Proxy revalidation/singleflight; Team C Group resolver/ACL; Team D clientes reales/contratos. `MavenAssetKey` y `StoragePort` fijos previamente por integrador. Luego integración conjunta y release.

**Ejemplo H6 paralelo:** OSV/GHSA ingest vs NVD/KEV/EPSS vs Risk Assessment vs Cedar admission. `VulnerabilityIdentity` y `EvidenceProvenance` fijados antes; de lo contrario no paralelizar.
