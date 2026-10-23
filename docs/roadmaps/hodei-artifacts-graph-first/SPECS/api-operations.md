# SPEC A-001 — API, CLI, Web UI y observabilidad de recursos

**Legacy:** `docs/DESIGN_REST_API_AND_PLAYGROUND.md`, `docs/openapi/`, `docs/PLAYGROUND_EXAMPLES.md`, `src/handlers/`, `src/openapi/`; comparar rutas existentes antes de definir aliases, evitar cambios arbitrarios.

## Principios
- API REST pública versionada (`/api/v1` para rutas nuevas, ejemplo) con OpenAPI y CLI que consume la API, no rutas internas al grafo. Protocolos Maven/OCI conservan rutas esperadas por sus clientes (no meter `/api/v1` delante salvo especificación).
- Operaciones command/query separadas en vertical slice, permisos explícitos por recurso y acción. Error algebraico mapeado a HTTP: `InvalidInput 400`, `Unauthenticated 401`, `Forbidden 403`, `NotFound 404`, `Conflict 409`, `PreconditionFailed 412`, `TooLarge 413`, `RateLimited 429`, `Unavailable 503` según protocolo concreto.
- Responses paginadas/cursor opaco con orden determinista para explorar grafos sin devolver closure completo; consulta de grafo con maxDepth/nodes/time budget y coste medible.
- Endpoints gestion: create/list repo, mutate policy, publish/upload, get/download, SBOM/evidence, advisory impact, cache snapshot, health/live/ready, metrics; deduplicar con endpoints legacy donde haya cliente.
- Auditoría para cambios de permisos/datos, trace/span/request IDs, métricas de errores por endpoint, latencias p50/p95 y lag de workers; nunca secrets completos o tokens en logs.
- Cuando una operación devuelve estado eventual, incluir `assessment_revision`, `observed_at`, `staleness` y no declararlo verificado si no lo está.

## UI de usuario
- Primer vertical: repositorios y assets + permisos básicos; segundo: árbol de dependencias / SBOM; tercero: findings y policy simulator, según release. Reusar `frontend/` existente únicamente tras verificar su build. Preferir API mock como contrato para agent-team frontend sin acceso a TypeDB, pero probar integración real antes del release.

## Contratos de aceptación
Login/CLI/token y errores negativos; recurso privado no visible en listado/paginación ni detalle; tipos HRN no colisionan; CLI y API equivalentes en identidad y status.
