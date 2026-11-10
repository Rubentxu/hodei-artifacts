# Superficie administrativa autenticada

`admin_api_enabled` ya no es un bypass: es un opt-in que solo abre la administración si el `AppState` tiene **authenticator y authorizer** configurados. En cualquier otro caso la superficie permanece cerrada.

## Matriz de decisión

| Estado | Petición anónima | Token inválido | Token válido sin permiso | Token válido con `permit` |
| --- | --- | --- | --- | --- |
| `admin_api_enabled=false` | `403 administrative API disabled` | `403` | `403` | `403` |
| `true`, sin authenticator/authorizer | `403 administrative API requires authentication` | `403` | `403` | `403` |
| `true`, configurados | `401` + `WWW-Authenticate: Bearer` | `401` | `403 administrative permission denied` | handler |

- El gate usa el mismo `PrincipalAuthenticator` y `AuthorizeResourceUseCase` que las rutas de repositorios. La cuenta sale del `VerifiedContext`; no se acepta identidad por cabeceras.
- **Migración HSP-20:** el recurso pasó de `AdminApi/<método>/<ruta>` a `AdminApi/<método>-<ruta>` para que la jerarquía Cedar no exija ancestros no declarados. Las políticas con el formato anterior deben actualizarse; no hay ventana de compatibilidad dual porque el formato previo no se desplegó en producción.
- Acción: `Admin::Action::"AccessAdminApi"`. Recurso: `hrn:hodei:admin:global:<cuenta>:AdminApi/<método>-<ruta-con-guiones>`; incluir el método evita que un `permit` de listado habilite creación sobre la misma ruta. Una política puede restringir método y ruta (`get-schemas-load`, `post-playground-evaluate`, etc.). El path se codifica en un solo segmento para que la jerarquía Cedar no exija ancestros no declarados en el schema.
- El arranque emite un `error!` si `admin_api_enabled=true` y falta la integración; `bootstrap` deja el estado no listo y el gate responde `403`.
- Healthchecks, Swagger y OpenAPI siguen públicos; no ejecutan handlers administrativos.
- El preflight CORS sigue respondiendo en la capa exterior.

## Configuración

```toml
[security]
# Solo tiene efecto con authenticator y authorizer inyectados en AppState.
admin_api_enabled = false
```

`HODEI_SECURITY__ADMIN_API_ENABLED=true` no autentica por sí solo. Sin el bundle de seguridad de HSP-20 el binario sigue con `DenyAllAuthenticator` y la API administrativa permanece cerrada (`403`); con JWT/JWKS y Cedar configurados, el gate aplica la matriz anterior.

## Pruebas

`tests/admin_surface.rs` (15 tests) cubre:

- cerrado por defecto y cerrado con flag activo sin integración;
- `401` + challenge para anónimo con integración configurada;
- `403` para identidad válida sin permiso y para un `permit` de otro método/ruta;
- `200`/handler alcanzable para alice con `permit` en schemas y playground; flag false con auth configurada sigue cerrado;
- healthchecks, CORS y la ruta de repositorios con su gate independiente.
