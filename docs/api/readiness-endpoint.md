# Endpoints de salud: `/health/ready` y `/health/live`

## Descripción

El servicio expone tres sondas, en la raíz del servidor y **sin prefijo
`/api/v1` ni autenticación**:

| Ruta | Pregunta que responde | Hace E/S |
| --- | --- | --- |
| `GET /health` | Comprobación heredada: el proceso responde. | No |
| `GET /health/live` | ¿Vive el **proceso**? | No |
| `GET /health/ready` | ¿Puede el despliegue **servir**? | Sí, acotada |

La liveness y la readiness son preguntas distintas. Una dependencia caída pone
la readiness en `503` y deja la liveness en `200`: el proceso está bien, y
reiniciarlo no arregla la dependencia. Usa `/health/live` para el reinicio y
`/health/ready` para sacar la réplica de la rotación (el chart de Helm lo hace
así, ver `charts/hodei-artifacts/values.yaml`).

El código vive en `src/handlers/health.rs` (handlers), `src/readiness.rs`
(probe, presupuestos y caché) y `src/router.rs` (rutas).

## `GET /health/live`

Siempre `200 OK`, sin tocar ninguna dependencia:

```json
{
  "status": "alive",
  "version": "0.1.0",
  "service": "hodei-artifacts-api",
  "timestamp": "2026-01-15T10:30:00+00:00"
}
```

## `GET /health/ready`

### Cuerpo

El cuerpo es **grueso a propósito**: nombra cada dependencia que el perfil
activo exige y su estado, y nada más. Nunca incluye URLs, endpoints, nombres de
bucket, rutas, usuarios ni credenciales (el error crudo de un driver puede
traer cualquiera de ellos, así que no viaja).

```json
{
  "status": "not_ready",
  "dependencies": [
    { "name": "graph", "status": "ok" },
    { "name": "content_store", "status": "unreachable" }
  ]
}
```

- `status`: `ready` o `not_ready`.
- `dependencies`: **todas** las dependencias exigidas (no solo las que fallan),
  en este orden: `catalog`, `graph`, `content_store`. Una dependencia que el
  perfil no exige no aparece y no puede hacer fallar la readiness.
- `dependencies[].name`: `catalog`, `graph` o `content_store`. (`trust_policy`
  existe como valor del tipo, pero ningún perfil lo exige: la política de
  confianza se carga en memoria al arrancar.)
- `dependencies[].status`: `ok`, `unreachable` (no se alcanza), `timeout` (no
  contesta dentro del presupuesto) o `misconfigured` (declarada de una forma
  que no se puede usar: credenciales ACL rechazadas, bucket inexistente o
  prohibido, ruta que no es un directorio).

### Códigos

| Código | Cuándo |
| --- | --- |
| `200 OK` | Todas las dependencias exigidas están disponibles (`status = ready`). |
| `503 Service Unavailable` | Al menos una falla (`status = not_ready`). El proceso sigue vivo. |

Un fallo de dependencia nunca es un `500` ni un pánico: es un estado de la
readiness.

## Qué exige cada perfil y qué llamadas hace

El probe comprueba **una vez** cada dependencia exigida por veredicto:

| Perfil | Dependencia | Llamada de la comprobación |
| --- | --- | --- |
| Embebido (sin `[graph]`) | `catalog` | Una sentencia local `RETURN true` sobre el motor embebido (dentro del proceso; no toca tablas). |
| Embebido, contenido en disco | `content_store` | Una llamada al sistema de archivos: el directorio existe y es un directorio. No lee su contenido. |
| Contenido en S3 (cualquier perfil) | `content_store` | Un único `HeadBucket`. Nunca lista el bucket ni los objetos. |
| Grafo (`[graph]`, `RUN_MODE=dgraph`) | `graph` | Login ACL del Alpha, una comprobación de versión (best effort) y una consulta DQL de solo lectura (`schema {}`) autenticada. Sin escrituras ni recorrido de datos. |

El perfil de grafo exige `graph` y `content_store`; no exige `catalog` (la
autoridad del metadato es el Alpha, no el motor embebido).

## Presupuestos y caché

Definidos en `src/readiness.rs`:

| Constante | Valor | Efecto |
| --- | --- | --- |
| `DEPENDENCY_BUDGET` | 1 s | Plazo de **una** comprobación; al agotarse, esa dependencia queda en `timeout`. |
| `PROBE_BUDGET` | 2 s | Plazo de **todo** el probe. Las dependencias que ya no caben se declaran `timeout` sin comprobarlas. |
| `VERDICT_TTL` | 1 s | El veredicto (también un `503`) se cachea 1 s: sondeos seguidos dentro del TTL no repiten las llamadas. |

Consecuencias:

- Una petición a `/health/ready` tarda como mucho unos 2 s aunque las
  dependencias no contesten (con dos dependencias mudas, 1 s + 1 s; con tres,
  la tercera se declara `timeout` sin esperar).
- Tras recuperarse una dependencia, la readiness vuelve a `200` en como mucho
  1 s (el TTL) más el tiempo de la comprobación.
- Con un orquestador que sondea cada pocos segundos, el coste es de una
  comprobación por dependencia y TTL, no por petición.

## Ejemplos

```bash
curl -s -o /dev/null -w '%{http_code}\n' http://localhost:3000/health/live
curl -s -w '\n%{http_code}\n' http://localhost:3000/health/ready
```

Con el S3 apagado:

```text
{"status":"not_ready","dependencies":[{"name":"graph","status":"ok"},{"name":"content_store","status":"unreachable"}]}
503
```

## Pruebas

- `src/readiness.rs` (unitarias): timeout, dependencia opcional, caché y una
  comprobación por dependencia.
- `tests/hsp237_readiness.rs`: servicio real con el almacén de contenido
  desconectado en caliente; probe en proceso.
- `tests/hsp237_graph_readiness.rs`: perfil de grafo con un Dgraph simulado que
  cae y se recupera.
- `tests/hsp292_readiness_timeout.rs`: por HTTP (`build_router`), un probe que
  no contesta da `503` con `timeout` en el cuerpo en menos de 2,5 s, sin
  necesidad de Dgraph ni S3.
- `tests/hsp237_profile_external_e2e.rs` (con `scripts/hsp237-profile-e2e.sh`):
  binario real en perfil de grafo con Dgraph y S3 reales (Podman); parar y
  devolver cada uno. Sin Podman o sin imágenes termina con `NOT_RUN` (exit 77),
  nunca con un PASS. Evidencia en `target/uat/hsp237-profile-e2e/`.

```bash
cargo test -p hodei-artifacts-api --test hsp292_readiness_timeout
scripts/hsp237-profile-e2e.sh
```

La documentación OpenAPI se sirve en `/api-docs/openapi.json` si está
habilitada.
