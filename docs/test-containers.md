# Plan Estrategia Unificada de Tests de Integración con testcontainers (Mongo)

## 1. Objetivos
- Eliminar dependencia manual de una instancia Mongo local para ejecutar tests de integración.
- Permitir que los tests usen variables MONGO_* existentes (modo CI pre-provisionado) o arranquen un contenedor efímero automáticamente.
- Proveer un helper único reutilizable por todos los bounded contexts que necesitan Mongo.
- Mantener aislamiento entre tests (DB aleatoria/namespace).
- Minimizar boilerplate en cada test → patrón build_store() centralizado.
- Facilitar futura extensión a otros servicios (Kafka, S3 minio) con la misma abstracción.

## 2. Alcance (fase actual)
Cubre RepositoryStore y ArtifactRepository (persistencia sobre Mongo). Queda fuera (futuro):
- SearchIndex (cuando se implemente).
- SupplyChainStore.
- Métricas, eventos y demás dependencias externas no Mongo.

## 3. Dependencias
Añadir crate `testcontainers` (versión estable actual) a:
- `[dev-dependencies]` de cada crate que tenga tests de integración con feature `integration-mongo`.
- Alternativa: centralizar en workspace `[workspace.dependencies]` y referenciar igual que `tokio`.

## 4. Diseño Helper
Nuevo módulo en infra-mongo bajo feature `test-util`:
- Archivo: `crates/infra-mongo/src/test_util/mongo_test_container.rs`
- Funciones expuestas:
  - `async fn ephemeral_store() -> (MongoClientFactory, Option<Container<'static, GenericImage>>)`
    - Si `MongoConfig::from_env()` ( [MongoConfig::from_env](crates/infra-mongo/src/config.rs:28) ) funciona → retorna factory sin contenedor.
    - Si falla → levanta contenedor Mongo `mongo:6.0`, genera DB aleatoria, construye config y retorna `Some(container)`.
  - `fn ensure_docker_available() -> Result<(), String>` para logging / skip anticipado opcional (no se usará inicialmente, preferimos fallo explícito).
- Internamente:
  - Cliente Docker global estático vía `OnceLock`.
  - Imagen configurada con `WaitFor::message("Waiting for connections")`.
  - Puerto mapeado dinámicamente → construir URI `mongodb://127.0.0.1:{host_port}`.
  - Base de datos: `it_{context}_{rand_u64}`.

Reutiliza tipos existentes:
- [MongoClientFactory](crates/infra-mongo/src/client.rs:35)
- [MongoDatabaseHandle](crates/infra-mongo/src/client.rs:21)

## 5. API Simplificada para Tests
Se añade un wrapper de conveniencia:

```rust
pub async fn build_repository_store_for_test() -> (MongoRepositoryStore, Option<TestMongoContainer>)
```

Donde `TestMongoContainer` es un struct liviano que envuelve el `Container` para mantenerlo vivo (Drop detiene contenedor).  
Análogo para Artifact:

```rust
pub async fn build_artifact_repository_for_test() -> (MongoArtifactRepository, Option<TestMongoContainer>)
```

Estas funciones vivirán (temporalmente) dentro de `infra-mongo::test_util` para evitar duplicación en cada crate consumidor.

## 6. Patrón de Uso en Test
Ejemplo (RepositoryStore):

```rust
#[tokio::test]
async fn duplicate_name_returns_error() {
    let (store, _c) = build_repository_store_for_test().await;
    // uso normal del trait RepositoryStore (importar RepositoryStore)
}
```

Importante: Cada test mantiene su propio `_c` (opcional) en el scope para que el contenedor no se destruya antes de finalizar.

## 7. Estrategia Aislamiento
- Mismo contenedor compartido entre tests o contenedores por test?
  - Opción elegida: CONTENEDOR ÚNICO + DB aleatoria por test (más rápido).
  - Implementación: el helper mantiene contenedor global (OnceLock) y cada llamada genera nueva `MongoConfig.database`.
- Justificación: Minimiza overhead de arranque (Mongo tarda ~2–4s frío) y mantiene aislamiento lógico.

## 8. Detección Modo CI
Orden de preferencia:
1. Si `MONGO_URI` y `MONGO_DATABASE` existen → no iniciar Docker (caso: CI con servicio docker-compose pre-creado).
2. Else → usar testcontainers.

Esto permite pipelines duales inicialmente y migración escalonada.

## 9. Integración en CI (GitHub Actions Ejemplo Simplificado)
(Documento no crea workflow ahora; se implementará en CI-T2 más adelante)

```yaml
services:
  mongo:
    image: mongo:6.0
    ports: ["27017:27017"]

env:
  MONGO_URI: mongodb://localhost:27017
  MONGO_DATABASE: hodei_ci
```

Escenario alternativo: no definir servicio → testcontainers lanza el contenedor de forma autónoma (requiere Docker in Docker / privileged runner).

## 10. Fases de Migración

| Fase | Acción | PR Objetivo |
|------|--------|-------------|
| 1 | Añadir dependencia testcontainers en workspace (dev) | TEST-TC2 |
| 2 | Implementar helper `mongo_test_container.rs` + build_repository_store_for_test | TEST-TC3 |
| 3 | Refactor tests RepositoryStore a usar helper (eliminar lógica local actual) | TEST-TC4 |
| 4 | Añadir build_artifact_repository_for_test y refactor tests artifact (cuando existan) | TEST-TC5 |
| 5 | Documentar en `docs/test-containers.md` (este archivo) y link desde `docs/plan.md` | TEST-TC6 |
| 6 | Ajustar pipeline CI (decidir usar servicio mongo fijo o modo dinámico) | TEST-TC7 |
| 7 | Extender patrón a futuros adapters (search, supply-chain) | Futuro (nuevas tasks) |

## 11. Cambios Concretos en Código

1. `Cargo.toml` (workspace / crates que testean):
   - Agregar (dev):
     ```
     testcontainers = { version = "0.15", features = ["experimental"] }
     ```
     (La feature concreta se validará; si no requerida, se omite.)
2. Nuevo archivo: `crates/infra-mongo/src/test_util/mongo_test_container.rs`
3. Re-export en `crates/infra-mongo/src/lib.rs` dentro de `#[cfg(feature = "test-util")]`.
4. Refactor de tests en `crates/repository/tests/it_repository_store_integration.rs`:
   - Reemplazar `build_store()` local por helper `build_repository_store_for_test()`.
   - Remover referencias directas a `MongoConfig::from_env()` en test.
5. Crear (si no existe) tests de integration artifact que reutilicen el helper (posterior a Artifact tests plan).

## 12. Errores y Logging
- Si Docker no está disponible y no existen MONGO_* → fallo temprano con mensaje claro.
- Añadir log de modo seleccionado (`info!`: "integration tests using ephemeral mongo container at {uri}" / "using pre-provisioned mongo env").
- Evitar logs ruidosos innecesarios (usar `tracing` existente).

## 13. Riesgos y Mitigaciones
| Riesgo | Impacto | Mitigación |
|--------|---------|------------|
| Overhead inicial arranque contenedor | Aumenta tiempo tests ~3s | Reutilización global de contenedor |
| Contención en puerto si servicio CI también corre Mongo | Falla de conexión / colisiones | Precedencia: env primero, contenedor sólo si falta env |
| Limpieza de datos entre tests | Cross-test interference | DB aleatoria por test (nombre único) |
| Fugas de contenedor si panic antes de Drop | Recursos residuales | Uso contenedor global deliberado; termina al finalizar job |

## 14. Extensibilidad Futura
- Añadir rasgo genérico `TestServiceSpawner` para otros servicios.
- Cacheo multi-servicio (Mongo + MinIO + Kafka) con un registro global.

## 15. Checklist Implementación (ligado a tasks)
- [ ] TEST-TC2 Dependencia testcontainers añadida.
- [ ] TEST-TC3 Helper mongo_test_container.rs creado.
- [ ] TEST-TC4 RepositoryStore tests refactorizados.
- [ ] TEST-TC5 ArtifactRepository tests refactorizados.
- [ ] TEST-TC6 Documentación (este archivo) enlazada en plan general.
- [ ] TEST-TC7 Pipeline CI ajustado y validado.

## 16. Decisiones Clave Resumidas
- Contenedor único + DB por test.
- Fallback automático si faltan variables.
- Helper centraliza toda la lógica.
- No se fuerza skip silencioso: fallos explícitos para visibilidad.

## 17. Próximo Paso Inmediato
Implementar TEST-TC2 (agregar dependencia) y crear helper base vacío compilable antes de refactor.
