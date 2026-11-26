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
`--threshold-pct 50`, `--min-delta-ms 2`, `--inject-delay-ms N` (solo para
comprobar que `--compare` falla) e `--inject-kill <carga>` (mata el servicio a
mitad de esa carga, para comprobar la sonda de vida). Nunca se registra un
baseline con ninguna de las dos inyecciones.
Sin `HODEI_GRAPH_IT_ENDPOINT` el target se salta y termina con código 0.

La pipeline (`integration (dgraph)`) ejecuta el target a escala 200 como humo:
falla si alguna petición falla, pero no compara con el baseline porque la
comparación solo tiene sentido en la misma máquina.

### `--compare`

Falla (código 1) si el p95 de cualquier pareja (escala, carga) compartida con
el baseline empeora más de `--threshold-pct` por ciento **y** más de
`--min-delta-ms` milisegundos (el suelo evita que el ruido de submilisegundo
tumbe la ejecución), o si alguna petición falla. Las parejas que no están en
el baseline se listan como no comparadas.

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
como observación única (una réplica, sin repetir). No se midió CPU del servicio
ni RSS del servicio (extensión 4).

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
- **Esta extensión (S2, 1 y 2) no mide CPU/RSS del servicio ni compara p99 o
  rendimiento**; eso queda para las extensiones 3 y 4. Tampoco se ha
  investigado el 503 de publicar con concurrencia alta.
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
