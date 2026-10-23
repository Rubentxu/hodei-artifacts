# ADR-007 — Graph-first con afirmaciones/evidencia y no hechos inferidos ciegamente
**Estado:** ACEPTADA COMO DIRECCIÓN.
**Decisión:** distinguir `Fact` transaccional vs `Assertion` de SBOM/provenance, `Observation` de scanner y `Assessment` versionado. PURL distinto HRN/digest. Relación tipada conserva roles, fuente y timestamps. CVE/GHSA/OSV alias solo por fuentes fiables; nada de fuzzy merge automático. Los originales siguen en CAS.
**Alternativas:** `Vec<dependencies>` en Asset agregado, 1 SBOM blob sin relaciones, overwriting scanner fields, universal vulnerability score, todos rechazados por pérdida de semántica.
**Gates:** reingesta idempotente, impacto inverso, VEX conflictivo y audit de decisión.
