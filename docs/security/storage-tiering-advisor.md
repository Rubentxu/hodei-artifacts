# Asesor de tiering de solo lectura (HSP-326)

Recomienda, por digest, el tier en el que viviría un blob **citando los hechos que usó**, y no
ejecuta ninguna colocación. Mover bytes entre tiers es otra tarea con otra acción Cedar
(`ManageStorageRelocation`, ver `storage-migration.md`); este tramo solo aconseja.

| Operación | Ruta | Autorización |
| --- | --- | --- |
| Aconsejar | `GET /api/v1/storage/tiering/advice?repository=<clave>[&after=sha256:<hex>][&limit=<n>]` | acción Cedar dedicada `Artifacts::Action::"ExploreStorageTiering"` sobre el repositorio |

Ninguna otra acción la implica (ni `DownloadAsset`, ni `ManageStorageRepair`, `ManageStorageMigration`
o `ManageStorageRelocation`), y ella no concede ninguna de esas. Está declarada en
`config/community/schema.json` y `config/examples/schema.json`.

## Qué hechos existen realmente

Se comprobó contra el catálogo, el ledger y el almacén antes de diseñar. Nada se inventa.

| Hecho | Origen | Estado |
| --- | --- | --- |
| `size_bytes` | metadatos del almacén (`BlobStore::length`) | usado |
| `age_secs` (desde la última escritura o reutilización) | metadatos del almacén (`BlobStore::age`, nuevo, solo lectura) | usado si el almacén guarda fecha; el almacén de ficheros sí, S3 y los envoltorios que no lo reenvían devuelven «desconocida» |
| `references` | ledger: filas raíz del repositorio que nombran el digest (el máximo de filas de un mismo tipo de raíz, porque una publicación deja una fila de activo, de versión y de referencia) | usado |
| coste por GiB-mes de cada tier | configuración del despliegue | usado |
| **frecuencia de acceso** | no existe: la plataforma no cuenta lecturas | **falta** |
| **último acceso** | no existe: leer no deja rastro | **falta** |

La recencia es por tanto «desde que se escribió», nunca «desde que se leyó». Cada respuesta declara
`missing_facts: ["access_frequency", "last_access"]`, y por entrada añade `age_secs` cuando el
almacén no pudo dar fecha (una edad desconocida jamás se toma por vieja). Un tier por frecuencia real
de acceso exige antes un contador de lecturas: es trabajo futuro, no algo que este asesor simule.

## La decisión (núcleo puro, `src/tiering.rs`)

Los tiers se declaran de más caliente a más frío. Un tier es *elegible* si se cumplen todas sus
condiciones: edad ≥ `min_age_secs`, tamaño ≥ `min_size_bytes`, referencias ≤ `max_references`. Se
recomienda el **tier elegible más frío**; el más caliente no tiene condiciones, así que siempre hay
uno. Cada entrada cita (`reasons`) el hecho, su valor y la regla satisfecha, el motivo por el que el
siguiente tier más frío no se eligió (`colder_tier_blocked_by`) y el coste mensual estimado frente al
del tier caliente. Es una función pura: mismos hechos y misma política, mismo consejo. Tiene pruebas
unitarias y de propiedades por barrido determinista (el consejo es el tier elegible más frío; envejecer
nunca calienta; el coste recomendado nunca supera el del tier caliente; una escalera validada siempre
aconseja).

## Configuración (`[tiering]`, validada al arrancar)

```toml
[tiering]
cost_unit = "USD"            # 1..8 alfanuméricos
max_digests = 200            # 1..=1000   digests por respuesta
max_rows = 10000             # 1..=100000 filas del ledger leídas por petición
max_response_bytes = 262144  # 1024..=4 MiB
timeout_secs = 20            # 1..=120
max_concurrent = 4           # 1..=32

[[tiering.tiers]]            # de más caliente a más frío, 1..=8
name = "hot"                 # [a-z0-9_-]{1,32}, único
monthly_cost_micro_per_gib = 23000000   # millonésimas de cost_unit, <= 1e12

[[tiering.tiers]]
name = "cold"
monthly_cost_micro_per_gib = 4000000
min_age_secs = 2592000
min_size_bytes = 1048576
max_references = 1
```

Un campo desconocido (en la sección o en un tier), un rango fuera de límites, un nombre repetido, un
tier base con condiciones, un tier más frío que cueste más o que acepte blobs más jóvenes que el
anterior impiden arrancar. Sin sección `[tiering]` no hay asesor y el endpoint responde `501
E_TIERING_UNAVAILABLE` (tras Cedar). Solo el perfil de grafo cablea el asesor.

## Respuestas

`200` con `{repository, read_only: true, cost_unit, tiers, entries, truncated, truncated_by,
next_after, rows_read, missing_facts}`; cada entrada es `advised` (con consejo) o `blob_missing` (el
ledger nombra bytes que el almacén no tiene: es un hallazgo para el scrub, no un tier). `400` entrada
mala (`after`, `limit`, parámetro desconocido); `401`; `403` sin la acción; `404` repositorio ajeno o
inexistente (idéntico); `413 E_TIERING_RESPONSE_TOO_LARGE` una sola entrada no cabe en
`max_response_bytes`; `429 E_TIERING_BUSY` sin plaza en el semáforo; `501`; `503 E_TIERING_FAILED`
ledger o almacén caídos (nunca una respuesta parcial que parezca completa); `504 E_TIERING_TIMEOUT`.

## Modelo de amenaza

| Amenaza | Defensa |
| --- | --- |
| Un principal sin permiso sondea tamaños, edades o digests | Cedar decide **antes** de leer el ledger o el almacén; un denegado cuesta cero lecturas y se audita `Denied` |
| Otra cuenta enumera blobs ajenos o confirma su existencia | el repositorio debe ser de la cuenta verificada (404 idéntico a la ausencia); solo se listan digests referenciados por **ese** repositorio de **esa** cuenta; el cursor `after` no escapa de la cuenta |
| El consejo se usa como puerta trasera para escribir o mover | el endpoint solo lee: no recibe cuerpo, no llama a `put`/`begin`/migración/relocalización ni a la cuarentena; una prueba compara todos los nodos de Dgraph y todo el árbol del CAS (bytes y fechas) antes y después de varias llamadas, y cuenta cero escrituras en el almacén |
| Denegación de servicio por repositorios enormes | topes validados: digests por respuesta, filas del ledger por petición (también acota el tamaño de página), bytes de respuesta, plazo y semáforo de concurrencia (`429`) |
| Un cliente que cuelga evita la auditoría | el trabajo y su auditoría (`Allowed`/`Failed`) corren en una tarea propia; el `Denied` de Cedar lo emite la propia compuerta dentro de ella |
| Un fallo se lee como «no hay nada que mover» | falla cerrado: ledger sin paginación, ledger o almacén caídos, plazo vencido: 501/503/504, nunca una lista corta |
| Una recomendación basada en datos que no existen | los hechos ausentes se declaran; una edad desconocida no satisface ninguna condición de edad |
| Configuración peligrosa o con erratas | validación al arrancar con rangos y `deny_unknown_fields`; sin tiers, sin endpoint |
| Fuga en los mensajes o la auditoría | la auditoría lleva repositorio, recuento de entradas y filas, nunca digests ni tamaños; los errores no citan datos |

## Qué no hace

No ejecuta colocaciones ni las programa, no persiste el consejo, no mira el tier actual de un blob
(el journal de ubicación no está mapeado a la escalera), no estima costes de egreso o de petición ni
la moneda real, y no ve la frecuencia de acceso. El ahorro es «coste en el tier recomendado frente al
tier más caliente», sobre tarifas de la configuración.
