# Migración verificable de blobs entre backends (HSP-163, tramo mínimo)

Copia blobs del CAS en servicio a un **segundo almacén** que el operador configuró (otro
directorio, otro volumen o pool), verificando cada copia por re-hash. Es el primer paso de una
migración de backend: **copiar y verificar**. No cambia hashes, coordenadas, grants ni clientes, y
**nunca borra ni escribe el origen**.

| Operación | Ruta | Autorización |
| --- | --- | --- |
| Migrar | `POST /api/v1/storage/migrate` `{repository, digests:[...]}` (1..64) | acción Cedar dedicada `Artifacts::Action::"ManageStorageMigration"` sobre el repositorio |

La respuesta es siempre el **informe**: `entries[]` (`digest` y `outcome` por digest, en el orden
pedido y sin duplicados), `migrated`, `already_present`, `pending`, `bytes_copied`,
`source_retained: true`, `elapsed_ms`. Resultados por digest: `migrated`, `already_present`,
`source_absent`, `source_corrupt`, `source_failed`, `too_large`, `deferred`, `destination_damaged`,
`destination_failed`.

Códigos: `200` todo lo pedido queda verificado en el destino; `207` parte sí y parte no (el informe
dice cuál); `503` nada verificado y falló un almacén; `409` nada verificado por otra causa (origen
ausente o corrupto, blob demasiado grande); `404` repositorio de otra cuenta o digest no referenciado
por ese repositorio (indistinguible de la ausencia, **y no se mueve nada**, ni los digests válidos
de la misma petición); `403` sin la acción; `400` petición mal formada o más de 64 digests;
`429 E_MIGRATE_BUSY` ya hay 2 migraciones en curso o esa cuenta ya tiene una; `504` venció el plazo de la petición (lo hecho queda en el informe, lo demás `deferred`); `501` sin destino configurado. Cada intento
tras Cedar se audita **dentro de la tarea de migración** (aunque el cliente cuelgue queda la traza): `Allowed` si todo quedó verificado, `Conflict` si fue parcial (207/504 con algo verificado), `Failed` si nada quedó verificado, `Denied` digest ajeno. El recurso lleva `repositorio[n]`, el recuento por estado, los bytes y `set=<16 hex>` (hash del conjunto de digests; el campo de auditoría está acotado a 256 caracteres).

## Garantías

- **El destino es del despliegue, no de la petición.** `content.migration_destination_path`
  (`HODEI_CONTENT__MIGRATION_DESTINATION_PATH`) apunta a un directorio **existente** (un error
  tipográfico nunca crea un almacén) con el formato del CAS. Debe ser distinto del `cas_path` y de
  `repair_source_path`, y ninguno puede contener a otro; si no, el servicio no arranca.
- **No se propaga lo dañado.** Los bytes del origen se hashean en el servicio y solo se escriben si
  igualan el digest (`source_corrupt` si no; el remedio es `POST /api/v1/repair`, que no se estorba
  con la migración: ninguna borra ni pone en cuarentena en el destino).
- **Verificada por lectura.** Tras escribir, el destino se re-verifica; solo entonces el digest es
  `migrated`. Un destino que confirma sin guardar, o que falla, queda `destination_failed`.
- **El origen nunca se borra ni se escribe.** No hay todavía un interruptor de lectura por digest:
  borrar la copia de origen quitaría el blob al almacén que lo sirve. Retirar el origen es un paso
  posterior y explícito que este tramo no ofrece. Consecuencia: GC y holds no interactúan con la
  migración (no hay borrado que proteger); la GC solo conoce el almacén de origen.
- **Idempotente y reanudable.** Lo que el destino ya verifica es `already_present` (no se lee el
  origen). Un fallo a medias deja el origen intacto y, a lo sumo, copias verificadas o ausentes:
  repetir la petición completa lo que falte. Una copia ya presente pero dañada en el destino se
  deja como está (`destination_damaged`, falla cerrado; no se sobrescribe ni se borra).
- **Costes acotados.** 64 digests por petición; **1 migración simultánea por cuenta** y 2 en total
  (una cuenta no acapara las plazas); **plazo total por petición de 5 min**: pasado el plazo no se
  empieza nada más, lo hecho se informa, lo demás queda `deferred` y la respuesta es `504` (la plaza
  se libera siempre, al terminar la tarea); las referencias de todos los digests se comprueban en
  **una sola pasada** del libro, tras tomar la plaza y con timeout de 30 s (fallo cerrado, 503); `content.migration_max_blob_bytes` (256 MiB, >0) por
  blob, sin leerlo si lo supera (`too_large`); `content.migration_max_request_bytes` (1 GiB, >=
  el anterior) por petición: lo que no cabe es `deferred` y se pide de nuevo; 2 migraciones
  concurrentes como mucho; timeout de 30 s en cada llamada a un almacén; la tarea es inmune a que el
  cliente cuelgue.
- **Aislamiento por cuenta**: el repositorio es de la cuenta verificada y todos los digests deben
  estar referenciados por una fila de ese repositorio.

## Límites conocidos (documentados, no resueltos)

- **Retención en el destino fuera de GC/holds.** La GC solo conoce el almacén de origen: las copias
  del destino no se barren, no las protege un hold y no se liberan solas. Es el operador quien
  gestiona su ciclo de vida (retirarlas, o adoptarlas como nuevo backend).
- **El `verify` previo del destino no tiene tope de tamaño.** Para saber si un digest ya está
  verificado se re-hashea la copia existente del destino, aunque supere
  `migration_max_blob_bytes`; el tope solo protege la lectura del origen. Es coste de E/S acotado
  por el plazo de la petición.
- **Symlinks y hardlinks del destino.** El destino es un directorio del operador: el servicio no
  defiende frente a enlaces simbólicos o duros colocados dentro de él. Quien lo prepara es
  responsable de su contenido y permisos (mismo supuesto que `repair_source_path`).
- **Memoria.** Cada blob se lee entero en memoria (el `BlobStore` no verifica en streaming): como
  mucho 2 migraciones x `migration_max_blob_bytes` (2 x 256 MiB por defecto) más la copia hasheada.
  Ajuste `migration_max_blob_bytes` a la RAM disponible.

## Alcance de perfil

Solo el perfil de grafo cablea la migración. El destino es un directorio de ficheros; el origen es
el backend de contenido configurado (filesystem o S3).

## Fuera de este tramo (decisiones de producto pendientes)

Journal de pools y switch de lectura por digest (necesarios para retirar el origen con fencing frente
a la GC y para rollback), varios miembros por pool, políticas de residencia/colocación por cuenta,
destino S3, selección de «todo el repositorio» sin enumerar digests, medición de RPO/RTO y coste/s.
