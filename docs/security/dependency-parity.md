# Dependencias publicadas y paridad HRN

Artifacts consume **hodei-authz 0.1.2** publicado, que a su vez usa **hodei-hrn 0.2.1**. Ya no hay copia local de HRN 0.1 en Authz.

## Estado

- `hodei-authz = "=0.1.2"` y `hodei-hrn = "0.2.1"` (dev) fijados en `Cargo.toml`; `Cargo.lock` versionado.
- El camino de autorización usa el contrato 0.2.1 a través de `hodei-authz`; `execute_canonical` construye el HRN con `Hrn::new` y conserva paths anidados.
- `kernel::Hrn` se mantiene para el contrato persistido/HTTP. No se retira porque está serializado en documentos IAM y es la clave del store de repositorios; la paridad con 0.2.1 demuestra que la retirada futura es posible.
- El encoding de tipos Cedar es inyectivo: `package-version` → `_x2D`, `package_version` → `_x5F`, y los servicios con dígito inicial se aceptan igual que el contrato.

## Verificación

- `cargo test --test hrn_parity`: fixtures v2, v1/global, legacy repository y path anidado contra 0.2.1.
- `cargo test --workspace`: verde con Authz 0.1.2.
- `cargo tree -i hodei-authz` → `hodei-authz v0.1.2`; sin `path`/`git`.
- `cargo run --manifest-path ../hodei-authz/examples/published-consumer/Cargo.toml`: `alice=Permit bob=Deny`.

## Límites

- `kernel::Hrn` y `hodei-hrn` coexisten por compatibilidad de persistencia; no se migran datos en este cambio.
- La paridad cubre fixtures; no demuestra rechazo idéntico para entradas malformadas.
