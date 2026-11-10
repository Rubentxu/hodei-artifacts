# Scope de cuenta en el CRUD IAM

El gate administrativo inserta el `VerifiedContext` en la petición. Los handlers IAM usan `context.account()` como scope obligatorio; ningún campo del body puede elegir la cuenta.

## Contrato

- `create`: valida el id (no vacío, `[A-Za-z0-9_-]`, ≤128) y que cada `attached_principal` pertenezca a la cuenta verificada; el id se persiste como `account~policy_id` y el HRN de respuesta usa la cuenta verificada y el id corto original.
- `get`/`update`/`delete`: el HRN solicitado debe pertenecer a la cuenta verificada. Un HRN de otra cuenta devuelve `404 Policy not found` antes de consultar o mutar.
- `list`: el adaptador lista la tabla y el handler filtra solo las filas con prefijo `account~`; el `total_count` refleja el conjunto filtrado.
- `evaluate`: principal **y recurso** deben pertenecer a la cuenta verificada; los ids determinantes se devuelven solo si llevan el prefijo de la cuenta y sin ese prefijo.
- El adaptador SurrealDB conserva la tabla global `policy`, pero **el identificador persistido incluye la cuenta** y cada fila guarda `account`, de modo que dos cuentas pueden usar el mismo `policy_id` sin colisión.
- `list_for_account` filtra, cuenta y pagina por `account` en la consulta SurrealDB. El índice `policy_account` se crea con `SurrealPolicyAdapter::initialize_scope` como migración explícita; el bootstrap no lo ejecuta porque mantener esa transacción abierta retenía el lock de RocksDB y rompía el arranque repetido.

## Migración/backfill

Las políticas creadas antes de este cambio tienen ids sin prefijo (`policy_id`): no son visibles para get/update/delete/list del scope nuevo, pero **el evaluador sigue consultándolas** hasta completar el backfill. El backfill debe:

1. identificar la cuenta propietaria de cada fila a partir de sus `attached_principals`/origen documentado;
2. crear una fila nueva con id `account~policy_id` y el mismo `content`/`attached_principals`, conservando el `policy_id` original como sufijo;
3. verificar `get`/`list` desde la cuenta propietaria antes de retirar la fila antigua.

El comando operativo está en `src/bin/backfill-iam-scope.rs`:

```text
cargo run --bin backfill-iam-scope -- --db data/hodei.rocksdb \
  --mapping backfill.json [--apply]
```

El mapping es `{ "legacy-policy-id": "account" }`. Sin `--apply` solo muestra el plan; con `--apply` copia cada fila a `account~policy_id`, conserva contenido/attachments y elimina la fila legacy **solo tras** crear la nueva. Es idempotente y se niega a migrar filas ambiguas (attachments de cuentas distintas) sin mapping explícito.

## Verificación

`tests/iam_scope.rs` crea el mismo `policy_id` en dos cuentas y comprueba que:

- ambas creaciones funcionan y devuelven HRNs de su cuenta;
- alice no puede leer, actualizar ni borrar el HRN de other;
- cada cuenta lista solo su política.

`tests/http_contract.rs` mantiene el ciclo create/get/update/delete/list/evaluate con la cuenta autenticada.

## Límites

- El índice de `account` requiere ejecutar la migración `initialize_scope` una vez; el bootstrap no la ejecuta automáticamente.
- La tabla antigua `policy` no se elimina; el backfill es operativo y debe auditarse.
