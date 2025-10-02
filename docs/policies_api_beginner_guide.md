# Guía para principiantes: API de Policies

Esta guía explica cómo usar los endpoints principales del API de policies. Verás ejemplos "copy-paste" con cURL y la forma esperada de requests y responses.

## Conceptos básicos

- **Cedar**: lenguaje de autorización usado para definir políticas.
- **EUIDs**: identificadores de entidades de Cedar, p. ej. `User::"alice"`, `Action::"view"`, `Resource::"doc1"`.
- **Contexto**: datos JSON de la petición (p. ej. `{ "mfa": true }`) disponibles en condiciones `when { ... }`.
- **Schema (opcional)**: describe tipos/acciones. Si no estás validando tipos/acciones, puedes omitirlo.

Rutas en `src/api/policy_handlers.rs`:
- Playground: `POST /api/v1/policies/playground`
- Playground Batch: `POST /api/v1/policies/playground/batch`
- Analysis: `POST /api/v1/policies/analysis`
- Crear/Listar/Actualizar policies: `POST/GET/PUT /api/v1/policies[...]` (opcional)

---

## 1) Playground (evaluación ad-hoc)

Endpoint:
- `POST /api/v1/policies/playground`

Cuándo usarlo:
- Para probar 1..N escenarios de autorización contra un set de políticas Cedar sin persistir nada.

Request mínimo:
```json
{
  "policies": [
    "permit(principal, action, resource) when { context.mfa == true };"
  ],
  "schema": null,
  "entities": [],
  "authorization_requests": [
    {
      "name": "alice-allow",
      "principal": "User::\"alice\"",
      "action": "Action::\"view\"",
      "resource": "Resource::\"doc1\"",
      "context": { "mfa": true }
    }
  ],
  "options": { "include_diagnostics": true, "include_policy_traces": false }
}
```

Respuesta (resumen):
- `policy_validation`: estado de parseo/validación.
- `schema_validation`: estado de schema (si se envió).
- `authorization_results[]`:
  - `scenario_name`, `decision` (Allow|Deny), `reasons`, `determining_policies?`
- `statistics`: agregados (allow/deny/tiempos).

Ejemplo cURL:
```bash
curl -sS -X POST http://localhost:8080/api/v1/policies/playground \
  -H 'content-type: application/json' \
  -d '{
    "policies": ["permit(principal, action, resource) when { context.mfa == true };"],
    "schema": null,
    "entities": [],
    "authorization_requests": [
      {
        "name": "alice-allow",
        "principal": "User::\"alice\"",
        "action": "Action::\"view\"",
        "resource": "Resource::\"doc1\"",
        "context": {"mfa": true}
      }
    ],
    "options": {"include_diagnostics": true, "include_policy_traces": false}
  }'
```

Tips:
- **include_policy_traces=true** habilita heurística de “políticas determinantes” (re-evaluaciones). Tiene coste extra.

---

## 2) Playground con trazas (políticas determinantes)

Mismo endpoint que Playground:
- `POST /api/v1/policies/playground`

Setea:
- `"options": {"include_diagnostics": true, "include_policy_traces": true}`

Request de ejemplo (prohíbe admins y luego permite todo):
```json
{
  "policies": [
    "forbid(principal in Group::\"admins\", action, resource);",
    "permit(principal, action, resource);"
  ],
  "schema": null,
  "entities": [
    { "uid": "User::\"alice\"", "attributes": {}, "parents": ["Group::\"admins\""] },
    { "uid": "Group::\"admins\"", "attributes": {}, "parents": [] }
  ],
  "authorization_requests": [
    {
      "name": "alice-deny",
      "principal": "User::\"alice\"",
      "action": "Action::\"view\"",
      "resource": "Resource::\"doc1\"",
      "context": null
    }
  ],
  "options": { "include_diagnostics": true, "include_policy_traces": true }
}
```

Respuesta destacada:
- `authorization_results[].determining_policies`: IDs de políticas que, al removerse, cambian la decisión. Útil para comprender el “por qué” del Allow/Deny.

---

## 3) Playground Batch

Endpoint:
- `POST /api/v1/policies/playground/batch`

Cuándo usarlo:
- Para muchas evaluaciones similares. No devuelve cada resultado, solo estadísticas agregadas y conteos.

Request:
```json
{
  "policies": [
    "permit(principal, action, resource) when { context.mfa == true };",
    "forbid(principal == User::\"bob\", action, resource);"
  ],
  "schema": null,
  "entities": [],
  "scenarios": [
    {"name":"alice","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}},
    {"name":"bob","principal":"User::\"bob\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}}
  ],
  "limit_scenarios": 100,
  "timeout_ms": 2000
}
```

Respuesta:
- `results_count`: número de escenarios evaluados (respeta `limit_scenarios`).
- `statistics`: `allow_count`, `deny_count`, tiempos agregados.

Tips performance:
- `limit_scenarios` recorta carga.
- `timeout_ms` corta escenarios lentos (computacional o por volumen).

---

## 4) Analysis (reglas de seguridad)

Endpoint:
- `POST /api/v1/policies/analysis`

Cuándo usarlo:
- Para verificar propiedades de seguridad sobre tu set de políticas.

Reglas soportadas (iniciales):
- `no_permit_without_mfa`: detecta Allows sin `context.mfa == true`.
- `no_permit_without_condition`: detecta `permit(...)` sin `when`/`unless`.
- Validación con `Schema/Validator` (si `schema` presente).

Request PASS (mfa requerida):
```json
{
  "policies": ["permit(principal, action, resource) when { context.mfa == true };"],
  "schema": null,
  "rules": [ { "id": "r1", "kind": "no_permit_without_mfa", "params": {} } ]
}
```

Request FAIL (permit sin mfa):
```json
{
  "policies": ["permit(principal, action, resource);"],
  "schema": null,
  "rules": [ { "id": "r1", "kind": "no_permit_without_mfa", "params": {} } ]
}
```

Respuesta:
- `passed: true|false`
- `violations[]`: `rule_id`, `message` con evidencia mínima (escenario P/A/R que violó).

---

## 5) Entidades, EUIDs y Contexto

- **EUID**: `"Type::\"id\""` (comillas escapadas).
  - Ejemplos: `User::"alice"`, `Group::"admins"`, `Action::"view"`, `Resource::"doc1"`.
- **Entities** (`entities[]`):
  - `uid`: EUID
  - `attributes`: JSON (se mapea a `RestrictedExpression`)
  - `parents[]`: EUIDs de padres (p. ej. membresías a grupos)
- **Contexto**:
  - JSON plano por escenario. Se accede en Cedar con `context.<clave>`:
    - `when { context.mfa == true }`

---

## 6) Errores comunes

- **400 Bad Request**:
  - Políticas con sintaxis incorrecta.
  - EUIDs mal formados (falta `\"` interno).
  - Request sin escenarios/políticas.
- **Validaciones de schema**:
  - Si se envía `schema` inválido, aparecen errores en `schema_validation.errors`.
- **Traces lentas**:
  - `include_policy_traces=true` re-evalúa por política; úsalo como flag opcional.

---

## 7) Ejemplos rápidos con cURL

Playground con MFA:
```bash
curl -sS -X POST http://localhost:8080/api/v1/policies/playground \
  -H 'content-type: application/json' \
  -d '{
    "policies": ["permit(principal, action, resource) when { context.mfa == true };"],
    "schema": null,
    "entities": [],
    "authorization_requests": [
      {"name":"s1","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context":{"mfa":true}}
    ],
    "options": {"include_diagnostics": true, "include_policy_traces": false}
  }'
```

Batch básico:
```bash
curl -sS -X POST http://localhost:8080/api/v1/policies/playground/batch \
  -H 'content-type: application/json' \
  -d '{
    "policies": [
      "permit(principal, action, resource) when { context.mfa == true };",
      "forbid(principal == User::\"bob\", action, resource);"
    ],
    "entities": [],
    "schema": null,
    "scenarios": [
      {"name":"alice","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}},
      {"name":"bob","principal":"User::\"bob\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}}
    ],
    "limit_scenarios": 100,
    "timeout_ms": 2000
  }'
```

Analysis (no permit sin MFA):
```bash
curl -sS -X POST http://localhost:8080/api/v1/policies/analysis \
  -H 'content-type: application/json' \
  -d '{
    "policies": ["permit(principal, action, resource);"],
    "schema": null,
    "rules": [ {"id":"r1","kind":"no_permit_without_mfa","params":{}} ]
  }'
```

---

## 8) Buenas prácticas

- **IDs explícitos de política** en trazas para mejor lectura:
  - `@id("permit-all") permit(...);`
- **Usar `schema`** cuando quieras validaciones más estrictas (acciones/entidades definidas).
- **Batch**: usa `limit_scenarios` y `timeout_ms` para cargas grandes.
- **Traces**: úsalas con prudencia (coste N×políticas).

---

## 9) Dónde mirar en el código

- Handlers HTTP: `src/api/policy_handlers.rs`
- Playground (dominio): `crates/policies/src/features/policy_playground/use_case.rs`
- Traces: `crates/policies/src/features/policy_playground_traces/use_case.rs`
- Analysis: `crates/policies/src/features/policy_analysis/use_case.rs`
- Util paralelo: `crates/policies/src/shared/application/parallel.rs`
