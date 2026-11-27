# Dependency Freeze: vista inmutable para resolución reproducible (HSP-161, tramo 1)

Un freeze es un **manifiesto** `(ruta, paquete, versión) -> digest` de un repositorio
alojado (`raw` o `npm`), con identidad igual al digest de sus propios bytes. No es un
lockfile ni una copia: los bytes siguen en el CAS bajo su digest.

| Operación | Ruta | Autorización |
| --- | --- | --- |
| Crear | `POST /api/v1/freezes` `{repository, prefix?, ttl_secs?}` | acción Cedar dedicada `Artifacts::Action::"ManageDependencyFreeze"` sobre el repositorio |
| Liberar | `POST /api/v1/freezes/unpin` `{id}` | `ManageDependencyFreeze` |
| Manifiesto | `GET /api/v1/freezes/{id}` | `DownloadAsset` sobre el repositorio origen |
| Contenido | `GET /api/v1/freezes/{id}/content/{*ruta}` | `DownloadAsset` sobre el repositorio origen |

## Garantías

- **Vista inmutable**: la lectura sale solo del manifiesto. Mover `latest` o publicar versiones
  nuevas en el origen no cambia lo que sirve el freeze (mismos bytes y digests).
- **Ausente es error explícito**: una ruta que el manifiesto no nombra es
  `404 {"error":"E_MISSING_DEPENDENCY"}`; nunca cae a `latest` ni al repositorio vivo.
- **Retirado o revocado no se sirve**: en un repositorio de canal `trusted`, un digest sin
  concesión activa hoy es `403 E_WITHHELD`; al crear, lo no admitido ni se congela. La identidad
  (el manifiesto) es histórica; el permiso es el de ahora.
- **Pin de GC**: un hold por digest más un hold registro `freeze-<id>-manifest`, en el mismo libro
  que ya lee el colector. Liberar quita solo los holds de ese freeze; el censo general decide
  después con las demás referencias (raíces de catálogo y holds de otros freezes). Con `ttl_secs`
  los holds caducan solos y la lectura responde `410 E_FREEZE_EXPIRED`.
- Un blob subido por un usuario con forma de manifiesto no es un freeze: solo lo es si existe el
  hold registro, que solo escribe este módulo.

## Contrato de despliegue

El bundle de Cedar debe declarar la acción `ManageDependencyFreeze` (appliesTo `Iam::User` sobre
`Artifacts::Repository`; ya está en `config/community` y `config/examples`). No se añade
comprobación en el arranque: un bundle que no la declare **deniega** crear/liberar (fail closed).
Las rutas requieren el servicio de GC (501 si no está).

## Fuera de este tramo

Export/import offline y reconciliación (AC4); Maven y PyPI; repositorios proxy/group; protocolo
nativo del cliente contra el freeze (el packument guardado conserva las URL del origen);
override explícito para servir revocados; listado de freezes; limpieza de holds caducados.
