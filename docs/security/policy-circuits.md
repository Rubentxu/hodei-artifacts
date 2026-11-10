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

## Deuda

Los dos circuitos no comparten todavía el mismo schema/bundle: el gate usa el fichero de producción y el evaluador IAM usa el schema persistido. Unificarlos exige un registro de schemas/snapshot común; queda como decisión de arquitectura, no como bypass actual.
