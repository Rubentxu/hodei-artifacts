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

El mapping es `{ "legacy-policy-id": "account" }`. Sin `--apply` solo muestra el plan; con `--apply` copia cada fila a `account~policy_id`, conserva contenido/attachments y elimina la fila legacy **solo tras** crear la nueva.

El backfill se apoya en tres garantías:

- **Atomicidad.** La copia y la revocación viajan en una única transacción de SurrealDB, de modo que un fallo entre ambas nunca deja las dos filas vivas ni una política legacy sin scope. La transacción cubre un lote de filas, y un fallo revierte el lote completo.
- **Titularidad explícita.** La cuenta se toma del mapping o de los `attached_principals`, que deben pertenecer todos a la misma cuenta. Un adjunto que no es un HRN válido hace la fila ambigua en lugar de descartarse en silencio, porque si se ignorara la fila podría migrarse bajo la cuenta de los adjuntos que sí parsean.
- **Conflictos sin sobrescritura.** Si la fila destino ya existe, se compara su contenido con el de la fila legacy. Si coincide, solo se revoca la legacy; si difiere, la fila se reporta como conflicto y no se toca ni la legacy ni la destino.

El comando sale con código distinto de cero cuando hay filas ambiguas o conflictos, de modo que un pipeline pueda detectar una migración que requiere intervención manual.

### Salida del comando

Cada fila del plan se imprime con su estado (`ready`, `already-migrated`, `ambiguous`, `conflict`) y, cuando el estado lo explica, con el motivo. El resumen final cuenta las filas migradas, revocadas, omitidas, ambiguas y en conflicto:

```text
legacy-a -> Some("acme~legacy-a") [ready]
migrated=1 revoked=0 skipped=0 ambiguous=0 conflicts=0 apply=true
```

## Verificación

`tests/iam_scope.rs` crea el mismo `policy_id` en dos cuentas y comprueba que:

- ambas creaciones funcionan y devuelven HRNs de su cuenta;
- alice no puede leer, actualizar ni borrar el HRN de other;
- cada cuenta lista solo su política.

`tests/http_contract.rs` mantiene el ciclo create/get/update/delete/list/evaluate con la cuenta autenticada.

`tests/iam_backfill.rs` cubre la migración contra un RocksDB real:

- el dry run lista la cuenta propuesta sin escribir nada;
- el apply copia contenido, attachments y cuenta, y revoca la fila legacy;
- una segunda pasada no migra nada (idempotencia);
- un adjunto con HRN inválido hace la fila ambigua;
- una fila sin adjuntos ni mapping es ambigua;
- un mapping explícito resuelve una fila sin adjuntos;
- una fila destino con otra política se reporta como conflicto y no se sobrescribe;
- una fila destino con la misma política revoca la legacy pendiente;
- una escritura rechazada por la base de datos revierte el lote completo, sin dejar copias parciales ni filas legacy revocadas a medias;
- el comando operador se ejecuta contra una base real y publica el plan.

## Límites

- El índice de `account` requiere ejecutar la migración `initialize_scope` una vez; el bootstrap no la ejecuta automáticamente.
- La tabla antigua `policy` no se elimina; el backfill es operativo y debe auditarse.
