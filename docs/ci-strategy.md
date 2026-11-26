# Estrategia de CI de Artifacts

> Reescrito el 2026-11-26 (HSP-317). La versión anterior describía GitHub Actions
> (`workflow_dispatch`, un job `nextest-serial`); en este repositorio no existe `.github` y
> no hay CI remota.

## Decisión vigente

- La CI es **local** y la lleva PipelineK: la definición única es [`pipeline.kts`](../pipeline.kts).
  La prohibición de CI remota es una decisión vigente, no una preferencia.
- Las etapas (`validate`, `contract`, `fast`, `integration`, `infrastructure`, `e2e`, entre otras)
  y su razón de ser están descritas en la cabecera de `pipeline.kts`. Cada suite se ejecuta
  exactamente una vez, y `tests/ci_suites.rs` lo comprueba contra los tests que existen en disco.
- El único perfil de producción es Dgraph (el motor embebido se retiró en HSP-314). Las pruebas
  de integración con base de datos corren contra Dgraph.
- La suite con `cargo nextest ... --test-threads=1` queda fuera de `pipeline.kts` a propósito
  (se ejecuta aparte): duplicaría la cobertura en cada ejecución sin estar en el camino de la entrega.

## Medición histórica (2026-11-10)

> **HISTÓRICO (HSP-314, 2026-11-26).** Medida sobre el motor embebido retirado; no es
> reproducible en `main`. Se conserva solo como registro de por qué `nextest` salió del
> camino crítico.

En el workspace local, con la caché caliente:

| Comando | Tiempo | Resultado |
| --- | ---: | --- |
| `cargo test --workspace --no-fail-fast` | 43.35 s | verde |
| `cargo nextest run --workspace --no-fail-fast` | 100.36 s | 891/893; 2 fallos por contención de RocksDB/binary bajo paralelismo |

## Límites

- La medición histórica es local y de una sola ejecución; no se ha repetido sobre Dgraph.
- Los tests de binario/distribución siguen siendo costosos.
