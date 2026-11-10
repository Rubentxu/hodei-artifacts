# Lectura y creación autorizadas de repositorios

HSP-15 expone el primer vertical slice productivo: `GET /api/v1/repositories?hrn=<HRN>`.

## Flujo

```text
Authorization: Bearer <token>
  -> verify_principal (PrincipalAuthenticator)
  -> account del principal verificado
  -> HRN canónico de la query
  -> 404 uniforme si la cuenta del HRN no es la del principal
  -> AuthorizeResourceUseCase (same-account/partition + Authorizer compartido)
  -> 403 si Deny/Indeterminate
  -> RepositoryReader::find_by_hrn(cuenta_verificada, HRN)
  -> 404 si no existe
  -> 200 RepositoryView
```

- La ruta tiene **su propio gate**; no depende de `admin_api_enabled`.
- La cuenta usada para buscar es la del principal verificado, nunca la del cliente; un HRN de otra cuenta devuelve el mismo `404` que uno inexistente.
- La ruta exige forma canónica de repositorio (`partition=hodei`, `service=artifacts`, `resource_type=repository`); un HRN de otro servicio/tipo no se evalúa y devuelve `404`.
- Desviación declarada del baseline: una identidad válida sin permiso devuelve `403`, no `404`; la denegación ocurre antes de leer y no revela existencia.
- Token ausente/inválido → `401` (con `WWW-Authenticate: Bearer`).
- Identidad válida sin permiso o `Indeterminate` → `403`.
- HRN malformado → `400`.
- Lector no disponible → `503`.
- La respuesta es `RepositoryView` (`hrn`, `account`, `name`, `description`); no incluye credenciales ni datos de almacenamiento.

## Crear un repositorio

`POST /api/v1/repositories` con `Authorization: Bearer` y:

```json
{"name": "releases", "description": "optional", "region": "eu"}
```

- La autenticación precede a la deserialización del body: sin token, un JSON inválido sigue siendo `401`.
- La cuenta sale del principal verificado; el cuerpo **no** admite `account` (`deny_unknown_fields`, rechazo `400`).
- `name` se valida (no vacío, ≤128, `[A-Za-z0-9._-]`, sin `/` ni `.`/`..`); `description` ≤1024; `region` por defecto `global` y limitada a minúsculas/dígitos/`-`.
- `AuthorizeResourceUseCase` evalúa `CreateRepository` sobre el HRN canónico de la cuenta del principal; `Deny`/`Indeterminate` → `403` sin escritura.
- El `RepositoryWriter` hace comprobación de unicidad e inserción bajo **un único lock** (sin TOCTOU). Éxito → `201` + `RepositoryView` con HRN; duplicado → `409`; fallo → `503`.
- El lector y el escritor comparten el mismo store en el slice; dos peticiones concurrentes equivalentes producen exactamente una entidad.

## Implementación del slice

- `RepositoryReader` es el puerto de lectura; `RepositoryWriter` el de creación. `InMemoryRepositoryStore` implementa ambos con un lock único y comprobación de unicidad atómica; `InMemoryRepositoryReader` sigue disponible para escenarios de solo lectura.
- `AppState` inyecta authenticator/reader con `with_repository_auth` y el writer con `with_repository_writer`; el bootstrap crea **un único** `InMemoryRepositoryStore` compartido por ambos puertos, con `DenyAllAuthenticator` hasta configurar un adaptador real.
- `AuthorizeResourceUseCase::execute_canonical` convierte el `kernel::Hrn` sin perder paths anidados.
- La ruta se monta fuera del router administrativo (`admin_v1_routes`), en `repository_v1_routes`.

## Pruebas

`tests/repository_create.rs` (14 tests): create autorizado + readback, body con cuenta rechazado, 401/403/Indeterminate sin escritura, duplicado 409 conservando el original, nombres inválidos, 503 del writer, concurrencia multi-hilo sobre el router y sobre el store atómico, y verificación de que bootstrap comparte reader/writer.

`tests/repository_read.rs` (13 tests in-process con router/config reales): permit con readback, 401 ausente/inválido, 403 sin permiso y recurso de la misma cuenta no permitido, 404 ajeno/inexistente/no-repositorio con cuerpo uniforme, 400 malformado/sin query, 503 del reader, `WWW-Authenticate`, Indeterminate y ausencia de llamadas al reader en rechazos.

`tests/admin_surface.rs` fija que la ruta sigue respondiendo `401` con `admin_api_enabled` true/false: su gate no es el flag administrativo.

## Límites

- `InMemoryRepositoryStore` es el adaptador del slice; la persistencia real (SurrealDB) sigue pendiente. No se reactiva el crate legacy.
- `POST` y `GET` comparten auth/authorizer; HSP-20 conecta JWT HS256 o JWKS RS256/ES256 en el bootstrap cuando se configura el bundle de seguridad.
- Sin bundle, el bootstrap sigue fail-closed (`401`); ver [production-auth.md](production-auth.md).
- El store de creación es en memoria; HSP-18/adaptador de persistencia lo sustituirá.
- El contrato OpenAPI estático histórico (`/v1/repositories` por UUID) no se ha reconciliado con esta ruta; HSP-18/contrato lo harán.
