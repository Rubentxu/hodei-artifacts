# Seguridad

Documentos de seguridad de Hodei Artifacts.

- [Baseline de la API y del dominio repository](security/repository-baseline.md): superficie activa, crate `repository` desconectado y seam para el primer `GetRepository`.

- [Wiring del Authorizer compartido](security/authorization-wiring.md): puerto, caso de uso y sustitución reversible.

- [Aislamiento del playground](security/playground-isolation.md): simulación vs autorización productiva, gate y fallos seguros.

- [Lectura autorizada de repositorios](security/repository-read.md): ruta, identidad, 404 uniforme y lector read-only.

- [Eventos de auditoría de CreateRepository](security/audit-events.md): identidad/resultado/correlación, redacción y durabilidad best-effort.

- [Autenticación y autorización de producción](security/production-auth.md): JWT HS256/JWKS, Cedar validado y arranque fail-closed.

- [Persistencia durable de repositorios](security/repository-durable.md): adaptador SurrealDB, unicidad y configuración.

- [Scope de cuenta en IAM](security/iam-account-scope.md): handlers, prefijo persistido y backfill.

- [Un solo parser HRN](security/hrn-parser.md): delegación al contrato y parser explícito de acciones.

- [Circuitos de políticas](security/policy-circuits.md): autoridad del gate vs evaluador IAM.
