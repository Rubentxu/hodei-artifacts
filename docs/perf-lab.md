# Laboratorio de rendimiento sobre Dgraph (HSP-319, S1 y S2)

Este laboratorio mide, con evidencia reproducible, cuánto cuesta publicar,
leer y buscar artefactos en el perfil de grafo (`RUN_MODE=dgraph`) contra un
Dgraph real. Sustituye al laboratorio anterior (`perf_lab`, retirado en
HSP-314 junto con el motor embebido). No usa SurrealDB ni RocksDB.

## Qué hace

El target `hsp319_perf_lab_it` (`tests/hsp319_perf_lab_it.rs`, sin harness de
libtest para aceptar sus propias opciones) arranca el binario de producción
sobre un namespace ACL nuevo del alpha por cada escala y, con el backend de
contenido en sistema de ficheros:

| Carga | Qué ejecuta |
| --- | --- |
| `publish` | `PUT /repository/perf/<ruta>` de N activos únicos (1 KiB por defecto) |
| `read` | `GET` de `--samples` activos publicados, verificando los bytes |
| `search_text` | `GET /api/v1/search?text=...`, comprobando que devuelve el activo |
| `search_digest` | `GET /api/v1/search?digest=sha256:...`, ídem |

Por carga informa de muestras, errores, p50/p95/p99 (ms), rendimiento
(peticiones/s con solo los éxitos) y tiempo de pared. El p99 se informa como
`null` con menos de 1000 muestras correctas.

## Cómo ejecutarlo

Necesita un Dgraph con ACL (el mismo entorno que el resto de los `*_it`):

```bash
export HODEI_GRAPH_IT_ENDPOINT=127.0.0.1:9080   # gRPC, sin http://
export HODEI_GRAPH_IT_ADMIN=127.0.0.1:8080
export HODEI_GRAPH_IT_USER=groot
export HODEI_GRAPH_IT_PASSWORD=password

# ejecución y comparación con el baseline versionado
cargo test --test hsp319_perf_lab_it -- --scales 1000,10000 \
    --compare docs/perf/baseline-graph.json
```

Opciones: `--scales 1000,10000`, `--concurrency 4`, `--samples 1000`,
`--payload-bytes 1024` (lista, véase abajo), `--dist uniform|zipf`,
`--zipf-s 1.0`, `--seed 319`, `--out target/perf-lab/last.json`,
`--record <ruta>` (escribe el baseline), `--compare <ruta>`,
`--threshold-pct 50`, `--min-delta-ms 2`, `--p99-threshold-pct 100`,
`--p99-min-delta-ms 5`, `--throughput-drop-pct 30`,
`--min-throughput-delta-rps 5`, `--repeat N`, `--dgraph-container NOMBRE`
(o `HODEI_GRAPH_IT_CONTAINER`), `--inject-delay-ms N` (solo para
comprobar que `--compare` falla) e `--inject-kill <carga>` (mata el servicio a
mitad de esa carga, para comprobar la sonda de vida). Nunca se registra un
baseline con ninguna de las dos inyecciones.
Sin `HODEI_GRAPH_IT_ENDPOINT` el target se salta y termina con código 0.

La pipeline (`integration (dgraph)`) ejecuta el target a escala 200 como humo:
falla si alguna petición falla, pero no compara con el baseline porque la
comparación solo tiene sentido en la misma máquina.

### `--compare`

Falla (código 1) si, en cualquier pareja (escala, carga) compartida con el
baseline, empeora alguna de estas métricas (cada una necesita superar el
porcentaje **y** el suelo absoluto; el suelo evita que el ruido pequeño tumbe
la ejecución), o si alguna petición falla:

| Métrica | Falla si | Umbral | Suelo |
| --- | --- | --- | --- |
| p95 | `ahora > antes x (1 + pct/100)` | `--threshold-pct` (50) | `--min-delta-ms` (2 ms) |
| p99 | ídem | `--p99-threshold-pct` (100) | `--p99-min-delta-ms` (5 ms) |
| rendimiento | `ahora < antes x (1 - pct/100)` | `--throughput-drop-pct` (30) | `--min-throughput-delta-rps` (5 rps) |

Se imprime una línea por métrica (`[compare] ... / p95: antes -> ahora (límite,
suelo) ok|REGRESSION`) y un `FAIL` por cada regresión. El p99 solo se compara si
las dos filas lo tienen (hacen falta 1000 muestras correctas); si falta en
alguno de los lados la línea dice `skipped` y no falla, de modo que las filas
del baseline antiguo y las ejecuciones con `--samples` bajo no rompen la
comparación. El comportamiento del p95 es el de siempre. Las parejas que no
están en el baseline se listan como no comparadas.

Con `--repeat N` la comparación usa la **mediana** de las N ejecuciones.

### `--repeat N` y cómo elegir el umbral

`--repeat N` ejecuta N veces la misma configuración (cada una con servicio y
namespace nuevos) y deja en cada fila la mediana como cifra principal y, en
`repeat.dispersion.<carga>.<métrica>`, `median`, `min`, `max`, `mad` (desviación
absoluta mediana), `mad_pct`, `range_pct` (`(max-min)/mediana`) y los valores
de cada ejecución.

Medido en esta máquina (1000 activos, 1 KiB, concurrencia 4, `--repeat 5`):

| Métrica | `mad_pct` | `range_pct` (peor carga) |
| --- | --- | --- |
| p95 | 0,8 a 2,2 % | 22,7 % (`publish`, una ejecución atípica de 75 ms frente a 61 a 63 ms); 5 a 9 % en el resto |
| p99 | 0,2 a 2,0 % | 3,8 a 10,9 % |
| rendimiento | 0,8 a 1,9 % | 4,6 a 7,5 % |

Regla para fijar un umbral: ejecuta `--repeat 5` (como mínimo) sobre la
configuración que vas a vigilar, toma el mayor `range_pct` de la métrica y fija
el umbral en **2 a 3 veces ese rango**, de forma que una ejecución sana no falle
y una regresión real (decenas de por ciento) sí. Con eso, los valores por
defecto son: p95 50 % (2,2 veces el peor rango), p99 100 % (p99 es más
ruidoso: se mide sobre 10 muestras de cola y se compara con un baseline de una
sola ejecución) y rendimiento 30 % (4 veces el peor rango, porque el
rendimiento es la métrica más estable). El suelo absoluto se elige al nivel del
ruido de la métrica más rápida (2 ms de p95 en `read`, que ronda 10 ms; 5 rps
de rendimiento). Si en otra máquina o escala `range_pct` es mayor, sube el
umbral en proporción o compara contra la mediana de un baseline hecho con
`--repeat`. Un baseline de una sola ejecución arrastra su propio ruido.

Comprobado con mutaciones (1000 activos contra el baseline): `--inject-delay-ms
50` falla en las tres cargas de lectura por p95, p99 y rendimiento (`publish`
solo por rendimiento, su latencia base ya es alta); con p95 y rendimiento
relajados falla solo por p99; `--concurrency 1` falla solo por rendimiento.

### Muestreo de recursos (HSP-140, HSP-215)

Durante cada carga un hilo muestrea cada ~250 ms el proceso del servicio
(`/proc/<pid>/stat` para CPU y `/proc/<pid>/status` para `VmRSS`) y la fila de
cada carga lleva `resources.service`: `cpu_pct_mean`, `cpu_pct_max` (porcentaje
de **un** núcleo: 200 son dos núcleos ocupados), `rss_mib_max`, `rss_mib_mean`,
`readings` y `discarded_readings`.

Para Dgraph hay que decir qué contenedor mirar, con `--dgraph-container NOMBRE`
o `HODEI_GRAPH_IT_CONTAINER`. Entonces `resources.dgraph_resources` trae
`cpu_pct_mean`, `cpu_pct_max` (derivados del `cpu_time` acumulado de
`podman stats --no-stream --format json`), `mem_mib_max` y `mem_mib_mean`. Sin
contenedor, o si el contenedor no existe o no publica el puerto de
`HODEI_GRAPH_IT_ENDPOINT` (no es ese alpha), el valor es la cadena
`"not_sampled"`, `parameters.dgraph_container` dice por qué y no se inventa
nada. Cada lectura de Dgraph se valida: el ID debe ser el resuelto al empezar,
el límite de memoria no puede cambiar respecto a la primera lectura y el
`cpu_time` no puede retroceder (un lector que pasa a otro cgroup a mitad de la
ejecución, como se observó con `docker stats`). Una lectura incoherente se
descarta y se cuenta en `discarded_readings` con el motivo en
`discard_reasons`. La validación no se ha forzado con un cambio real de cgroup
(no se pudo provocar): solo se ha comprobado que en las ejecuciones normales no
se descarta nada. Con `--repeat`, los picos (`*_max`) son el máximo de las
ejecuciones y el resto la mediana.

El proceso del laboratorio (el cliente de carga) no se mide.

El baseline versionado (`docs/perf/baseline-graph.json`) **no se ha regenerado**
con estos campos: una ejecución de registro posterior, con la misma
configuración y sin errores, salió entre un 10 y un 25 % más lenta en esta
máquina (el alpha compartido acumulaba ya los namespaces de las pruebas), y
versionarla habría aflojado la referencia sin motivo. Los recursos viven en el
JSON de `--out` de cada ejecución; el baseline no los lleva y `--compare` no los
compara.

### Réplica válida: sonda de vida y `permission_probe` (HSP-173)

Tras el arranque y al terminar cada carga el laboratorio ejecuta dos sondas:

- **Vida**: `GET /health/live` (y que el proceso no haya terminado). Si falla,
  la réplica queda `INVALID_SERVICE_DIED`.
- **`permission_probe`**: una petición autorizada (`GET /api/v1/catalog/repositories`
  con el token de la cuenta). Si no hay respuesta (estado 0 / conexión
  rechazada) o devuelve 401/403, la réplica queda `INVALID_PERMISSION_PROBE`.
  Otros estados no son un fallo de autorización: se avisan por `stderr`.

Ante cualquiera de los dos el laboratorio **aborta en el acto** (no sigue
midiendo) y el JSON lleva `replica.verdict`, `replica.scenario` (la carga
tras la que se detectó, con escala y tamaño), `replica.last_good_scenario`
(la última carga cuyas sondas pasaron), `replica.detected_at_utc` y
`replica.partial_workloads` como evidencia. El proceso sale con código 1;
`replica.verdict` es `VALID` en una ejecución sana. **`--record` y `--compare`
rechazan una réplica inválida** (nunca alimenta un baseline ni una
comparación) y `--compare` también rechaza un baseline cuyo veredicto no sea
`VALID`. Un fallo de arranque (`INVALID_SETUP`) se trata igual.

Comprobación con mutación: `--inject-kill read` mata el servicio a mitad de la
lectura y debe terminar en `INVALID_SERVICE_DIED` con último escenario bueno
`publish(...)`.

### Tamaños de contenido y distribución de acceso (HSP-140, HSP-215)

- `--payload-bytes 1KiB,64KiB,1MiB` (acepta `B`, `KiB`, `MiB`, `GiB` o bytes
  sin unidad) ejecuta **varios tamaños en una sola ejecución**: cada pareja
  (escala, tamaño) es una fila `scales[]` con `payload_bytes`, sobre un
  namespace y un servicio nuevos. El defecto sigue siendo 1 KiB. Ojo con el
  disco: `escala x tamaño` bytes se escriben en el directorio temporal.
- `--dist uniform|zipf` elige cómo `read`, `search_text` y `search_digest`
  escogen el activo. `uniform` es el comportamiento de S1 (mismas muestras,
  comparable con el baseline). `zipf` usa `P(rango k) ~ 1/k^s` con
  `--zipf-s` (1.0 por defecto) y `--seed` (319): mismas opciones, mismos
  accesos. Los rangos se reparten por el espacio de claves con un paso
  coprimo con la escala, de modo que los activos calientes no son los primeros.
- Cada fila informa de `access`: `distinct_assets`, `top1_share`,
  `top10_share` y los cinco activos más pedidos con su recuento, que es la
  prueba de que Zipf concentra las lecturas y uniforme no.
- `--compare` empareja filas por (activos, tamaño, distribución); las filas del
  baseline antiguo (schema 1) cuentan como 1 KiB y uniforme.

### Escala 100 000 (HSP-140, HSP-215)

`--scales 100000` está soportado (la generación de la distribución Zipf y del
plan de accesos escala a 100 000 sin problema). Observado en esta máquina
(commit de la rama, binario de depuración, Dgraph v25.5.1 de un solo nodo,
`--dist zipf --samples 200`, concurrencia 4, 1 KiB):

| Observación | Valor |
| --- | --- |
| Tiempo total de la ejecución | 22 min 38 s (publicar 1013,7 s; `search_text` 290,2 s; `search_digest` 47,5 s) |
| `publish` | p50 27,5 ms, p95 94,0 ms, p99 252,4 ms, 98,6 peticiones/s |
| `read` (zipf) | p50 9,3 ms, p95 10,5 ms |
| `search_text` | p50 6281 ms, p95 6806 ms (recorre el catálogo; ~0,7 peticiones/s) |
| `search_digest` | p50 35,4 ms, p95 2800 ms |
| Errores | 1 de 100 000 publicaciones devolvió HTTP 503 (el hallazgo conocido; no investigado) |
| Memoria del proceso del laboratorio | 58 MiB de RSS máximo (`/usr/bin/time -v`) |
| Memoria del Dgraph (`docker stats`, muestras cada 30 s) | pico muestreado 2,87 GiB durante la publicación; a mitad de la ejecución `docker stats` pasó a leer otro cgroup (límite 24 GiB) y las lecturas posteriores (1,0 a 1,6 GiB) no son comparables |

Esa ejecución **no es un baseline**: tuvo un error, así que el laboratorio la
declara inválida (código 1) y `--record` la rechazaría. No se ha añadido ninguna
fila de 100 000 a `docs/perf/baseline-graph.json`; solo se afirma lo anterior,
como observación única (una réplica, sin repetir). Esa observación es anterior al
muestreo de recursos del laboratorio (extensión 4).

## Baseline

`docs/perf/baseline-graph.json` declara el commit, el hardware (CPU, núcleos,
memoria, kernel), la imagen de Dgraph, los parámetros y las escalas medidas
(1000 y 10 000 activos). Se registró con:

```bash
cargo test --test hsp319_perf_lab_it -- --scales 1000,10000 \
    --record docs/perf/baseline-graph.json
```

Para regenerarlo en otra máquina hay que declarar esa máquina; un baseline de
una máquina no es comparable con otra.

## Límites y lo que NO se afirma

- **Solo se afirma lo medido**: 1000 y 10 000 activos, en el hardware y con el
  binario de depuración declarados en el baseline (el servicio se compila sin
  optimizar: las cifras absolutas no representan un despliegue de producción).
- Las extensiones 3 y 4 de S2 añaden la comparación de p99 y rendimiento, la
  dispersión con `--repeat` y el muestreo de CPU/RSS del servicio y de Dgraph.
  Siguen sin cubrirse: bucle abierto, réplicas del servicio, métricas internas
  del alpha más allá de CPU y memoria del contenedor, y el 503 de publicar con
  concurrencia alta (no investigado).
- **10M y 100M de activos: `NOT_RUN`.** Pertenecen a S4 (HSP-216) y el JSON lo
  declara en `not_run`. No hay ninguna afirmación a esas escalas.
- **Bucle cerrado**: hay `--concurrency` peticiones en vuelo; no se corrige la
  omisión coordinada, así que las latencias de un servicio saturado son
  optimistas.
- **Hallazgo**: con concurrencia 16 la publicación devolvió HTTP 503 en
  ~0,3 % de las peticiones y con 8 todavía apareció alguno a 10 000 activos;
  con 4 no hubo errores. El laboratorio cuenta los errores aparte (nunca como
  rendimiento) y por eso el defecto es 4. La causa del 503 no se ha
  investigado aquí.
- `search_text` recorre el catálogo (no hay índice de texto completo): su p95
  pasó de ~32 ms a 1000 activos a ~254 ms a 10 000.
- No cubre: OCI/npm/PyPI/Maven, blobs grandes, escenas frías, el plano de datos
  rápido ni el radio de impacto (HSP-147, 148, 172, 173, 215, que siguen
  abiertos en HSP-319), ni réplicas de la ejecución completa.
