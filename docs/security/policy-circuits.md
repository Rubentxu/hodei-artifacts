# Circuitos de políticas

Hay dos circuitos separados y con autoridades distintas:

| Circuito | Fuente | Decide |
| --- | --- | --- |
| Gate HTTP (repositorios y administración) | Bundle Cedar validado al arrancar (`authorization_policy_path` + schema) | Acceso real a las rutas |
| Evaluador IAM (`/api/v1/iam/evaluate`) | Políticas persistidas en SurrealDB | Diagnóstico/administración de políticas IAM |

Crear o modificar una política IAM **no publica** automáticamente una nueva revisión en el gate: el bundle es el snapshot efectivo y se actualiza con el despliegue/reinicio. `tests/policy_circuits.rs` demuestra que una política persistida que concede `CreateUser` no concede acceso a `/api/v1/repositories`; el gate solo aplica su bundle.

## Contrato de publicación

- Propietario del bundle: operador de despliegue.
- Validación: schema declarado al arrancar; una política/schema inválido impide arrancar.
- Actualización: nueva revisión del fichero + reinicio; no hay recarga en caliente.
- El evaluador IAM es una herramienta administrativa; sus decisiones no se reutilizan para autorizar operaciones del gate.

## Schema efectivo: se elige por identidad, no por ser el último

El schema que gobierna las peticiones lo declara el despliegue: `authorization_schema_path`
se persiste bajo la identidad `authorization-bundle`, y tanto el validador de políticas como
el evaluador lo seleccionan **por esa identidad**, no por ser el esquema guardado más
recientemente.

La diferencia importa. El schema que el motor carga es el **registrado**, de alcance IAM,
mientras que el bundle del gate es el del despliegue; si la selección fuera por recencia,
registrar el esquema IAM —que ocurre después— desplazaría al bundle y los dos circuitos
validarían contra declaraciones distintas sin que nadie lo hubiera decidido.

Con la identidad declarada:

- el bundle gobierna las peticiones y decide el **namespace de la acción**: una acción se
  resuelve contra el schema (`Artifacts::Action::"ReadRepository"`), y una acción que el
  schema no declara, o que declara más de una vez, no tiene respuesta única y la petición se
  deniega con el motivo;
- el validador comprueba cada política contra ese mismo schema, así que ninguna política que
  el gate fuese a rechazar se puede almacenar;
- registrar el esquema IAM no desplaza al bundle.

Cuando el despliegue **no** declara bundle, el schema registrado se sigue usando como
diagnóstico, pero no gobierna la petición: es de alcance IAM y aplicarlo a una petición
genérica denegaría todo lo que no pertenezca al mundo IAM.

## Verificación

`tests/policy_circuits.rs` cubre la matriz de decisión completa —permit, deny, forbid e
indeterminate— sobre los dos circuitos con el mismo schema y la misma política:

- el gate decide `Permit`, `Deny(ExplicitForbid)`, `Deny(NoMatchingPolicy)` o
  `Indeterminate` según el caso;
- el evaluador persistido llega a la misma decisión efectiva en los cuatro casos;
- con el bundle declarado **y** el esquema IAM registrado, una política que usa una acción
  del bundle se acepta y una que usa una acción solo del esquema IAM se rechaza;
- una política persistida que concede `ReadRepository` no abre el gate: el gate solo aplica
  su bundle.

## Los resultados que cada circuito puede dar

El gate responde `Permit`, `Deny` —por `forbid` o por no encajar ninguna política— o
`Indeterminate`, que es cuando Cedar no puede decidir.

El evaluador persistido responde **permitido o denegado**, y su decisión efectiva es la
misma en los cuatro casos de la matriz. Cuando no puede evaluar, no inventa una decisión:
una petición que no encaja con el schema se **deniega con el motivo**, y un error de
evaluación es un error. La comparación entre los dos circuitos es, por tanto, sobre la
decisión efectiva, no sobre el tipo de resultado.

## Límites

- El IAM guarda **una política por fila** y Cedar analiza una política cada vez, así que una
  fila no puede contener a la vez un `permit` y un `forbid`: un `forbid` se guarda solo.
- El gate no recarga el bundle en caliente: el bundle es el snapshot efectivo y cambia con el
  despliegue.
- El key set de JWKS se lee al arrancar: rotar una clave exige reiniciar el servicio.
- Una fila escrita antes de que existieran las marcas se actualiza con marca de actualización
  y sin marca de creación: se sabe cuándo se actualizó por última vez, pero no cuándo se
  creó. Es lo que hay, y las respuestas lo dicen con un `null` en lugar de inventar una fecha.

