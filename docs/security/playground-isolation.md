# Aislamiento del playground

El playground evalúa políticas inline y schemas inline o versionados con fines de simulación. **No es la autoridad para ejecutar acciones de negocio.**

## Separación

| Camino | Identidad | Persistencia | Autoridad |
| --- | --- | --- | --- |
| `POST /api/v1/playground/evaluate` | Principal ficticio del payload | No escribe políticas | Simulación |
| `AuthorizeResourceUseCase` | `VerifiedContext` (HSP-8/15) | No persiste | Decisión productiva |

- El endpoint está bajo el gate administrativo: `admin_api_enabled=false` devuelve `403` antes del handler.
- El playground recibe `inline_policies`, `inline_schema`/`schema_version` y un request ficticio; el caso de uso no depende de un almacén de políticas, por lo que no puede crear ni modificar políticas operativas.
- El caso de uso productivo recibe `VerifiedContext`; un `principal_hrn` arbitrario no es un parámetro válido (doctest `compile_fail` en `AuthorizeResourceUseCase::execute`).
- El playground puede devolver `ALLOW` para una simulación explícita, pero ese resultado no se reutiliza para autorizar una acción real. El caso de uso productivo (`AuthorizeResourceUseCase`) usa el Authorizer compartido y el contexto verificado; su ruta HTTP llega con HSP-15.

## Fallos seguros

- Sin políticas: el comando se rechaza con `400`; no hay decisión `ALLOW`.
- Schema incongruente: los errores de validación **fuerzan `DENY`** y se reportan, aunque la condición Cedar casara.
- `schema_version` inexistente: error de carga (`500` en el handler actual); no se degrada a schema vacío.
- `admin_api_enabled=true` no autentica; el playground sigue siendo una herramienta administrativa de un despliegue controlado.

## Pruebas

- `tests/playground_isolation.rs`: gate por defecto, evaluación inline que no altera `GET /api/v1/iam/policies` (almacén vacío: detecta creación neta; el caso de uso no tiene puerto de escritura), comando sin políticas y mismatch de schema sin `ALLOW`.
- `tests/schema_persistence.rs` S4/S5: registro versionado, carga por versión, evaluación reproducible y política inválida contra el schema que no concede.
- `src/authorization.rs`: doctest `compile_fail` que impide pasar un principal string al caso de uso productivo.

## Límites

- El playground es una herramienta administrativa, no un entorno multiusuario aislado por principal.
- `POST /api/v1/iam/evaluate` es una evaluación administrativa sobre políticas persistidas con un principal del payload; no ejecuta operaciones. No es la autoridad productiva; HSP-15 cablea la ruta que usará `AuthorizeResourceUseCase`.
- No hay una autoridad productiva cableada todavía: hay evaluación/simulación administrativa y el caso de uso tipado. HSP-15 los conecta y HSP-14 deja la reconciliación pendiente.
- La autenticación del principal productivo llega con HSP-15; hasta entonces el caso de uso se prueba con un authenticator de test.
- La reconciliación del schema/políticas productivas con el evaluador IAM es parte de HSP-14/HSP-15.
