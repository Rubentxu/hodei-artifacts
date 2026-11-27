# Migración verificable de blobs entre backends (HSP-163, tramo mínimo)

Copia blobs del CAS en servicio a un **segundo almacén** que el operador configuró (otro
directorio, otro volumen o pool), verificando cada copia por re-hash. Es el primer paso de una
migración de backend: **copiar y verificar**. No cambia hashes, coordenadas, grants ni clientes, y
**nunca borra ni escribe el origen** (mover la lectura y apartar el origen es el tramo siguiente, HSP-325, descrito abajo).

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
- **La migración nunca borra ni escribe el origen.** Copiar no cambia la lectura: eso lo hace, de
  forma explícita y reversible, `POST /api/v1/storage/relocate` (sección del journal más abajo).
  Consecuencia: GC y holds no interactúan con la copia; con el journal, `retire` sí comprueba los holds.
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

## Journal de ubicación y retirada verificada del origen (HSP-325)

La migración solo copia. Para **cambiar la lectura** y **retirar el origen** sin perder datos hay un
journal de ubicación por digest en Dgraph (`LocationJournal`, predicados `hodei.loc.*`), con lease y
*fencing*, y una operación HTTP:

| Operación | Ruta | Autorización |
| --- | --- | --- |
| Relocalizar | `POST /api/v1/storage/relocate` `{repository, operation: "switch"\|"retire"\|"rollback", digests:[...]}` (1..64) | acción Cedar dedicada `Artifacts::Action::"ManageStorageRelocation"` sobre el repositorio (ninguna otra la implica, ni `ManageStorageMigration`) |

Fases por digest y adónde va la lectura:

| Fase | Lectura | Copia del origen |
| --- | --- | --- |
| `origin` (sin fila = esta) | solo el origen | intacta |
| `switched` | destino; si no tiene bytes verificados, cae al origen | intacta |
| `retired` | destino; si no tiene bytes verificados, cae al origen | **apartada** (cuarentena versionada), nunca borrada |

Transiciones permitidas: `origin→switched`, `switched→retired`, `switched→origin`, `retired→origin`
(cualquier otra es `InvalidRequest`). `retire` es **reversible**: el origen se aparta con la
cuarentena condicional del almacén (`quarantine_if_untouched`, solo si sigue siendo la versión
observada) y `rollback` repone bytes verificados y devuelve la lectura. **No existe paso de
purga/confirmación: ningún camino de este tramo borra una copia del origen** (el informe lleva
`origin_deleted: false`).

### Lease y fencing

Cada fila lleva `fence` (entero **monótono**), `token`, `owner` y `lease_until`. Para escribir hay
que tomar el lease del digest: sin fila o con lease expirado se toma con `fence+1` y token nuevo (el
escritor anterior queda fuera); el mismo dueño con lease vivo renueva con el mismo token; otro dueño
con lease vivo es `Conflict` (`busy` en el informe). Cada avance va **en una sola transacción**
guardada por token, fence, lease vivo y la fase que se espera abandonar, y el veredicto sale de la
relectura posterior al commit. Un escritor viejo o expirado que se reanuda recibe `fenced` y no cambia
nada. El lease se renueva justo antes del único efecto sobre un almacén (apartar el origen) y un
hueco en la reclamación (fence distinto) detiene la operación. Cada operación usa un dueño propio
(dos tareas del mismo proceso no comparten lease).

### Cuándo se retira el origen (todas, o nada)

1. La fase es `switched` (si no, `not_switched`).
2. Hay filas del catálogo que nombran el digest (`not_referenced` si no: no se pueden evaluar holds).
3. **Ningún hold activo** lo alcanza: de digest, de repositorio **o de cuenta**. Los de repositorio y
   cuenta se resuelven contra los propietarios del blob, leídos frescos de las filas raíz (activo,
   versión, referencia, snapshot apt); los holds se leen paginados completos (falla cerrado si no se
   pueden leer: `holds_unreadable`). Los holds se leen **antes** y **otra vez después** de apartar el
   origen: si apareció uno, el origen se repone y la fase sigue en `switched` (`held`).
4. El destino se **relee y se hashea aquí** (no se confía en su palabra): ausente → `destination_absent`,
   corrupto/mentiroso → `destination_corrupt`, caído → `destination_failed`.
5. El almacén de origen sabe apartar condicionalmente (`unsupported` si no); la versión no cambió
   mientras tanto (`raced` si cambió: no se toca).

Todo fallo deja el origen exactamente como estaba. Si el origen ya no existe (p. ej. el proceso murió
tras apartarlo y antes de escribir el journal), el reintento completa a `retired` con
`origin_already_absent: true` sin inventar nada.

### Interacción con GC, holds y repair

- **GC**: sigue operando sobre el almacén de origen (`reclaimer`/`conditional` del `LocatedStore` son
  los del origen). Los blobs referenciados siguen enraizados; la copia apartada no es de la GC. Una
  pasada de GC concurrente con `retire`/`rollback` no pierde bytes (la copia verificada del destino
  siempre existe antes de apartar el origen). Los holds frenan la retirada, no se alteran.
- **Repair**: sigue sobre el origen. Si el origen está corrupto, `retire` no lo necesita (se apoya en
  el destino verificado). Un repair tras `retire` puede **resucitar** la copia del origen desde la
  fuente de reparación; el journal, no la presencia de la copia, decide adónde va la lectura.
- `rollback` solo devuelve la lectura a un origen que **verifica ahora**; si falta lo repone desde el
  destino verificado; si está corrupto (`origin_corrupt`) hay que repararlo primero; si no hay copia
  verificable en ninguno (`nothing_to_restore`) no cambia nada.

### Códigos y auditoría

`200` todo lo pedido está donde lleva la operación; `207` parte sí; `409` nada, por causa lógica
(hold, destino que no verifica, lease ajeno...); `503` nada y falló un almacén, el journal o la
lectura de holds; `504` venció el plazo; `404` repositorio de otra cuenta o digest no referenciado por
ese repositorio (indistinguible de la ausencia; **no se mueve nada**, ni los digests válidos de la
misma petición); `403` sin la acción; `400` petición mal formada; `429 E_RELOCATE_BUSY` una operación
por cuenta y 2 en total; `501` sin destino configurado. Cada intento tras Cedar se audita **dentro de la
tarea spawneada**: `Allowed` si todo quedó donde se pidió, `Conflict` si fue parcial, `Failed` si nada,
`Denied` digest ajeno (la denegación de Cedar la audita el gate). El recurso lleva `repo[n] op=<op>`,
el recuento por estado y `set=<16 hex>`.

### Modelo de amenaza

| Amenaza | Defensa | Residuo declarado |
| --- | --- | --- |
| Escritor viejo/zombie avanza el journal | fence monótono + guarda token/fence/lease/fase en la transacción + relectura | un zombie puede ejecutar el apartado del origen ya iniciado tras perder el lease (ventana entre la renovación y el efecto); el destino verificado y la caída de lectura al otro almacén mantienen los bytes accesibles, y la fase la decide quien tiene el lease |
| Perder datos por retirar sin copia buena | el destino se relee y hashea antes de apartar; el origen no se borra nunca, se aparta | no hay purga: la copia apartada ocupa espacio hasta que el operador decida (fuera de este tramo) |
| Retirar algo retenido por un hold | holds de los tres alcances, leídos completos antes y después; falla cerrado | holds de repositorio/cuenta se resuelven por propietarios **directos** del blob (filas raíz); un propietario solo transitivo vía manifiesto no se calcula. La consecuencia está acotada: el origen queda apartado y reversible y el destino sirve |
| Un hold aparece entre la comprobación y el apartado | segunda lectura de holds tras apartar; si aparece, se repone el origen | la ventana existe pero es reversible y detectada |
| Destino corrupto o mentiroso | hash local sobre bytes releídos; un destino corrupto tras el switch no tapa un origen bueno (cae al origen) | si el destino se corrompe **después** de retirar el origen, la lectura falla; el destino no tiene reparación en este tramo (el rollback solo repone el origen desde un destino verificado) |
| Sondeo entre cuentas | repositorio de la cuenta verificada, todos los digests referenciados por él, 404 uniforme | — |
| Abuso de coste / bloqueo | 64 digests, tope por blob, 1 por cuenta y 2 globales, timeouts por llamada, plazo total, tarea inmune al cuelgue del cliente | — |
| Journal caído | las lecturas intentan origen y luego destino (siempre verificados); `switch`/`retire`/`rollback` fallan cerrado | — |

### Límites conocidos del journal (no resueltos)

- **Coste de lectura**: con destino configurado, cada lectura de contenido consulta la fila del digest
  en Dgraph (una lectura puntual por clave). No hay caché en proceso a propósito (con varias réplicas
  una caché haría que la lectura no siguiera al journal).
- **Un solo destino** (el configurado) y dos fases de retirada; sin varios miembros por pool.
- **El destino sigue fuera de GC/holds** (ver arriba): el journal no los adopta.
- **Sin purga** de la copia apartada ni de las filas del journal de digests ya borrados por la GC.

## Fuera de este tramo (decisiones de producto pendientes)

Purga de la copia apartada, varios miembros por pool, políticas de residencia/colocación por cuenta,
destino S3, selección de «todo el repositorio» sin enumerar digests, medición de RPO/RTO y coste/s.
