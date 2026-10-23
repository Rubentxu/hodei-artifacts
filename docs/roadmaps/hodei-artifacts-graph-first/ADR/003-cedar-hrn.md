# ADR-003 — HRN v2 y autorización Cedar compuesta
**Estado:** ACEPTADA; es condición de seguridad P0 para releases con tenants.
**Decisión:** identidad global `hrn:partition:service:region:account:kind/id` tipada y no reciclable; UID Cedar incluye HRN completo. IAM permits y resource permits como unión de grants, intersectados con tenant/org boundary; SCP no concede permisos, forbid gana. Cedar snapshot request-scoped coherente, schema + policies + context validados; fail-closed y audit.
**Alternativas:** `Iam::User::"alice"` (rechazado: colisión), concatenar SCP PolicySet para decidir (rechazado: falsa concesión), `ALLOW` caché TTL sin versión (rechazado: revocación tardía).
**Migración:** alias v1 solo con mapping explícito a tenant; conflictos manuales antes de activar nueva identidad. Cubrir aislamiento E2E real.
