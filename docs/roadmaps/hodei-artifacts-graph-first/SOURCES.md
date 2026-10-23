# Fuentes verificadas y qué validan (2026-10-22)

## Repositorios reales y especificaciones previas
- Hodei baseline: https://github.com/Rubentxu/hodei-artifacts/tree/2626e35b7c12375af0a1f23342eee4b4619bfc3c
- Skill/router/lentes: https://github.com/Rubentxu/agent-skill/tree/main/skills/delivery-first-testing
- Nexus core público (no igual a CE completa): https://github.com/sonatype/nexus-public
- Nexus CE matriz oficial: https://help.sonatype.com/en/nexus-repository-feature-matrix.html
- Nexus CE feature tiers: https://help.sonatype.com/en/nexus-repository-tiers-and-feature-availability.html

## TypeDB
- TypeDB 3 PERA entidades/relaciones: https://typedb.com/docs/core-concepts/typeql/entities-relations-attributes/
- Modelado y cardinalidades: https://typedb.com/docs/guides/schema-modeling/
- Transacciones y snapshot isolation: https://typedb.com/docs/core-concepts/typedb/transactions/
- TypeQL 3: https://typedb.com/docs/typeql-reference/schema/define/
- Export/import desde 3.4+: https://typedb.com/docs/maintenance-operation/database-export-import/
- TypeDB driver Rust: https://github.com/typedb/typedb-driver
- Alta disponibilidad TypeDB Cloud (estado vigente requiere revisar release): https://typedb.com/docs/home/install/cloud/

## Cedar y grafos de supply chain
- Cedar schema: https://docs.cedarpolicy.com/schema/schema.html
- Cedar validación: https://docs.cedarpolicy.com/policies/validation.html
- Cedar Rust entities: https://docs.rs/cedar-policy/latest/cedar_policy/struct.Entities.html
- SPDX: https://spdx.dev/ ; CycloneDX: https://cyclonedx.org/
- PURL: https://github.com/package-url/purl-spec
- GUAC: https://github.com/guacsec/guac ; Trustify: https://github.com/guacsec/trustify
- OSV: https://osv.dev/ ; NVD: https://nvd.nist.gov/ ; KEV: https://www.cisa.gov/known-exploited-vulnerabilities-catalog ; FIRST EPSS: https://www.first.org/epss/
- OCI Distribution spec: https://github.com/opencontainers/distribution-spec

**Condición:** estas fuentes ayudan a diseñar; antes de implementar verificar versiones/protocolos exactos, licencia y semántica real; no se ejecutaron TypeQL de estos docs ni se validó contra Nexus CE binario en esta preparación.
