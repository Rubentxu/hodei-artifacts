# SPEC K-001 — SBOM / evidencia / vulnerabilidades externas / decisión

## Ingesta
Conectores: CycloneDX, SPDX, SLSA/in-toto attestations, VEX (OpenVEX/CSAF según formato soportado). Conservar original CAS por digest, origen, firma, parser/version, validación, timestamp y event_id. Reingesta idempotente por (tenant, digest, parser_rev), sin duplicar relaciones. Parser robusto límites tamaño, profundidad y entidades externas.

## Ontología de evidencias
- `Assertion` declara un hecho según un productor; `Observation` es resultado de escáner o feed; `Assessment` es inferencia/política que especifica modelo, fecha, evidencia y revisión.
- `SbomDocument` describe PackageVersion/Blob/Asset; `DependencyAssertion` guarda `(source PURL, target PURL, resolution context, scope, evidence, observed_at)`, no transforma target en dependencia instalada sin resolver.
- PURL/CPE son identificadores externos potencialmente ambiguos, versión/rango/arquitectura importan. CVE, GHSA, OSV aliases solo se fusionan con prueba verificable; fuzzy match queda candidato no autoritativo.
- Finding de Trivy/Grype ≠ Advisory OSV/NVD ≠ Vulnerability CVE: conservar los tres, múltiples scanner runs, severidad por fuente, VEX justification, estado `Affected|NotAffected|UnderInvestigation|Fixed|Unknown`.
- EPSS probabilístico, CISA KEV como señal de explotación, CVSS como severidad; evaluar conjunto, no borrar histórico cuando feed cambie.

## Flujo reactividad
1. Descargar feed con rate limit/ETag/token desde adaptador; fingerprint del contenido.
2. Validar parsear y normalizar; commit TypeDB de nueva observación + trabajo de impacto; errores no silenciosos.
3. Worker aplica correlación y analiza grafo inverso acotado por tenant, marca assessment rev / fuente, actualiza proyección dentro de TypeDB.
4. Gates Cedar consumen una evaluación de seguridad confiable con freshness; si stale/no dato y política exige ello -> fail closed. Confirmar/quarantine/promote por comando explícito auditado.

## APIs
`GET /api/v1/assets/{hrn}/evidence`, `POST /api/v1/sboms` (conceptual), `GET /api/v1/advisories/{id}/impact`, `GET /api/v1/projects/{id}/risk`, `POST /api/v1/policies/{id}/simulate` (versionables y autorizadas); contratos exactos se alinean con `docs/openapi/` existente antes de publicar.

## Reglas de escalado
No descargar NVD completo en cada build, ni ejecutar escaneo dentro de GET de asset; procesos workers con budget/circuit breakers, snapshots de feeds, retries y métricas `feed_lag`, `impact_lag`, `graph_query_p95`, `edges/second`. No filtrar repos de otros tenants mediante grafos públicos.

## Acceptance
CVE nueva → PackageVersion afectada → project build/OCI impact → evaluación Cedar, sin falsos aliases. Reingesta 2 veces mismo SBOM mantiene número de relaciones. VEX negando afectación conserva hallazgo y cambia assessment según confianza. Los originales y firmas son verificables.
