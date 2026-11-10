# Estrategia de CI de Artifacts

## Medición (2026-11-10)

En el workspace local, con la caché caliente:

| Comando | Tiempo | Resultado |
| --- | ---: | --- |
| `cargo test --workspace --no-fail-fast` | 43.35 s | verde |
| `cargo nextest run --workspace --no-fail-fast` | 100.36 s | 891/893; 2 fallos por contención de RocksDB/binary bajo paralelismo |

La segunda señal de nextest no era gratuita: duplicaba la suite y, con la concurrencia por defecto, los tests de RocksDB/binario agotaban el tiempo. No se elimina la intención de aislamiento; se saca del camino crítico de PR.

## Decisión

- PR/push: fmt, clippy, `cargo check`, build y `cargo test --workspace`. Esta es la compuerta completa y rápida.
- `workflow_dispatch`: job `nextest-serial` con `--test-threads=1` para isolación; no bloquea PRs.
- No se elimina ninguna suite ni caso; se evita ejecutar la misma cobertura dos veces en cada push.

## Límites

- La medición es local y de una sola ejecución; GitHub Actions puede variar. Repetir la medición antes de ajustar umbrales.
- Los tests de binario/distribución siguen siendo costosos; si se estabilizan, pueden volver a un job programado.
