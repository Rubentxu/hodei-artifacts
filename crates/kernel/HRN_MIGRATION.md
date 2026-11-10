# Migración HRN en Artifacts

`kernel::domain::hrn_migration` es el adaptador **source-aware** para leer HRN persistidos sin reinterpretar el ámbito ni cambiar los bytes guardados.

## Por qué

El parser canónico acepta el layout v2 y su alias v1 (región vacía), pero hay datos históricos cuyo significado de campo no se puede inferir solo de la cadena:

- `hodei-authz` 0.1 usa un `tenant` en la posición de cuenta.
- El servicio antiguo de repositorios era `repository`; el canónico es `artifacts`.

Convertir esas formas “porque el parser las acepta” cambiaría el scope o el servicio en silencio. El adaptador exige declarar el origen y, cuando aplica, la cuenta esperada.

## API

```rust
use kernel::domain::hrn_migration::MigratedHrn;

let migrated = MigratedHrn::from_artifacts(
    "hrn:hodei:artifacts:eu-west-1:acme:repository/releases",
)?;
assert_eq!(migrated.canonical().account_id(), "acme");
assert_eq!(migrated.stored(), "hrn:hodei:artifacts:eu-west-1:acme:repository/releases");

let legacy = MigratedHrn::from_legacy_repository(
    "hrn:hodei:repository:eu-west-1:acme:repository/releases",
    "acme",
)?;
assert_eq!(legacy.canonical().service(), "artifacts");
assert_eq!(
    legacy.legacy_repository_string().as_deref(),
    Some("hrn:hodei:repository:eu-west-1:acme:repository/releases")
);
```

- `from_artifacts`: exige `partition = hodei`, `service = artifacts` (v1/v2). `stored` conserva los bytes; `canonical()` es el `Hrn` tipado. Un HRN IAM u otro servicio se rechaza con `NotArtifacts`.
- `from_authz_v01(stored, confirmed_account)`: exige `partition = hodei`, `service = iam`; el llamante **confirma** que el tenant de authz 0.1 es la cuenta en ese despliegue. Si no coincide, `ScopeMismatch`; si el layout no es authz, `WrongSource`. No se infiere ni se equipara tenant=account automáticamente.
- `from_legacy_repository(stored, expected_account)`: exige `partition = hodei`, `service = repository` y `resource_type = repository`, conserva cuenta/región/tipo/path y produce el canónico `artifacts`. `legacy_repository_string()` solo existe para un `MigratedHrn` leído como `LegacyRepository`; no degrada un canónico.
- Un servicio distinto no se reescribe: `NotLegacyRepository`.

## Reglas

1. **Dual-read, no dual-authorize**: el adapter separa `stored()` de `canonical()`; cuando se cablee (HSP-13), la autorización debe usar `canonical()`. Hoy el adapter no tiene consumidor de producción.
2. **No se adivina el origen**: sin `from_*` explícito no hay conversión.
3. **La cuenta forma parte del scope**: dos cuentas con el mismo id local producen `scoped_entity_id()` distintos.
4. **Los paths anidados se conservan**: el parser canónico no recorta `resource_id`.
5. **La API de negocio recibe `Hrn`**, no detalles de parsing; el cableado real es HSP-13.

## Fixtures y pruebas

`tests/fixtures/hrn_migration.json` contiene v2, v1, authz 0.1, legacy repository y path anidado, con `expected_account` explícito cuando aplica. `tests/hrn_migration.rs` verifica bytes almacenados, canónico, origen, aislamiento de cuentas en cada fuente, mismatch, fuentes incorrectas, normalización de caja y reversibilidad. El adapter expone un `Hrn` tipado; el cableado real de negocio es HSP-13.

## Colisión de representaciones

Un dato legacy `repository` y su canónico `artifacts` con la misma región/cuenta/tipo/path representan la misma identidad lógica pero **dos cadenas persistidas distintas**. El adapter no las fusiona; un dual-read que mantenga ambas debe elegir el canónico para autorizar y no permitir que una sombree a la otra. La migración de almacenamiento es explícita.

## Límites

- No reescribe datos persistidos en bloque; la migración de almacenamiento es una tarea de despliegue.
- No detecta automáticamente Audit Trail ni orígenes intercambiados; requieren su propio adapter.
- El adaptador no autoriza; solo prepara la identidad canónica para el dominio/autorizador.
