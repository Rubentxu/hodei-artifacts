# Persistencia durable de repositorios

HSP-21 añade `SurrealRepositoryStore`, un adaptador que implementa los puertos `RepositoryReader` y `RepositoryWriter` sobre la misma base RocksDB/SurrealDB del servicio.

## Garantías

- Tabla `repository` con `hrn`, `account`, `name`, `description`.
- Índice único `repository_hrn_unique` sobre `hrn`: la unicidad la impone la base de datos, no el proceso.
- `create` valida que `view.account`/`view.hrn` coincidan con el principal/HRN; un conflicto de índice se mapea a `AlreadyExists` (409); los fallos de conexión a `Unavailable` (503).
- `find_by_hrn` filtra por HRN y cuenta.
- El mismo store se inyecta como reader y writer (`AppState::with_repository_store`).

## Configuración

```toml
[repository]
durable = true
```

- `config/release.toml` lo activa; el default de `AppConfig` es `false` para tests/desarrollo rápido y mantiene el store en memoria.
- El adaptador se inicializa al arrancar y crea tabla/índice de forma idempotente.

## Verificación

`tests/repository_durable.rs`:

1. create → reconstruir el adaptador sobre la misma RocksDB → find devuelve la entidad; un segundo create devuelve `AlreadyExists`.
2. dos writers sobre la misma base compiten por el índice único: exactamente uno crea y el otro recibe conflicto.

## Límites

- El test de persistencia reconstruye el adaptador sobre la misma base; no levanta un proceso OS separado. La garantía es del almacén (RocksDB), no de memoria del adaptador.
- La migración de tablas existentes y el backfill de `account` quedan para el despliegue; el schema es nuevo.
- No se reactiva el crate legacy/Mongo.
