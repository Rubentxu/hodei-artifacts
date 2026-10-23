# SPEC G-001 — Ontología PERA TypeDB 3 y lenguaje ubicuo
**Fuente:** `docs/domain/domain-model-in-bounded-context.md`, `docs/domain/diagrama-domain.md`; modelo nuevo sustituye duplicaciones, sin borrar historia.

## Partición lógica de ontología
1. Identity: Tenant, Organization, Account, User, ServiceAccount, Group, Role, CredentialRef (secret real fuera).
2. Resources: Resource(HRN), Repository{Hosted,Proxy,Group}, Upstream, Package, PackageVersion, Asset, Blob, Tag.
3. Build: Project, SourceRevision, BuildRun, Toolchain, DependencySnapshot, ResolutionContext.
4. Supply chain: SbomDocument, SbomComponentRef(PURL), Provenance, Signature, Attestation, DependencyAssertion, Evidence.
5. Security: Scanner, ScanRun, Finding, Vulnerability, Advisory, VexStatement, Assessment, RiskScore.
6. Governance: Policy, PolicyBinding, PolicyRevision, DecisionRecord, Approval, QuarantineState.

**Identidades:** HRN incluye tenant/scope/region y opaque ID; PURL externo (paquete), `sha256` (bytes), CVE/GHSA/OSV (aliases verificados). No relacionar por nombres visuales. Todas las entidades multitenant tienen tenant inmutable; entidades públicas/advisories globales tienen `global-scope` explícito y aristas de pertenencia autorizadas.

## Categorías TypeDB
- **Entity:** objeto con vida independiente (Repository, PackageVersion, SbomDocument, Scanner, Advisory).
- **Relation:** conexión con roles explícitos (publication publisher/repository/asset, dependency parent/dependent/evidence/context, scan-detection scan/version/finding).
- **Attribute:** valor simple con restricciones (HRN, digest, PURL, timestamps, source-version), no usar 50 tipos por atributo sin necesidad.
- **Assertion vs Observation vs Assessment:** afirmaciones con fuente y fecha; observaciones recibidas; evaluaciones derivadas, versionadas. Conservar conflicto entre evidencias en vez de sobrescribir.

## Esqueleto TypeQL 3 (ilustrativo, validar contra versión exacta antes de migrar)
```typeql
# ESQUEMA PARCIAL de ejemplo, NO migración completa ni validada.
define
  attribute hrn, value string;
  attribute digest, value string;
  attribute path, value string;
  attribute recorded-at, value datetime;
  entity resource @abstract, owns hrn @key;
  entity repository sub resource, plays serves:repository;
  entity asset sub resource, owns path, plays serves:asset, plays backing:asset;
  entity blob, owns digest @key, plays backing:blob;
  relation serves, relates repository @card(1), relates asset @card(1);
  relation backing, relates asset @card(1), relates blob @card(1);
```
**Nota:** el real debe usar tipos adicionales, claves/cardi­nali­dades y tipos de ruta únicos por repo/tenant; el ejemplo no asegura unicidad compuesta ni hace limpieza GC. En TypeDB las cardinalidades sobre roles/ownership dependen de la sintaxis exacta del 3.x pinneado; correr transacción SCHEMA y pruebas de constraints antes de activar. Consultar documentación TypeDB 3 citada en `SOURCES.md`.

## Regla de metadatos de artefactos
- Repository (ruta y política) contiene PackageVersion (coordenadas lógicas), que tiene 1..n Asset (ruta accesible); cada Asset apunta a Blob (digest+storage receipt); distintos assets pueden referenciar el mismo blob.
- OCI Manifest/Index y layer tienen relaciones a Blob, Tag es referencia mutable versionada a manifest; GC camina referencias vivas.
- Group no copia assets: resolución ordenada mediante aristas membership con posición; detectar ciclos y cross-tenancy.
- Proxy registra origin, last-validated, ETag upstream y cache state; negative TTL con límites.

## Unicidad y concurrencia
- Unicidad por HRN (clave opaca global) + unicidad lógica `(tenant,repository,asset_path)` mediante `AssetKey` canónico y `@key`, no confiar solo en relationship traversal.
- TypeDB ofrece snapshot isolation; para protección contra write skew en capacidad crítica imponer versión por agregado, condición de commit/contrato serializable por operación o lock/admission en aplicación por clave; probar dos escritores concurrentes con mismo asset.
- Contextos de resolución Maven siempre explícitos: un POM igual puede resolver diferente por perfil, BOM, repositorio, toolchain y tiempo.

## Versionado temporal
Cada assertion/scan/evaluation tiene `evidence-id`, fuente, fecha observada y validez declarada si la aporta; decisiones guardan versión de política + digest evidencia + modelo; evitar confundir fecha de informe con vigencia temporal de una CVE.

## Consultas de aceptación
- Resolver `(tenant, repo, asset-path)` y devolver digest sin búsqueda global.
- Mostrar assets de versión, dependencies transitivas hasta presupuesto, reverse dependencies por CVE, SBOM raw/document, publisher/owner; todas las queries parametrizadas y acotadas.
- Medir cardinalidad antes/después de reingesta de mismo documento: no duplicación.
- Tenant A no enumera tenant B aunque conozca PURL o digest.
