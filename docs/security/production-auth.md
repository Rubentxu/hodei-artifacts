# Autenticación y autorización de producción

HSP-20 conecta el binario/configuración real con un `PrincipalAuthenticator` JWT y un `Authorizer` Cedar validado contra schema. Sin ese bundle el arranque sigue fail-closed.

## Configuración

Las cinco claves deben configurarse juntas; un bundle parcial es un error de arranque:

```toml
[security]
auth_jwt_secret = "..."              # HS256; nunca se registra
auth_issuer = "https://issuer.example"
auth_audience = "hodei-artifacts"
authorization_policy_path = "config/policies.cedar"
authorization_schema_path = "config/schema.json"
```

Variables equivalentes:

```text
HODEI_SECURITY__AUTH_JWT_SECRET
HODEI_SECURITY__AUTH_ISSUER
HODEI_SECURITY__AUTH_AUDIENCE
HODEI_SECURITY__AUTHORIZATION_POLICY_PATH
HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH
```

- `JwtAuthenticator` valida firma HS256, `iss`, `aud`, `exp`, `sub` y `account`.
- El `sub` debe ser un HRN canónico; su cuenta debe coincidir con el claim `account` (y `verify_principal` lo vuelve a comprobar). Una discrepancia se rechaza con `401`.
- El bundle Cedar se lee del disco y se valida con `CedarAuthorizer::from_policies` y el schema declarado; una política inválida impide arrancar.
- El `Debug` de `SecurityConfig` redacta el secreto; los errores no incluyen tokens ni el secreto.

## Flujo

```text
Authorization: Bearer <JWT>
  -> JWT signature/iss/aud/exp
  -> sub HRN + account
  -> VerifiedPrincipal/VerifiedContext
  -> CedarAuthorizer (policy bundle + schema)
  -> handler de repositorio o admin
```

Los repositorios exigen `Artifacts::Action::"ReadRepository"`/`"CreateRepository"`; la administración exige `Admin::Action::"AccessAdminApi"` con recurso `AdminApi/<método>-<ruta>`. El gate administrativo solo se abre con `admin_api_enabled=true` y este bundle inyectado.

## Verificación

`tests/production_auth.rs` (6 tests):

1. bootstrap con JWT+Cedar: POST repositorio `201`, GET `200`, admin `200`;
2. token válido sin política → `403`;
3. audiencia incorrecta, caducado y firma corrupta → `401`;
4. configuración parcial → bootstrap falla;
5. `sub` de otra cuenta con claim `acme` → `401`;
6. admin habilitado sin token → `401`.

## Límites

- Esta entrega usa HS256 con secreto compartido. Un proveedor OIDC con JWKS/RS256 requiere un adaptador adicional; la frontera `PrincipalAuthenticator` ya está preparada.
- El binario de ejemplo sigue arrancando sin bundle (fail-closed) para no romper desarrollo/tests; un despliegue productivo debe configurar las cinco claves.
- La rotación del secreto es responsabilidad operativa; no hay recarga en caliente.
