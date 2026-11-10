# Dependencias publicadas y paridad HRN

HSP-18 sustituye la dependencia git de `hodei-authz` por la versión publicada `0.1.1` con pin exacto (`=0.1.1`) y `Cargo.lock` versionado. El build de Artifacts ya no necesita un checkout hermano de `hodei-authz`.

## Estado

- `hodei-authz = "=0.1.1"`; `Cargo.lock` se versiona para que la resolución sea reproducible.
- `hodei-hrn = "0.2.0"` y `hodei-hrn-01 = 0.1.0` (alias de paquete) son `dev-dependencies` de paridad.
- El código de producción usa el `Hrn` reexportado por `hodei-authz`, que a su vez resuelve `hodei-hrn 0.1.0`.
- `kernel::Hrn` se mantiene: está serializado con serde en documentos IAM persistidos y es la clave de almacenamiento de repositorios (`hrn.to_string()`). Cambiarlo alteraría persistencia/contrato sin una decisión explícita (AC4).
- `tests/hrn_parity.rs` compara dos capas:
  1. los fixtures persistidos contra `hodei-hrn 0.1.0`, la versión real del path de autorización;
  2. los mismos fixtures más un path anidado contra `hodei-hrn 0.2.0`.
  Compara partition, service, región, cuenta/tenant, tipo, resource path, serialización y alias v1.
- La equivalencia se limita a los fixtures listados; no se afirma paridad byte a byte para layouts que el parser 0.1 nunca aceptó ni para el rechazo de entradas malformadas.

## Deuda declarada

- Coexisten `kernel::Hrn`, `hodei-hrn 0.1.0` (authz) y `hodei-hrn 0.2.0` (contrato). Retirar el adapter histórico exige subir `hodei-authz` a `hodei-hrn 0.2.0` y migrar la conversión manual de `src/authorization.rs`; no se hace en este cambio.
- La paridad demuestra que la retirada es posible para los fixtures cubiertos, no que sea gratuita.

## Verificación

- `cargo test --test hrn_parity`: 2/2.
- `cargo test --workspace`: verde con la dependencia de registry.
- `cargo check --workspace` desde worktree limpio de HEAD: OK; `cargo tree -i hodei-authz` → `hodei-authz v0.1.1`.
- Sin `path`/`git` para `hodei-authz` en `Cargo.toml`; sin `[patch]`.

## Límites

- La publicación de los crates es HSP-3/HSP-9.
- No se migran datos persistidos en este cambio.
