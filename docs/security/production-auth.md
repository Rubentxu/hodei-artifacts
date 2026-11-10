# Autenticación y autorización de producción

HSP-20 conecta el binario/configuración real con un `PrincipalAuthenticator` JWT y un `Authorizer` Cedar validado contra schema. Sin ese bundle el arranque sigue fail-closed.

## Configuración

El bundle exige `auth_issuer`, `auth_audience`, `authorization_policy_path` y `authorization_schema_path`, más **exactamente uno** de `auth_jwt_secret` (HS256) o `auth_jwks_path` (RS256/ES256 con JWKS local):

```toml
[security]
# Opción A: HS256
auth_jwt_secret = "..."
# Opción B: JWKS local (RS256/ES256)
# auth_jwks_path = "config/jwks.json"

auth_issuer = "https://issuer.example"
auth_audience = "hodei-artifacts"
authorization_policy_path = "config/examples/policies.cedar"
authorization_schema_path = "config/examples/schema.json"
```

Variables equivalentes:

```text
HODEI_SECURITY__AUTH_JWT_SECRET
HODEI_SECURITY__AUTH_JWKS_PATH
HODEI_SECURITY__AUTH_ISSUER
HODEI_SECURITY__AUTH_AUDIENCE
HODEI_SECURITY__AUTHORIZATION_POLICY_PATH
HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH
```

- El secreto y la ruta JWKS se redactan/no serializan; los errores no incluyen tokens, secretos ni claves.
- Un bundle incompleto, con ambos métodos o con strings vacíos falla **antes** de abrir la base de datos.
- El bundle Cedar se lee del disco y se valida con `CedarAuthorizer::from_policies` y el schema; una política o schema inválido impide arrancar.
- `config/examples/` contiene un ejemplo mínimo de política y schema; el operador debe adaptarlo a sus cuentas/acciones.

## Flujo

```text
Authorization: Bearer <JWT>
  -> signature (HS256 o JWKS)
  -> iss, aud, exp, sub presentes
  -> sub HRN + account (deben coincidir)
  -> VerifiedPrincipal/VerifiedContext
  -> CedarAuthorizer (policy bundle + schema)
  -> handler de repositorio o admin
```

Los repositorios exigen `Artifacts::Action::"ReadRepository"`/`"CreateRepository"`; la administración exige `Admin::Action::"AccessAdminApi"` con recurso `AdminApi/<método>-<ruta>` (por ejemplo `get-schemas-load`). El gate administrativo solo se abre con `admin_api_enabled=true` y este bundle inyectado.

## Rotación

- Con HS256: el secreto se lee al arrancar. Para rotar, despliega el nuevo secreto y reinicia; no hay recarga en caliente ni ventana de doble secreto. Planifica la rotación antes de la expiración de los tokens emitidos o emite tokens nuevos tras el reinicio.
- Con JWKS: publica la nueva clave en el JWKS con un `kid` distinto antes de emitir tokens con ella; el verificador elige por `kid`. Retira la clave antigua cuando sus tokens hayan expirado.
- Ambos métodos requieren reiniciar el proceso para releer el fichero.

## Verificación

`tests/production_auth.rs` (9 tests):

1. bootstrap HS256 + Cedar: POST repositorio `201`, GET `200`, admin `200`;
2. token válido sin política → `403`;
3. audiencia incorrecta, caducado y firma corrupta → `401`;
4. configuración parcial → bootstrap falla;
5. `sub` de otra cuenta con claim `acme` → `401`;
6. admin habilitado sin token → `401`;
7. JWKS RS256 autentica y HS256 se rechaza con JWKS configurado;
8. issuer incorrecto y política Cedar inválida → rechazo/fallo de bootstrap;
9. secreto vacío y schema inválido → bootstrap falla.

## Límites

- No hay descubrimiento OIDC remoto ni recarga de JWKS; el JWKS es local y se lee al arrancar.
- El binario sin bundle sigue fail-closed para no romper desarrollo/tests.
- El almacén de repositorios sigue en memoria (HSP-21).
