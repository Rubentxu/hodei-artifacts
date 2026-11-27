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

## Reglas operativas

- **Creación sin garantía de idempotencia**: el id incluye `created_at`, quién lo creó y el `ttl`;
  cada `POST /api/v1/freezes` es un freeze distinto, aunque el contenido sea el mismo.
- **Atomicidad del pin**: se colocan primero los holds de contenido, se verifica que los blobs
  existen y solo al final se coloca el hold registro; un freeze "existe" cuando todos sus pines
  están. Cualquier fallo intermedio revierte todos los holds del freeze (el registro, el último);
  si la reversión también falla se registra en el log con el prefijo `freeze-<id>-`.
- **Topes**: como máximo 10 000 rutas por manifiesto (`413`; congele un `prefix` más estrecho) y
  100 freezes activos por cuenta (`409 E_FREEZE_LIMIT`). Los digests que no son sha256 se rechazan
  de forma explícita (`422`).
- **Liberar sin origen**: `unpin` exige que la cuenta del principal sea la dueña (si no, `404`).
  Si el repositorio origen existe, decide Cedar con `ManageDependencyFreeze`. Si el repositorio ya
  no existe o el manifiesto no se puede leer, solo puede liberarlo quien lo creó; cualquier otro
  necesita la liberación administrativa de holds (`/api/v1/gc/holds/release`). El hold registro
  se borra el último, así que una liberación interrumpida se puede repetir.
- **Crear no exige `DownloadAsset`**: basta `ManageDependencyFreeze` sobre el repositorio; quien lo
  tiene puede fijar contenido que quizá no pueda descargar, pero leerlo después sí exige
  `DownloadAsset`. Es una decisión explícita: la acción dedicada es la autoridad sobre el pin.
- **Coste**: leer un freeze hace una consulta puntual del hold registro (`ContentLedger::hold`);
  solo crear recorre los holds para contar los de la cuenta.

## Contrato de despliegue

El bundle de Cedar debe declarar la acción `ManageDependencyFreeze` (appliesTo `Iam::User` sobre
`Artifacts::Repository`; ya está en `config/community` y `config/examples`). No se añade
comprobación en el arranque: un bundle que no la declare **deniega** crear/liberar (fail closed).
Las rutas requieren el servicio de GC (501 si no está).

## Fuera de este tramo

Export/import offline y reconciliación (AC4); Maven y PyPI; repositorios proxy/group; protocolo
nativo del cliente contra el freeze (el packument guardado conserva las URL del origen);
override explícito para servir revocados; listado de freezes; limpieza de holds caducados.

## Time machine: comparar dos freezes (HSP-165, tramo 1)

`GET /api/v1/freezes/diff?from=<id>&to=<id>` explica la diferencia entre dos freezes **del mismo
repositorio y de la misma cuenta**, citando los hechos históricos que cada manifiesto registró
(ruta, paquete, versión, digest, tamaño). No hay un segundo ledger: la historia son los manifiestos.

- **Solo lectura**: no escribe decisiones, concesiones, holds ni freezes; solo el evento de auditoría
  del acceso. La respuesta es función de los dos manifiestos más el estado de hoy de los digests citados.
- **Autorización**: acción Cedar dedicada `Artifacts::Action::"ExploreRepositoryHistory"` sobre el
  repositorio origen, evaluada ahora. `DownloadAsset` o `ManageDependencyFreeze` no la implican.
- **Aislamiento**: un freeze de otra cuenta es `404`, igual que uno inexistente, sea cual sea el lado.
  Dos repositorios distintos son `422`.
- **Cotas**: se listan como máximo 200 diferencias (`total_changes` es exacto, `truncated` avisa) y solo
  se consultan en el almacén los digests listados; cada freeze ya está acotado a 10 000 rutas.
- **Falla cerrado**: si el almacén o el libro de confianza no responden, es `503` sin respuesta parcial.
- **Cobertura honesta**: por digest citado, `availability` es `present` o `gone` (el colector retiró los
  bytes; no se reconstruyen ni se devuelven) y `admitted_now` dice si el canal `trusted` lo admite hoy
  (`null` en canal abierto). Identidad histórica y permiso actual van separados.

Fuera de este tramo: simulación de prohibición con blast radius inverso, sellos de política a una fecha
de corte y selección de snapshot por instante.
