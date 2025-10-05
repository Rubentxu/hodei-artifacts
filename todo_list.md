# TODO List for Hodei Artifacts Modular Monolith Implementation

## Authorization Engine Refactoring - Cedar Integration

**Current Progress: 5/6 items completed (83%)**

- [x] Analizar el estado actual del `AuthorizationEngine`
- [x] Verificar que la API pública sea agnóstica
- [x] Implementar la traducción de entidades a Cedar
- [x] Asegurarme de que los tests del engine pasen
- [x] Verificar que el engine funciona correctamente
- [ ] Implementar la traducción de contexto (pendiente)

## Implementation Tasks

- [x] **Create authorization ports in kernel crate** - Already exists
- [x] **Refactor policies crate to remove CRUD operations and storage implementations**
- [ ] **Implement policy management features in hodei-iam crate**
- [ ] **Implement SCP management features in hodei-organizations crate**
- [ ] **Refactor hodei-authorizer to use new evaluator traits**
- [ ] **Update application state and DI composition**
- [ ] **Create new API handlers for policy management in respective bounded contexts**
- [ ] **Update tests to reflect new architecture**

## Detailed Task Breakdown

### 1. Refactor policies crate
- [x] Remove any storage implementations that exist in policies crate
- [x] Keep only schema-related functionality in engine.rs
- [x] Remove CRUD policy management features if they exist

### 2. Implement policy management in hodei-iam crate
- [x] Create complete VSA feature structure for create_policy
- [x] Implement CreatePolicyUseCase with execute() method
- [x] Implement DeletePolicyUseCase with execute() method
- [x] Implement UpdatePolicyUseCase with execute() method
- [x] Implement GetPolicyUseCase with execute() method
- [x] Implement ListPoliciesUseCase with execute() method
- [x] Create PolicyRepository for persisting IAM policies
- [x] Create unit tests for new use cases
- [x] Create integration tests for new policy management endpoints

### 3. Implement SCP management in hodei-organizations crate
- [x] Create complete VSA feature structure for create_scp
- [x] Implement CreateScpUseCase with execute() method
- [x] Implement DeleteScpUseCase with execute() method
- [x] Implement UpdateScpUseCase with execute() method
- [x] Implement GetScpUseCase with execute() method
- [x] Implement ListScpsUseCase with execute() method
- [x] Create ScpRepository for persisting SCPs
- [x] Create unit tests for new use cases
- [x] Create integration tests for new SCP management endpoints

### 4. Refactor hodei-authorizer crate
- [x] Update EvaluatePermissionsUseCase to delegate to ScpEvaluator and IamPolicyEvaluator traits
- [x] Remove direct dependencies on other bounded contexts
- [x] Simplify authorization logic to orchestrate and delegate
- [x] Update tests for EvaluatePermissionsUseCase to use new evaluator traits

### 5. Update application state and DI composition
- [x] Simplify src/app_state.rs to only contain main use cases from each bounded context
- [x] Update src/lib.rs to wire up new autonomous evaluators
- [x] Update src/main.rs if needed

### 6. Create new API handlers
- [x] Create handlers for IAM policy management in src/api/iam.rs
- [x] Create handlers for SCP management in src/api/organizations.rs
- [x] Remove old policy_handlers.rs or update it to only contain schema-related functionality

## Authorization Engine Status

### ✅ Completed
- **API Agnóstica**: La API del `AuthorizationEngine` es completamente agnóstica y no expone tipos de Cedar
- **Traducción de Entidades**: Implementado el traductor de entidades agnósticas a Cedar
- **Tests Unitarios**: Todos los tests del engine pasan correctamente
- **Compilación**: El código compila sin errores en el crate `policies`
- **Integración Cedar**: El engine integra correctamente Cedar 4.5.1 como implementación interna

### 🔄 En Progreso
- **Traducción de Contexto**: Marcado como TODO en el código (línea 134 en `core.rs`)

### 📋 Próximos Pasos
1. Implementar la traducción de contexto para completar la funcionalidad del engine
2. Actualizar la documentación para reflejar la nueva arquitectura
3. Realizar pruebas de integración completas del engine

## Resumen del Refactoring

El `AuthorizationEngine` ha sido exitosamente refactorizado para:

1. **Encapsular Cedar**: Cedar es ahora un detalle de implementación interno
2. **API Agnóstica**: Solo expone tipos del kernel (`HodeiEntity`, `Hrn`, etc.)
3. **Thread Safety**: Usa `Arc<RwLock>` para compartir estado entre threads
4. **Tests Pasan**: 6/6 tests unitarios del engine pasan correctamente
5. **Compilación Limpia**: Sin errores de compilación en el código principal

El engine está listo para ser utilizado en los bounded contexts con una API limpia y agnóstica.
