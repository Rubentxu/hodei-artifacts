# Congelación de publicaciones (freeze) en repositorios npm y raw

HSP-322 tramo 2, desde HSP-161 (Dependency Freeze). Un repositorio **hospedado
npm o raw** puede declarar una congelación en `config_settings.freeze`. Mientras
está vigente, el repositorio sirve exactamente lo que ya contiene: toda
publicación se rechaza **antes** de escribir un solo byte.

```json
"freeze": { "scope": "all", "reason": "ventana de auditoria 4.2", "until": "2026-12-01T00:00:00Z" }
```

| Campo | Significado |
| --- | --- |
| `scope` | **Obligatorio.** `all`: rechaza toda escritura (versión nueva, paquete nuevo, dist-tags, metadatos; en raw, ruta nueva o reemplazo). `existing`: rechaza cambios sobre lo ya publicado y admite nombres/rutas **nuevos**. |
| `reason` | **Obligatorio** (1 a 200 caracteres). Solo para el operador: va al log y no sale en la respuesta. |
| `until` | Opcional, RFC 3339. A partir de ese instante (exclusivo) la congelación deja de aplicar sola. Sin `until`, dura hasta que se descongela (`DELETE`, ver abajo). |

## Decisión de seguridad

- **Sin declaración, comportamiento intacto**: sin puerta y sin eventos.
- Rechazo: **409** con motivo genérico fijo («this repository does not accept
  publications at this time»); ni el motivo declarado ni el ámbito salen del
  servicio. Evento de auditoría `freeze.refused` (denied), recurso
  `repositorio/nombre-o-ruta`.
- **Nunca falla abierto**: una declaración ilegible (forma, ámbito, motivo,
  `until`, campos desconocidos) responde **503**, no publica nada y deja
  `freeze.unavailable` (failed).
- Las lecturas siguen sirviendo el conjunto fijado.
- La importación de migración **se niega** a escribir en un repositorio que
  declare una congelación (no pasa por esta puerta y sería una puerta trasera).

## Quién puede congelar y descongelar

Congelar es una palanca de denegación de publicación, así que tiene su propia
acción Cedar: **`Artifacts::Action::"ManageRepositoryFreeze"`**
(`actions::manage_repository_freeze`). `CreateRepository` y `UploadAsset` no la
implican: un principal que solo puede crear repositorios y publicar recibe 403
al intentar declararla y no se crea nada. La declaración ilegible o en un
formato no soportado es 400.

La declaración se hace **al crear** el repositorio (`POST /api/v1/catalog/repositories`
con `settings.freeze`) o después, con la API propia de la congelación, siempre
bajo `ManageRepositoryFreeze`:

- `PUT /api/v1/catalog/repositories/{nombre}/freeze` con el cuerpo
  `{scope, reason, until?}`: declara o **reemplaza** la congelación. Una
  declaración ilegible es 400 y no cambia nada. Respeta el resto de
  `config_settings`.
- `DELETE /api/v1/catalog/repositories/{nombre}/freeze`: **descongela**. Es
  idempotente: sin congelación responde 200 con `changed: false` y no escribe.

Ambas rutas buscan el repositorio en la cuenta del principal verificado (el de
otra cuenta es el mismo 404 que uno inexistente), exigen Cedar (403 auditado;
indeterminado también es 403) y solo existen para repos hospedados npm/raw (400
en otro caso). Cada cambio deja un evento de auditoría con el recurso
`<hrn> freeze=set` o `<hrn> freeze=released`. Quien solo publica o crea
repositorios no puede congelar ni descongelar. Además se levanta por caducidad
(`until`).

`DELETE` de assets en raw (HSP-323 tramo h) está **bloqueado** por la
congelación (en ambos ámbitos): borrar una ruta cambia lo que sirve el
repositorio. Responde 409 con el mismo motivo genérico y `freeze.refused`.

## Cambio del contrato de despliegue

**Sí cambia.** El esquema Cedar del despliegue debe declarar la acción
`ManageRepositoryFreeze` (aplica a `Iam::User` sobre `Artifacts::Repository`):
el arranque lo comprueba (`check_service_declared`) y se niega a arrancar si
falta. Los esquemas `config/community/schema.json` y
`config/examples/schema.json` ya la incluyen; los esquemas propios de cada
despliegue deben añadirla. Las políticas deben concederla de forma explícita a
quien pueda congelar; sin concesión, nadie puede declarar congelaciones.

## Límites conocidos

- Es una **congelación de publicación**, no el snapshot reproducible de HSP-161
  (identidad por digest de manifest, pin de GC, export offline y resolución
  desde el freeze): eso no está implementado aquí.
- Solo cubre repos hospedados **npm** y **raw** (en otros formatos o modos la
  declaración es 400). No cubre promoción entre repositorios ni otros formatos.
- `unpublish` de npm ya se rechaza siempre; no se toca.
- La actualización lee y reescribe `config_settings` completo (`update_config`, sin
  versión esperada): dos cambios de configuración simultáneos del mismo repositorio
  pueden pisarse. Hoy no hay otro endpoint que cambie la configuración.
- La restauración de recuperación (`recovery::import`) no pasa por esta puerta.
- La hora es la del servicio (`state.identity.now()`); con varios nodos, la
  deriva de reloj entre ellos mueve el instante efectivo de `until`.
