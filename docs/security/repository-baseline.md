# Baseline de la API Artifacts y del dominio `repository`

**Tarea:** HSP-11 (HSEC-AR-02). **Fecha:** 2026-11-10. **Alcance:** caracterizar la superficie HTTP activa y el crate `repository` desconectado, y decidir el seam para el primer `GetRepository` seguro. **No implementa** ese caso de uso; eso es HSP-15.

## 1. Resumen ejecutivo

- El workspace activo tiene **4 miembros**: `kernel`, `hodei-iam`, `hodei-policies` y `hodei-artifacts-api`.
- `cargo check --workspace` pasa y `cargo test --workspace` deja **820 tests pasando, 0 fallando y 64 ignorados** (doctests).
- La API activa expone salud, Swagger y **12 rutas administrativas** bajo `/api/v1` (IAM, políticas, esquemas y playground). Todas están cerradas por defecto con `403` desde HSP-10; no existe ninguna ruta de repositorios.
- El crate `crates/repository` **no es miembro del workspace** y no compila ni como paquete independiente: depende de `shared` (crate inexistente; fue renombrado a `kernel`) y de `mongodb` (no declarado en el workspace). Salió del workspace en `53026c3`, no con el rename posterior.
- Decisión: **KEEP_AS_IS** para `crates/repository` como referencia histórica. El primer `GetRepository` se hará con un **seam nuevo y reversible**: un puerto `RepositoryReader`, un DTO mínimo y una ruta con gate propio (no registrada hasta HSP-15), sin reactivar el dominio legacy ni Mongo.

## 2. Baseline ejecutado

Entorno: Linux, Rust 1.99, workspace `hodei-artifacts` sobre el código de `main` en `9eb2bce`. Este commit solo añade documentación y el test de ausencia de ruta, por lo que los números de código son reproducibles desde `9eb2bce`.

| Comando | Resultado |
| --- | --- |
| `cargo metadata --no-deps` | 4 paquetes: `kernel`, `hodei-iam`, `hodei-policies`, `hodei-artifacts-api` |
| `cargo check --workspace` | OK |
| `cargo test --workspace` (código `9eb2bce`) | **820 passed; 0 failed; 64 ignored** |
| `cargo check --manifest-path crates/repository/Cargo.toml` | **FALLA** (exit 1). El error observado es `dependency.mongodb` no está en `workspace.dependencies`; además el crate no es miembro y su `shared` no existe |
| `test -d crates/shared` | **no existe** |
| `grep -R "shared::" crates/repository/src` | **73 líneas (78 ocurrencias)** |

Desglose de tests activos:

| Suite | Passed |
| --- | --- |
| `hodei-artifacts-api` (lib, main, admin, auth isolation, binary, bootstrap, distribution, http, legacy quarantine, module tree, schema persistence) | 96 |
| `hodei-iam` (lib + 3 suites de integración) | 254 |
| `hodei-policies` | 222 |
| `kernel` (lib + SurrealDB integration) | 242 |
| Doc-tests de `kernel` | 6 |
| **Total en `9eb2bce`** | **820** |

La rama de este documento añade `repository_route_is_not_served`, por lo que en la rama la suite activa es **821 passed / 0 failed / 64 ignored**.

`crates/repository` no aporta tests al baseline: sus 4 tests de caso de uso no se compilan nunca.

## 3. Superficie HTTP activa

### Públicas (sin autenticación)

| Método | Ruta | Uso |
| --- | --- | --- |
| GET | `/health`, `/health/ready`, `/health/live` | Sondas |
| GET | `/swagger-ui`, `/api-docs/openapi.json` | Documentación (contrato, no datos) |

### Administrativas, bajo `/api/v1` y `admin_api_enabled` (default `false`)

| Método | Ruta | Dominio |
| --- | --- | --- |
| POST | `/api/v1/schemas/build` | Esquema Cedar |
| GET | `/api/v1/schemas/load` | Esquema Cedar |
| POST | `/api/v1/schemas/register-iam` | Esquema Cedar |
| POST | `/api/v1/policies/validate` | Políticas |
| POST | `/api/v1/policies/evaluate` | Políticas |
| POST | `/api/v1/iam/policies` | IAM |
| GET | `/api/v1/iam/policies` | IAM |
| POST | `/api/v1/iam/policies/get` | IAM |
| PUT | `/api/v1/iam/policies/update` | IAM |
| DELETE | `/api/v1/iam/policies/delete` | IAM |
| POST | `/api/v1/iam/evaluate` | IAM |
| POST | `/api/v1/playground/evaluate` | Playground |

Con la configuración por defecto todas devuelven `403` con cuerpo `administrative API disabled` **antes** de ejecutar el handler. El opt-in `admin_api_enabled=true` las expone sin autenticación: es una medida temporal de HSP-10, no una frontera de seguridad definitiva.

**No hay ruta de repositorios**, ni en `/api/v1` ni fuera. `AppState` no tiene ningún puerto de repositorios y `CompositionRoot` no ensambla ninguno.

Matiz del gate: la capa administrativa se aplica a **todo** `/api/v1`, no solo a las 12 rutas. Una ruta desconocida bajo el prefijo responde `403` con `admin_api_enabled=false` y `404` con el flag activo; el test `repository_route_is_not_served` fija que la ruta de repositorios no se sirve en ninguno de los dos casos.

El repositorio contiene además un **registry inactivo en el contrato OpenAPI**: `docs/openapi/paths/repositories.yaml` publica listar/crear/obtener/actualizar por UUID bajo `/v1/repositories`, con `ApiKeyAuth`/`BasicAuth`/`BearerAuth`. No está montado en Axum. La ruta propuesta aquí (`GET /api/v1/repositories?hrn=...`) no reconcilia todavía con esa especificación; HSP-12/HSP-15 deben decidir el contrato definitivo.

## 4. `crates/repository`: inventario y bloqueos

### Inventario

- 44 archivos (43 `.rs`), organizados en 4 features verticales: `create_repository`, `get_repository`, `update_repository`, `delete_repository`.
- Cada feature tiene `api`, `di`, `dto`, `ports`, `use_case` y `use_case_test`.
- Dominio: `repository`, `policy`, `storage`, `unified`, `events`, `error`.
- Infraestructura: `mongodb_adapter`, `unified_adapter`, `api`, `ports`.
- **9 funciones de test** en total: 4 en `use_case_test` (una por feature) y 5 en módulos no declarados. Ninguna se compila en el workspace activo.
- **4 módulos huérfanos** (`domain/unified.rs`, `infrastructure/api.rs`, `infrastructure/ports.rs`, `infrastructure/unified_adapter.rs`) no están declarados en `mod.rs`.

### Bloqueos verificados

1. **No es miembro del workspace**: `Cargo.toml` raíz solo declara `kernel`, `hodei-iam`, `hodei-policies` y `.`. `cargo check --manifest-path crates/repository/Cargo.toml` falla antes de compilar.
2. **`shared` no existe**: 73 líneas (78 ocurrencias) con `shared::hrn`, `shared::enums`, `shared::lifecycle`. `crates/repository` salió del workspace en `53026c3`; después, `shared` se renombró a `kernel` en `9fffbd3`, pero el dominio legacy no se actualizó.
3. **Tipos ausentes en `kernel`**: no hay `RepositoryId`, `OrganizationId`, `Lifecycle` ni `Ecosystem`; `kernel` solo aporta `Hrn`, entidades y value objects de IAM. Portar el dominio exige recrear o adaptar esos tipos.
4. **MongoDB**: 28 líneas con referencias a Mongo y una dependencia `mongodb` que no está en `workspace.dependencies`. La persistencia activa es SurrealDB sobre RocksDB, no Mongo. El `RepositoryAuthorizationAdapter` de producción devuelve `Ok(true)` (allow-all) en `get_repository/di.rs`.
5. **Persistencia genérica incompleta**: `SurrealDbGenericRepository::read` devuelve `Ok(None)` (placeholder); no hay tabla ni adapter real de repositorios.
6. **HRN incompatible**: el dominio legacy documenta `hrn:hodei:repository:<region>:<org_id>:repository/<name>` (servicio `repository`), mientras el contrato canónico y el kernel usan `hrn:hodei:artifacts:<region>:<account>:repository/<name>` (servicio `artifacts`). Reutilizar `RepositoryId` perpetúa la divergencia que HSP-12 debe resolver.
7. **Autorización embebida y contexto artificial**: `GetRepositoryUseCase` crea `OrganizationId::new("system")` con un `TODO` y llama a `RepositoryAuthorizationPort`, cuyo adaptador de producción es allow-all; no hay principal verificado ni account de confianza. `domain/unified.rs` contiene además un ejemplo `Private/Internal` que permite acceso (ya señalado en el baseline previo).

### Coste de reactivación

| Trabajo | Estimación |
| --- | --- |
| Restaurar `shared` o migrar 73 líneas a `kernel` | Alto |
| Recrear `RepositoryId`/`OrganizationId`/`Lifecycle`/`Ecosystem` o adaptarlos a `Hrn` | Medio-alto |
| Sustituir Mongo por SurrealDB/RocksDB (28 referencias) | Medio-alto |
| Reconciliar el formato HRN `repository` vs `artifacts` | Alto; afectaría a cualquier identificador persistido futuro (hoy no hay tabla de repositorios) |
| Añadir miembro al workspace, compilar, reparar tests | Medio |
| Cablear AppState/CompositionRoot/router y autenticación | Medio |
| **Resultado** | 4 features reactivadas, alto riesgo de arrastrar deuda y duplicar HRN |

## 5. Opciones y decisión

| Opción | Coste | Riesgo | Decisión |
| --- | --- | --- | --- |
| Reactivar `crates/repository` completo | Alto | Alto: shared, Mongo, HRN divergente, allow-all | **No** |
| Reactivar solo `get_repository` con un shim `shared` | Medio-alto | Alto: sigue necesitando Mongo, tipos y HRN legacy | **No** |
| Extraer el dominio a un crate nuevo antes del primer GET | Alto | Alto; retrasa el vertical slice | **No ahora** |
| Nuevo slice `GetRepository` con puerto + DTO + ruta | Medio | Bajo si se separa del legacy; reversible | **Sí** |
| KEEP_AS_IS del crate legacy + slice nuevo | Nulo en el legacy | Deuda histórica se conserva, no se usa en runtime | **Sí, decisión** |

## 6. Seam activo elegido: `RepositoryReader`

El crate legacy ya tiene un `RepositoryReaderPort` y dos handlers, pero están desconectados y no compilan. El seam **activo** es uno nuevo en la capa de aplicación del crate raíz, extraíble después a un bounded context propio; no se reutiliza el puerto legacy para no arrastrar `shared`, Mongo ni el HRN divergente:

```rust
#[async_trait]
pub trait RepositoryReader: Send + Sync {
    async fn find_by_hrn(
        &self,
        account: &str,          // del principal verificado, nunca de la query
        hrn: &kernel::Hrn,
    ) -> Result<Option<RepositoryView>, RepositoryReadError>;
}

#[derive(Debug, Clone)]
pub struct RepositoryView {
    pub hrn: kernel::Hrn,
    pub account: String,
    pub name: String,
    pub description: Option<String>,
}
```

- **Ruta**: `GET /api/v1/repositories?hrn=<percent-encoded-hrn>`. Se usa query, no path, porque el HRN contiene `:` y `/` y no debe someterse a normalización de path. La query se decodifica **una vez** y se parsea estrictamente.
- **No se registra todavía**: hasta que HSP-15 conecte identidad y autorización, la ruta no se monta. **No** se acopla a `admin_api_enabled`; activar IAM no debe abrir una ruta de negocio. El test `repository_route_is_not_served` fija que con `admin_api_enabled=true` la ruta sigue sin responder `200`.
- **Estado**: `Arc<dyn RepositoryReader>` en `AppState` y en `CompositionRoot`; no se toca el estado de IAM/policies.
- **Adaptador**: en HSP-15, `SurrealRepositoryReader` (tabla `repository`) o un `InMemoryRepositoryReader` determinista para tests. El contrato del puerto no obliga a Mongo ni al dominio legacy.
- **Identidad**: la cuenta se deriva del principal verificado (`AuthContextProvider` de `kernel`, hoy sin adaptador). El handler **no** usa el `account` de la query como autoridad; lo compara con la fila leída y consulta por `(account, resource_path)`. El `RepositoryReader` recibe la cuenta ya verificada, no confía en el HRN del cliente para el scope.
- **Contrato HTTP**: sin principal → `401`; recurso inexistente, de otra cuenta o denegado → **`404` uniforme** para no filtrar existencia; `403` solo cuando el contrato permita revelar la existencia. No se hace “cargar → 403” con un recurso ajeno.
- **HRN**: mientras Artifacts use `kernel::Hrn`, el handler debe exigir `partition = hodei`, `service = artifacts`, `resource_type = repository`, longitudes y forma canónica; `kernel::Hrn::from_string` es más laxo que el contrato. HSP-12 sustituye esta validación por `hodei-hrn` 0.2 o por un wrapper estricto. La clave de almacenamiento es la serialización canónica, nunca la query cruda ni `resource_id` como path.
- **Reversibilidad**: añadir el puerto, un handler y una ruta; retirarlos no requiere migración de datos. El dominio legacy no se toca.

### Por qué es un único seam activo

- No cambia `AppState` de IAM/policies, ni el esquema de persistencia, ni el gate administrativo: la ruta tiene su propio estado cerrado y no se registra hasta HSP-15. El `RepositoryReaderPort` legacy existente no se conecta.
- No reintroduce `shared`, Mongo ni el `RepositoryId` legacy.
- Permite que HSP-12 (HRN canónico) sea un requisito del adaptador, no un refactor global.
- HSP-16 (CreateRepository) puede implementar el mismo almacén y añadir escritura transaccional sin rehacer la lectura.

## 7. Desviación consciente de HSP-15

HSP-15 dice “rehabilitar solo el mínimo del crate repository necesario”. Este baseline concluye que ese mínimo no es barato ni reversible: obliga a `shared`, tipos ausentes, Mongo y un HRN `repository` distinto del canónico. La recomendación es **actualizar el alcance de HSP-15** a un slice nuevo con `RepositoryReader`; se registrará como comentario en la tarea antes de HSP-15. No se crea una tarea nueva ni se toca el crate legacy.

## 8. Plan derivado

| Tarea | Uso de este baseline |
| --- | --- |
| HSP-12 | Adoptar HRN canónico `artifacts` y conservar datos; el seam exige `kernel::Hrn` estricto o `hodei-hrn` 0.2 |
| HSP-15 | Implementar `RepositoryReader`, adaptador, ruta `GET /api/v1/repositories?hrn=...`, identidad, autorización y `404` uniforme |
| HSP-16 | Añadir `CreateRepository` con el mismo almacén, invariantes y atomicidad |
| HSP-18 | Retirar duplicación de HRN y decidir el destino de `crates/repository` tras paridad |

## 9. Riesgos y límites

- No existe todavía una tabla/almacén de repositorios en producción: HSP-15 debe definir el adaptador y los fixtures; HSP-16 aporta la escritura.
- El baseline se ejecutó con el workspace activo; `crates/repository` queda **NOT_RUN** por diseño (no compila).
- El opt-in `admin_api_enabled=true` sigue sin autenticación; la ruta de repositorios no se registra hasta HSP-15/HSP-8 y no se abre con ese flag.
- El riesgo residual es **medio** hasta que existan autenticación, autorización y validación HRN estricta; el seam no es una frontera de seguridad por sí solo.
- El contrato OpenAPI estático ya publica `/v1/repositories` por UUID con esquemas de auth, mientras Axum no sirve ninguna ruta de repositorios. HSP-12/HSP-15 deben reconciliar path, identidad y auth; este baseline no lo resuelve.
- La decisión no elimina el crate legacy; solo evita convertirlo en dependencia runtime antes de demostrar paridad.
- El baseline se ejecutó sobre el código de `9eb2bce`; este commit solo añade documentación y el test de ausencia de ruta, por lo que los números son reproducibles desde ese código. No se guardó un log crudo; las cifras salen de la ejecución `cargo test --workspace` y del desglose de `test result` del 2026-11-10.
