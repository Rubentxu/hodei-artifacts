# ADR-003 — HRN v2 y autorización Cedar compuesta
**Estado:** ACEPTADA; es condición de seguridad P0 para releases con tenants.
**Decisión:** identidad global `hrn:partition:service:region:account:kind/id` tipada y no reciclable; UID Cedar incluye HRN completo. IAM permits y resource permits como unión de grants, intersectados con tenant/org boundary; SCP no concede permisos, forbid gana. Cedar snapshot request-scoped coherente, schema + policies + context validados; fail-closed y audit.
**Alternativas:** `Iam::User::"alice"` (rechazado: colisión), concatenar SCP PolicySet para decidir (rechazado: falsa concesión), `ALLOW` caché TTL sin versión (rechazado: revocación tardía).
**Migración:** alias v1 solo con mapping explícito a tenant; conflictos manuales antes de activar nueva identidad. Cubrir aislamiento E2E real.

**Adopción por fases (ver `ADR/011-h1a-identity-and-decision-core.md`):** H1a aplica ya el UID Cedar con scope completo, el snapshot inmutable por petición y la forma única del UID de acción `Action::"X"`; el namespace de acción `Hodei::Action::"ReadAsset"` y la intersección de límites (IAM ∧ recurso ∧ tenant ∧ SCP-deny) quedan en H1b, que es donde el schema pasa a ser la autoridad y se aplica a la petición. El perfil de conformidad de H1a está en `SPECS/authorization-hrn.md`.
