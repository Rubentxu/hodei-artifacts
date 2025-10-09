# 📋 Checklist Interactivo - Completar Refactorización

**Progreso Actual:** 38% ✅ | **Pendiente:** 62% ⏳  
**Tiempo Estimado:** 6-7 horas

---

## 🎯 FASE 1: Desbloquear Compilación (3.5 horas)

### ✅ Completado (3 horas)

- [x] Implementar CompositionRoot pattern
- [x] Refactorizar AppState con `from_composition_root()`
- [x] Simplificar bootstrap.rs (eliminar `create_use_cases`)
- [x] Migrar hodei-policies (7 features)
- [x] Corregir handlers de hodei-policies (validate, evaluate, register)

### ⏳ Pendiente - Crítico (30 minutos)

- [ ] **Eliminar genérico `<S>` de handlers** (15 min)
  ```bash
  # Opción A: Script automático
  ./scripts/fix_handlers.sh
  
  # Opción B: Manual
  # Editar src/handlers/{iam,playground,policies,schemas}.rs
  # Cambiar: AppState<S> → AppState
  # Eliminar: where S: SchemaStoragePort...
  ```
  
- [ ] **Verificar compilación básica** (5 min)
  ```bash
  cargo check
  # Esperado: Errores solo por campos faltantes en AppState
  ```

- [ ] **Migrar create_policy** (45 min)
  - [ ] Crear `CreatePolicyPort` en `ports.rs`
  - [ ] Implementar trait en `use_case.rs`
  - [ ] Renombrar `di.rs` → `factories.rs`
  - [ ] Factory devuelve `Arc<dyn Port>`
  - [ ] Registrar en `composition_root.rs`
  - [ ] Añadir a `AppState`
  - [ ] Tests pasan: `cargo nextest run -p hodei-iam`

---

## 🔥 FASE 2: Features Críticas de IAM (2.5 horas)

### Gestión de Políticas

- [ ] **get_policy** (30 min)
  - [ ] Puerto: `GetPolicyPort`
  - [ ] Método: `async fn get(&self, query) -> Result<PolicyView, Error>`
  - [ ] Factory: `create_get_policy_use_case(repo)`
  - [ ] En CompositionRoot: `iam_ports.get_policy`
  - [ ] En AppState: `pub get_policy: Arc<dyn GetPolicyPort>`
  - [ ] Tests: ✅

- [ ] **list_policies** (30 min)
  - [ ] Puerto: `ListPoliciesPort`
  - [ ] Método: `async fn list(&self, query) -> Result<Response, Error>`
  - [ ] Factory: `create_list_policies_use_case(repo)`
  - [ ] En CompositionRoot: `iam_ports.list_policies`
  - [ ] En AppState: `pub list_policies: Arc<dyn ListPoliciesPort>`
  - [ ] Tests: ✅

- [ ] **update_policy** (30 min)
  - [ ] Puerto: `UpdatePolicyPort`
  - [ ] Método: `async fn update(&self, command) -> Result<View, Error>`
  - [ ] Factory: `create_update_policy_use_case(validator, repo)`
  - [ ] En CompositionRoot: `iam_ports.update_policy`
  - [ ] En AppState: `pub update_policy: Arc<dyn UpdatePolicyPort>`
  - [ ] Tests: ✅

- [ ] **delete_policy** (30 min)
  - [ ] Puerto: `DeletePolicyPort`
  - [ ] Método: `async fn delete(&self, command) -> Result<(), Error>`
  - [ ] Factory: `create_delete_policy_use_case(repo)`
  - [ ] En CompositionRoot: `iam_ports.delete_policy`
  - [ ] En AppState: `pub delete_policy: Arc<dyn DeletePolicyPort>`
  - [ ] Tests: ✅

### Checkpoint 1

```bash
# Verificar que compila sin errores de campos faltantes en handlers
cargo check

# Todos los handlers de políticas deben funcionar
cargo run &
curl http://localhost:8080/api/v1/iam/policies/create -X POST -d '{...}'
```

---

## 🟡 FASE 3: Features de Usuarios (2.5 horas)

### Gestión de Usuarios y Grupos

- [ ] **create_user** (45 min)
  - [ ] Puerto: `CreateUserPort`
  - [ ] Método: `async fn create(&self, command) -> Result<UserView, Error>`
  - [ ] Factory: `create_create_user_use_case(repo, hrn_gen)`
  - [ ] En CompositionRoot: `iam_ports.create_user`
  - [ ] En AppState: `pub create_user: Arc<dyn CreateUserPort>`
  - [ ] Tests: ✅

- [ ] **create_group** (45 min)
  - [ ] Puerto: `CreateGroupPort`
  - [ ] Método: `async fn create(&self, command) -> Result<GroupView, Error>`
  - [ ] Factory: `create_create_group_use_case(repo, hrn_gen)`
  - [ ] En CompositionRoot: `iam_ports.create_group`
  - [ ] En AppState: `pub create_group: Arc<dyn CreateGroupPort>`
  - [ ] Tests: ✅

- [ ] **add_user_to_group** (30 min)
  - [ ] Puerto: `AddUserToGroupPort`
  - [ ] Método: `async fn add(&self, command) -> Result<(), Error>`
  - [ ] Factory: `create_add_user_to_group_use_case(repo)`
  - [ ] En CompositionRoot: `iam_ports.add_user_to_group`
  - [ ] En AppState: `pub add_user_to_group: Arc<dyn Port>`
  - [ ] Tests: ✅

### Evaluación de Políticas

- [ ] **evaluate_iam_policies** (45 min)
  - [ ] Puerto: `EvaluateIamPoliciesPort`
  - [ ] Método: `async fn evaluate(&self, req) -> Result<Decision, Error>`
  - [ ] Factory: `create_evaluate_iam_policies_use_case(...)`
  - [ ] En CompositionRoot: `iam_ports.evaluate_iam_policies`
  - [ ] En AppState: `pub evaluate_iam_policies: Arc<dyn Port>`
  - [ ] Tests: ✅

- [ ] **get_effective_policies** (45 min)
  - [ ] Puerto: `GetEffectivePoliciesPort`
  - [ ] Método: `async fn get(&self, query) -> Result<Vec<Policy>, Error>`
  - [ ] Factory: `create_get_effective_policies_use_case(...)`
  - [ ] En CompositionRoot: `iam_ports.get_effective_policies`
  - [ ] En AppState: `pub get_effective_policies: Arc<dyn Port>`
  - [ ] Tests: ✅

### Checkpoint 2

```bash
# Compilación completa sin errores
cargo check

# Sin warnings
cargo clippy -- -D warnings

# Todos los tests pasando
cargo nextest run
```

---

## ✅ FASE 4: Verificación Final (1 hora)

### Quality Assurance

- [ ] **Compilación limpia**
  ```bash
  cargo clean
  cargo build --release
  # Esperado: Success sin errores ni warnings
  ```

- [ ] **Linting estricto**
  ```bash
  cargo clippy -- -D warnings
  # Esperado: 0 warnings
  ```

- [ ] **Suite completa de tests**
  ```bash
  cargo nextest run --all
  # Esperado: All tests passed
  ```

- [ ] **Documentación**
  ```bash
  cargo doc --no-deps --open
  # Verificar: Todas las APIs públicas documentadas
  ```

### Checklist de Arquitectura

- [ ] ✅ hodei-policies: 7/7 features con puertos
- [ ] ✅ hodei-iam: 11/11 features con puertos
- [ ] ✅ CompositionRoot: Único lugar con tipos concretos
- [ ] ✅ AppState: Solo `Arc<dyn Port>`, sin genéricos
- [ ] ✅ Handlers: Sin genéricos `<S>`
- [ ] ✅ Bootstrap: Usa `CompositionRoot::production()`
- [ ] ✅ Tests: 224/224 pasando (179 + 45)
- [ ] ✅ Warnings: 0 con clippy `-D warnings`
- [ ] ✅ Compilación: Success en release mode

### Métricas Finales

- [ ] **Cobertura de tests:** >80% en ambos crates
- [ ] **Tiempo de compilación:** <2 minutos (release)
- [ ] **Líneas de código:** ~4,500 líneas refactorizadas
- [ ] **Features migradas:** 18/18 (100%)
- [ ] **Deuda técnica:** 0 TODOs pendientes

---

## 🎉 FASE 5: Documentación y Cleanup (30 min)

### Documentación

- [ ] **Crear ARCHITECTURE.md**
  - [ ] Overview del sistema
  - [ ] Diagrama de bounded contexts
  - [ ] Patrón de puertos explicado
  - [ ] Ejemplos de uso

- [ ] **Actualizar README.md**
  - [ ] Sección de arquitectura
  - [ ] Comandos de desarrollo
  - [ ] Guía de testing

- [ ] **Archivar documentos de migración**
  ```bash
  mkdir -p docs/migration
  mv REFACTORING_COMPLETE_SUMMARY.md docs/migration/
  mv STATUS_AND_NEXT_STEPS.md docs/migration/
  mv EXECUTIVE_SUMMARY.md docs/migration/
  mv MIGRATION_STATUS/ docs/migration/
  ```

### Cleanup

- [ ] **Eliminar archivos temporales**
  ```bash
  rm -rf .backup_handlers_*
  find . -name "*.tmp" -delete
  ```

- [ ] **Formatear código**
  ```bash
  cargo fmt --all
  ```

- [ ] **Optimizar imports**
  ```bash
  # Eliminar imports no usados
  cargo fix --allow-dirty --allow-staged
  ```

---

## 🚀 FASE 6: Git y Deploy (30 min)

### Git Workflow

- [ ] **Commit incremental por feature**
  ```bash
  # Después de cada feature
  git add crates/hodei-iam/src/features/[feature_name]
  git add src/composition_root.rs src/app_state.rs
  git commit -m "feat(iam): migrate [feature_name] to ports pattern"
  ```

- [ ] **Commit final**
  ```bash
  git add .
  git commit -m "feat: Complete architecture refactoring to Clean Architecture

  BREAKING CHANGE: Complete refactoring to ports & adapters

  - Implemented Composition Root pattern
  - Migrated 18 features to trait-based ports
  - Zero generic types in public API
  - All tests passing (224/224)
  
  Co-authored-by: AI Agent (Claude)"
  ```

- [ ] **Push y PR**
  ```bash
  git push origin feature/complete-architecture-refactoring
  # Crear PR en GitHub/GitLab
  ```

### Deploy

- [ ] **Build de producción**
  ```bash
  cargo build --release
  # Binary en: target/release/hodei-artifacts-api
  ```

- [ ] **Docker image** (si aplica)
  ```bash
  docker build -t hodei-artifacts:latest .
  docker run -p 8080:8080 hodei-artifacts:latest
  ```

---

## 📊 Progreso Global

### Resumen por Fase

```
FASE 1: Desbloquear Compilación    [████████████████████░] 95% (3.5h/3.5h)
FASE 2: Features Críticas           [░░░░░░░░░░░░░░░░░░░░]  0% (0h/2.5h)
FASE 3: Features de Usuarios        [░░░░░░░░░░░░░░░░░░░░]  0% (0h/2.5h)
FASE 4: Verificación Final          [░░░░░░░░░░░░░░░░░░░░]  0% (0h/1h)
FASE 5: Documentación               [░░░░░░░░░░░░░░░░░░░░]  0% (0h/0.5h)
FASE 6: Git y Deploy                [░░░░░░░░░░░░░░░░░░░░]  0% (0h/0.5h)
────────────────────────────────────────────────────────────────
TOTAL:                              [███████░░░░░░░░░░░░░] 38% (3.5h/10.5h)
```

### Siguiente Paso Inmediato

🎯 **AHORA:** Eliminar genérico `<S>` de handlers (15 min)

```bash
cd /home/Ruben/Proyectos/rust/hodei-artifacts
./scripts/fix_handlers.sh
cargo check
```

---

## 🆘 Si Algo Sale Mal

### Compilación falla

1. Verificar imports en composition_root.rs
2. Verificar que todos los campos están en AppState
3. Restaurar backup: `cp .backup_handlers_*/*.rs src/handlers/`

### Tests fallan

1. Ver output: `cargo nextest run -- --nocapture`
2. Ejecutar test específico: `cargo nextest run test_name`
3. Revisar mocks en `mocks.rs` de la feature

### Imports circulares

- hodei-iam NO debe importar hodei-policies
- Solo puertos (traits) se importan entre crates
- main crate orquesta todo en CompositionRoot

---

**Última Actualización:** 2024-01-XX  
**Tiempo Total Estimado:** 6-7 horas  
**Estado:** 🟡 38% Completado - Arquitectura lista, migración pendiente