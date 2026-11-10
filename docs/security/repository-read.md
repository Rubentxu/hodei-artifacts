# Lectura autorizada de repositorios

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

## Implementación del slice

- `RepositoryReader` es un puerto read-only; `InMemoryRepositoryReader` es el adaptador determinista del slice y de los tests.
- `AppState` inyecta el authenticator y el reader con `with_repository_auth`; `CompositionRoot`/bootstrap usan `DenyAllAuthenticator` y un reader vacío hasta que se configure un adaptador real.
- `AuthorizeResourceUseCase::execute_canonical` convierte el `kernel::Hrn` sin perder paths anidados.
- La ruta se monta fuera del router administrativo (`admin_v1_routes`), en `repository_v1_routes`.

## Pruebas

`tests/repository_read.rs` (7 tests in-process con router/config reales):

1. permit → `200` con el repositorio real y el reader llamado una vez;
2. token ausente/inválido → `401` sin reader;
3. identidad válida sin permiso → `403` sin reader;
4. recurso de otra cuenta → `404` sin reader ni filtración;
5. repositorio inexistente en la cuenta → `404` tras consultar el reader;
6. HRN malformado → `400` sin reader;
7. authorizer `Indeterminate` → `403` sin reader.

`tests/admin_surface.rs` fija que la ruta sigue respondiendo `401` con `admin_api_enabled` true/false: su gate no es el flag administrativo.

## Límites

- `InMemoryRepositoryReader` es el adaptador del slice; la persistencia/escritura real es HSP-16. No se reactiva el crate legacy.
- No hay JWT/OIDC productivo todavía: el authenticator se inyecta; el bootstrap por defecto deniega todo (`401`).
- El contrato OpenAPI estático histórico (`/v1/repositories` por UUID) no se ha reconciliado con esta ruta; HSP-18/contrato lo harán.
