# Wiring del Authorizer compartido

HSP-13 conecta el Authorizer de `hodei-authz` con Artifacts mediante un puerto pequeño en el CompositionRoot, sin crear un segundo motor de autorización.

## Flujo

```text
Axum (HSP-8) -> VerifiedContext + ActionId + ResourceRef
             -> AuthorizeResourceUseCase
             -> same-account check
             -> AuthorizerPort
             -> SharedAuthorizer (hodei-authz / Cedar)
```

- `AuthorizeResourceUseCase` recibe un `VerifiedContext`; **no** acepta headers ni un tenant del cliente. `execute_canonical` convierte el `kernel::Hrn` campo a campo, conservando paths anidados que el parser legacy 0.1 rechazaría.
- Antes de evaluar, exige que la cuenta del principal y la del recurso coincidan (`ScopeMismatch`).
- `AuthorizerPort` es el puerto de aplicación; `SharedAuthorizer` adapta el trait `hodei_authz::Authorizer`.
- `DenyAllAuthorizer` es el adaptador de compatibilidad: deniega hasta que se inyecte un authorizer real. `CompositionRoot::with_authorization` permite sustituirlo de forma reversible.

## CompositionRoot

`CompositionRoot` expone `authorization` y lo propaga a `AppState` junto con el caso de uso `authorize_resource`. El constructor de producción usa `DenyAllAuthorizer` por defecto; HSP-14 inyectará el bundle de schema/políticas y HSP-15 expondrá la ruta real.

## Pruebas

`src/authorization.rs` prueba, sin persistencia:

- **Permit** con un `CedarAuthorizer` real y `HrnCedarMapper`, incluido un path anidado vía `execute_canonical`.
- **Deny** con el adaptador de compatibilidad.
- **Indeterminate** con una política que consulta un atributo ausente.
- **Cross-account** denegado antes de evaluar, incluso con una política laxa.
- Sustitución reversible entre shared y deny-all.

`composition_root` prueba que el authorizer es inyectable y que `AppState` conserva el puerto y el caso de uso.

## Límites

- HSP-15 monta `GET /api/v1/repositories?hrn=...` y usa este caso de uso; la ruta tiene su propio gate y no depende de `admin_api_enabled`.
- El bundle real de schema/políticas es HSP-14; el evaluador IAM existente sigue en `AppState` y su reconciliación queda pendiente.
- La dependencia con `hodei-authz` es la versión publicada `=0.1.1` desde HSP-18; el `Cargo.lock` versionado fija la resolución.
- El `AppState` ya expone `authorize_resource`; el handler de repositorios lo invoca con el `VerifiedContext` del bearer verificado.
- Este repo **no monta** un middleware de autorización de Axum: la ruta de repositorios hace la verificación en el handler. El contrato es el de `hodei-authz`/HSP-8.
- El authenticator productivo de la ruta sigue pendiente: bootstrap usa `DenyAllAuthenticator` y la ruta responde `401` hasta inyectar uno real.
