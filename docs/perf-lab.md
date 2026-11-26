# Laboratorio de rendimiento sobre Dgraph (HSP-319, S1)

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
`--payload-bytes 1024`, `--out target/perf-lab/last.json`,
`--record <ruta>` (escribe el baseline), `--compare <ruta>`,
`--threshold-pct 50`, `--min-delta-ms 2` e `--inject-delay-ms N` (solo para
comprobar que `--compare` falla; nunca se registra un baseline con él).
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
