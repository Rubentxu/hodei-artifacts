# Scope de cuenta en el CRUD IAM

El gate administrativo inserta el `VerifiedContext` en la petición. Los handlers IAM usan `context.account()` como scope obligatorio; ningún campo del body puede elegir la cuenta.

## Contrato

- `create`: valida el id (no vacío, `[A-Za-z0-9_-]`, ≤128) y que cada `attached_principal` pertenezca a la cuenta verificada; el id se persiste como `account~policy_id` y el HRN de respuesta usa la cuenta verificada y el id corto original.
- `get`/`update`/`delete`: el HRN solicitado debe pertenecer a la cuenta verificada. Un HRN de otra cuenta devuelve `404 Policy not found` antes de consultar o mutar.
- `list`: el adaptador lista la tabla y el handler filtra solo las filas con prefijo `account~`; el `total_count` refleja el conjunto filtrado.
- `evaluate`: principal **y recurso** deben pertenecer a la cuenta verificada; los ids determinantes se devuelven solo si llevan el prefijo de la cuenta y sin ese prefijo.
- El adaptador SurrealDB conserva la tabla global `policy`, pero **el identificador persistido incluye la cuenta**, de modo que dos cuentas pueden usar el mismo `policy_id` sin colisión.

## Migración/backfill

Las políticas creadas antes de este cambio tienen ids sin prefijo (`policy_id`): no son visibles para get/update/delete/list del scope nuevo, pero **el evaluador sigue consultándolas** hasta completar el backfill. El backfill debe:

1. identificar la cuenta propietaria de cada fila a partir de sus `attached_principals`/origen documentado;
2. crear una fila nueva con id `account~policy_id` y el mismo `content`/`attached_principals`, conservando el `policy_id` original como sufijo;
3. verificar `get`/`list` desde la cuenta propietaria antes de retirar la fila antigua.

No se ejecuta una migración automática porque una fila antigua no contiene la cuenta de forma inequívoca; el backfill requiere una decisión de procedencia y debe adjuntarse a una tarea operativa. Mientras tanto, las filas antiguas siguen siendo efectivas en la evaluación y no se pueden revocar por la API nueva: es un riesgo que hay que cerrar con el backfill.

## Verificación

`tests/iam_scope.rs` crea el mismo `policy_id` en dos cuentas y comprueba que:

- ambas creaciones funcionan y devuelven HRNs de su cuenta;
- alice no puede leer, actualizar ni borrar el HRN de other;
- cada cuenta lista solo su política.

`tests/http_contract.rs` mantiene el ciclo create/get/update/delete/list/evaluate con la cuenta autenticada.

## Límites

- El filtrado de `list` se hace en el handler tras el listado del adaptador; la paginación/`total_count` no se delega todavía a una consulta por cuenta.
- La tabla antigua `policy` no se elimina; el backfill es operativo y debe auditarse.
