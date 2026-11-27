# Self-healing del CAS: reparar un blob desde una fuente verificada (HSP-164, tramo mínimo)

El scrub (`GET /scrub`) **detecta** un blob corrupto o ausente. Este tramo lo **repara**: devuelve
al CAS los bytes correctos desde una fuente que el operador configuró, solo si esos bytes hashean
al digest pedido.

| Operación | Ruta | Autorización |
| --- | --- | --- |
| Reparar | `POST /api/v1/repair` `{repository, digest}` | acción Cedar dedicada `Artifacts::Action::"ManageStorageRepair"` sobre el repositorio |

Respuestas: `200 {"outcome":"repaired"|"already_sound", "digest", ...}`; `404` repositorio o digest
no referenciado por ese repositorio de la cuenta (indistinguible de la ausencia); `403` sin la
acción; `409 E_REPAIR_UNAVAILABLE` la fuente no tiene copia; `422 E_REPAIR_SOURCE_MISMATCH` la
fuente devuelve bytes que no hashean al digest (no se escribe nada); `413 E_REPAIR_TOO_LARGE`
supera el tope; `501` sin fuente configurada o almacén sin cuarentena condicional;
`503 E_REPAIR_FAILED` fallo de fuente o de almacén. Cada intento se audita (`Allowed`/`Denied`/`Failed`).

## Garantías

- **La fuente es del despliegue, no de la petición.** `content.repair_source_path` (variable
  `HODEI_CONTENT__REPAIR_SOURCE_PATH`) apunta a un directorio existente con el formato del CAS; el
  servicio solo lo lee. Sin él no hay reparación (501).
- **Verificación antes de escribir.** Los bytes de la fuente se hashean en el servicio y deben
  igualar el digest; si no, se rechazan sin tocar el almacén.
- **Un blob sano no se sobrescribe**: si `verify` pasa, la respuesta es `already_sound` y no se lee
  la fuente. Reparar dos veces es reparar una vez.
- **El blob dañado no se borra**: se mueve a un nombre de cuarentena (`quarantine/repair-*`) solo si
  sigue siendo la versión observada (marca condicional); tras escribir se vuelve a verificar y solo
  entonces se informa de reparación.
- **Aislamiento por cuenta**: el repositorio es de la cuenta verificada y el digest debe estar
  referenciado por una fila de ese repositorio (lectura paginada del libro, se detiene al primer
  acierto).
- **Costes acotados**: una reparación a la vez y `content.repair_max_bytes` (256 MiB por defecto)
  comprobado antes de mover bytes.

## Fuera de este tramo

Recomendación de tiering (por frecuencia/recencia/tamaño/coste) y su ejecución autorizada; disparo
automático desde el scrub o el runtime reactivo; fuentes S3 o remotas; reparación de varios blobs
por lote. Requieren decisión de producto (qué tiers existen, quién autoriza un placement).
