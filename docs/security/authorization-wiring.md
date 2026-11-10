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

- No hay ruta HTTP nueva todavía; HSP-15 la expone.
- El bundle real de schema/políticas es HSP-14.
- La dependencia con `hodei-authz` está pinneada por git (`rev = 16d1fba`) para que el CI de este repo aislado pueda resolverla; HSP-18 la sustituye por la publicada.
- El `AppState` aún no tiene un handler que use `authorize_resource`; HSP-15 expone la ruta y su productor de `VerifiedContext`.
- Este repo **no monta todavía** un middleware de autorización (el router solo tiene admin gate/CORS/trazas). El contrato es el de `hodei-authz`/HSP-8; el caso de uso será la autoridad cuando HSP-15 exponga la ruta y la decisión dependa del estado del recurso.
- El evaluador IAM existente sigue en `AppState`; HSP-14 reconcilia schema/políticas y evita que emita una decisión distinta de la del Authorizer compartido.
- El productor real de `VerifiedContext` llega con HSP-15; hoy los tests construyen uno con un authenticator de prueba.
