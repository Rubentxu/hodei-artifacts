# Guía de Organización de Tests (TEST-ORG1)

Versión: 1.1 (alineada con sección 16 de [`plan.md`](docs/plan.md))
Cambios 1.1: añade excepción clarificada para doctests, opción crate compartido `test-helpers`, recomendaciones `benches/`, comandos de filtrado `cargo test` y notas de cobertura.

Objetivo: Establecer una convención estricta, reproducible y automatizable para la localización, tipo y nivel de abstracción de las pruebas en el monorepo. Garantiza:
- Aislamiento (evitar acoplar tests a detalles internos).
- Señal temprana (rápida detección de regresiones).
- Escalabilidad (añadir bounded contexts y slices sin reabrir decisiones).
- Simplicidad de CI (jobs diferenciados sin lógica ad-hoc por crate).

---

## 1. Principios Rectores

1. Ningún test embebido dentro de `src/**`. Se elimina el uso de módulos `#[cfg(test)]` (política “cero inline tests”). Enforcement: script [`verify-no-inline-tests.sh`](scripts/verify-no-inline-tests.sh) (TEST-ORG4).
2. Caja negra sobre puertos públicos: los tests interactúan con traits de aplicación / dominio, no con structs internos privados salvo builders utilitarios.
3. Separación física y semántica por nivel:
   - Unit (sin I/O, lógica pura, conversiones doc↔entidad, validaciones).
   - Integration (adapters reales: Mongo, S3, Kafka) usando testcontainers o servicios provisionados.
   - E2E / Vertical Slice (flujo completo vía HTTP o comando top-level).
4. No exponer internals “sólo para test”. Se prohíben features como `test-internals` o tipos `pub(crate)` artificialmente ampliados a `pub`.
5. Determinismo y reproducibilidad: datos dinámicos (UUID, timestamps) encapsulados en builders que permiten fijar valores cuando las aserciones requieren igualdad exacta.
6. Fail Fast: ausencia de infraestructura externa esperada produce fallo explícito (no skip silencioso) salvo whitelists justificadas (actualmente vacías).
7. Cost Awareness: Costos de tiempo y recursos se limitan mediante:
   - Reutilización contenedor Mongo único + DB aleatoria por test (ver [`test-containers.md`](docs/test-containers.md)).
   - Jobs CI segmentados (unit vs integration vs futuro e2e).

---

## 2. Taxonomía de Directorios

Dentro de cada crate que requiera tests:

```
crates/<context>/
  src/
  tests/
    unit/      # Pruebas unitarias puras
    it/        # Integration (adapters reales)
    e2e/       # (Opcional) slice vertical interno
    support/   # Builders, fixtures, helpers reutilizables
```

Para pruebas cross-crate de la aplicación completa (HTTP, métricas, tracing) puede añadirse un nivel superior:
```
tests/
  e2e_service/
```
(Este todavía no se ha introducido; se hará cuando exista servidor HTTP unificado.)

---

## 3. Convenciones de Nombres

Prefijos obligatorios:
- `unit_*.rs` en `tests/unit/`
- `it_*.rs` en `tests/it/`
- `e2e_*.rs` en `tests/e2e/`

Ejemplos existentes:
- `it_repository_store_integration.rs` (se acepta sufijo descriptivo adicional).
- Archivo histórico deshabilitado: `_removed_unit_artifact_repository.rs.disabled` (conservado temporalmente para auditoría; no se compila).

Regla de legibilidad: el nombre comunica el “subject under test” + la característica (p.ej. `unit_repository_domain.rs`, `it_artifact_repository_mongo.rs`).

---

## 4. Criterios por Categoría

| Categoría | Permitido | Prohibido | Ejemplos de Aserciones |
|-----------|-----------|-----------|------------------------|
| Unit | Lógica pura, mapeos DTO↔Domain, validaciones regex, normalización de datos | I/O real, acceso a red, dependencias asíncronas externas | Creación entidad, error por nombre inválido |
| Integration | Adaptadores reales (Mongo, S3, Kafka), índices, comportamiento idempotente | Mocks de la misma tecnología persistente (no mockear driver) | Duplicado nombre repositorio produce error, carrera insert checksum |
| E2E / Slice | Flujo completo HTTP / comando vertical | Alterar configuración global fuera del scope del test | Upload → persist → evento (futuro) |

---

## 5. Estrategia de Mocks / Fakes

- Sólo se mockean puertos (interfaces definidas en `application::ports`), nunca modelos de dominio simples.
- Errores de infraestructura difíciles de reproducir (p.ej. fallos transitorios de publish) se simulan mediante implementación fake de puerto, ubicada en `tests/support/`.
- Se evita introducir traits adicionales en código productivo únicamente para estabilizar tests (regla: justificación arquitectónica o rechazo en PR).

---

## 6. Reglas de Aislamiento

1. Cada test de integración genera un namespace / base de datos lógico único (sufijo aleatorio).
2. Contenedor Mongo compartido (tiempo arranque amortizado) mediante helper central dentro de `infra-mongo` (feature `test-util`), documentado en [`test-containers.md`](docs/test-containers.md).
3. Limpieza de datos garantizada por uso de DB aisladas; no se requiere truncado entre tests.

---

## 7. Política “Cero Inline Tests”

Motivación:
- Evitar diffs ruidosos mezclando lógica de negocio y pruebas.
- Forzar que la API pública sea suficientemente expresiva para cubrir casos relevantes.
- Reducir tentación de hacer `pub` miembros privados o añadir ramas condicionales de test.

Enforcement:
- Script [`verify-no-inline-tests.sh`](scripts/verify-no-inline-tests.sh) (TEST-ORG4) falla si detecta patrones `#[cfg(test)]` o módulos `mod tests` bajo `src/**`.
- Whitelist inicial vacía. Excepciones futuras requerirán RFC interna + anotación en el script.

### 7.1 Doctests (Excepción Permitida)
Los doctests en comentarios de documentación (`///`) se permiten porque:
- Verifican ejemplos públicos sin acceder a internals.
- Refuerzan la calidad de la documentación.
Regla: si un doctest requiere acceder a algo no público, debe replantearse el ejemplo o la API (no se añaden `pub` “sólo para el doctest”).

---

## 8. Métricas y Objetivos Iniciales

| Métrica | Objetivo MVP |
|---------|--------------|
| Cobertura líneas dominio + adapters críticos | ≥ 70% (upload + repository) |
| p95 tiempo suite unit | < 50ms |
| p95 tiempo suite integración (Mongo) | < 5s (incl. arranque contenedor ya caliente) |
| Flakiness (re-runs) | 0 en main |

Cobertura se generará (CI-T6) sin gate inicial; gate se evaluará post MVP.

---

## 9. CI / Pipeline

Jobs planificados (ver WBS):
1. `test-unit`: `cargo test --all --lib --tests --no-default-features` (sin features de integración).
2. `test-integration`: activa features (`integration-mongo`, futuras `integration-kafka`) y ejecuta integración.
3. `test-e2e`: introducido cuando exista servidor HTTP consolidado.
4. Verificación organización tests: ejecuta `bash scripts/verify-no-inline-tests.sh` (CI-T7).

Orden recomendado en workflow:
- Lint / Format → Inline Test Verification → Unit → Integration → (E2E) → Coverage.

---

## 10. Anti-Patrones (Rechazar en PR)

| Anti-Patrón | Riesgo | Alternativa |
|-------------|--------|-------------|
| Añadir `#[cfg(test)]` en `src/**` | Fuga internals / diffs ruidosos | Crear test en `tests/unit/` usando API pública |
| Feature `test-internals` que expone structs privados | Aumenta superficie accidental | Refactor a puerto o mover helper a `tests/support/` |
| Mock del driver Mongo para simular duplicate key | Divergencia con comportamiento real | Test de integración con índice único real |
| Creación de múltiples contenedores Mongo por test | Penalización tiempo | Reutilizar helper compartido |
| Mocks anidados complejos (múltiples capas) | Fragilidad, falso positivo | Tests de integración vertical mínima |

---

## 11. Registro de Decisiones (Changelog de Organización)

| Fecha | Decisión | Referencia |
|-------|----------|------------|
| 2025-08-XX | Eliminación total de `test-internals` y traits de inyección para tests | TEST-ORG2 / TEST-ORG3 |
| 2025-08-XX | Adopción política “cero inline tests” + script verificación | TEST-ORG4 |
| 2025-08-XX | Normalización prefijos `unit_` / `it_` / `e2e_` | TEST-ORG5 |
| 2025-08-24 | Versión 1.1: excepción permitida doctests, guía helpers compartidos, benchmarks y comandos filtrado/cobertura | TEST-ORG1 |

(Actualizar fechas exactas en commit.)

---

## 12. Flujo para Añadir Nuevo Tipo de Test

1. Proponer necesidad (p.ej. nuevo adapter Redis).
2. Extender helper de infraestructura (feature `test-util` en crate dedicado).
3. Añadir categoría si difiere (ej. performance) en directorio separado `tests/perf/` (requiere RFC).
4. Actualizar esta guía (nueva versión semántica, sección 13).
5. Añadir job CI si requiere pipeline distinto.

---

## 13. Roadmap Post-MVP

| Mejora | Descripción |
|--------|-------------|
| Flakiness Tracker | Re-ejecuciones automáticas de tests inestables con etiquetado |
| Test de Carga Ligero | Escenarios básicos de throughput (no aún en MVP) |
| Métricas por Categoría | Publicar tiempos agregados y trending en job summary |
| Analizador Mutaciones | Introducir mutación selectiva en dominio crítico (checksum, validaciones) |

---

## 14. Uso de Builders y Fixtures

- Ubicar en `tests/support/builders.rs` (o submódulos).
- Patrón:
  ```rust
  pub struct RepositoryBuilder {
      name: String,
      description: Option<String>,
  }

  impl RepositoryBuilder {
      pub fn new() -> Self { Self { name: "repo_demo".into(), description: None } }
      pub fn with_name(mut self, v: impl Into<String>) -> Self { self.name = v.into(); self }
      pub fn build(self) -> Repository { Repository::new(self.name, self.description) }
  }
  ```
- Reglas: sin efectos secundarios (no I/O) y sin acceso estático global.

---

## 15. Ejemplo de Test de Integración Adecuado (Mongo Duplicate Key)

Pseudo-estructura (simplificada):

```rust
#[tokio::test]
async fn it_duplicate_checksum_returns_duplicate_error() {
    let (repo, _c) = build_artifact_repository_for_test().await;
    // arrange: crear Artifact base
    // act: insertar dos veces con mismo (repository_id, checksum)
    // assert: segunda inserción devuelve ArtifactError::Duplicate
}
```

Aspectos clave:
- Usa helper compartido.
- No manipula internals del adapter (sólo el puerto).
- Confirma semántica idempotente real basada en índice único.

---

## 16. Checklist de Cumplimiento (para revisión PR)

Antes de merge:
- [ ] Sin nuevos `#[cfg(test)]` en `src/**`.
- [ ] Nuevos archivos de test usan prefijo correcto.
- [ ] Helpers nuevos ubicados en `tests/support/`.
- [ ] Sin cambios de visibilidad `pub` no justificados por uso en producción.
- [ ] Tiempo local suite unit no incrementa notoriamente (< objetivo p95).
- [ ] Si se introduce dependencia externa en tests, documentada en este archivo.

---

## 17. Mantenimiento

Responsable inicial: Tech Lead del repositorio.  
Proceso de cambio: abrir PR con etiqueta `test-architecture` + actualización sección 11 (registro).  
Versionado: bump menor si se añaden reglas; bump mayor si se relajan restricciones (excepción excepcional).

---

## 18. Referencias

- Estrategia original: sección 16 de [`plan.md`](docs/plan.md#L239).
- Integración testcontainers: [`test-containers.md`](docs/test-containers.md).
- Adaptador Mongo Artifact: [`mod.rs`](crates/artifact/src/infrastructure/persistence/mod.rs).
- Factoría Mongo reusable: [`client.rs`](crates/infra-mongo/src/client.rs).

---

## 19. Helpers Compartidos (Crate Opcional)
Si el volumen de builders y utilidades crece, puede crearse un crate `test-helpers` (solo `[dev-dependencies]` en los demás crates).
Ventajas:
- Evita declarar `mod support;` repetido.
- Reutiliza builders consistentes (p.ej. `RepositoryBuilder`, `ArtifactBuilder`).
Reglas:
1. El crate no debe depender de código privado (solo API pública del workspace).
2. No introducir dependencias pesadas (mantener inicio rápido).
3. No colocar mocks de infraestructura real (seguir principio de integración real para adapters).

## 20. Benchmarks
Benchmarks (post-MVP) ubicados en `benches/` por crate:
- Ejecutar con: `cargo bench`.
- No mezclar micro-benchmarks con tests de performance de sistema (estos últimos irían a otra suite orientada a carga).
Política inicial: no obligatorios hasta que exista hot path identificado (telemetría).

## 21. Comandos Útiles Cargo Test
Filtrado por prefijo de archivo (integration / unit):
- Unit solamente:
  `cargo test --test 'unit_*'`
- Integración solamente:
  `cargo test --test 'it_*'`
- Archivo específico:
  `cargo test --test it_artifact_repository_mongo`
- Doctests solamente:
  `cargo test --doc`

## 22. Cobertura
Herramientas recomendadas:
- `cargo tarpaulin --workspace --ignore-tests` (más simple).
- Alternativa `grcov` (recolectando `LLVM_PROFILE_FILE` + `cargo test`).
Requisitos:
1. No manipular flags de compilación que oculten branches críticos.
2. Excluir (si se parametriza) código generado y soporte de test para mantener señal.
Objetivo futuro gate: ≥ 70% dominios críticos (upload, repository) manteniendo evolución incremental.

---

Fin del documento (TEST-ORG1).