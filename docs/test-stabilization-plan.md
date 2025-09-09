# Plan de Estabilización de Tests - IAM Crate

## Estado Actual
✅ Código principal compila correctamente
✅ PolicyId::new() implementado
✅ Ambigüedad en cedar_validator resuelta

## Problemas Pendientes por Feature

### 1. list_policies
**Problemas:**
- MockPolicyListerImpl no existe
- PolicyFilter campos faltantes (created_by, limit, name_contains, etc.)
- PolicyList campos faltantes (has_more, total_count)

**Solución:**
- Crear mocks usando mockall
- Usar PolicyFilter::new() y métodos builder
- Usar PolicyList::new() y métodos builder

### 2. delete_policy  
**Problemas:**
- MockPolicyDeleterImpl no existe
- MockPolicyDeleteEventPublisherImpl no existe
- PolicyId::new() ✅ (ya resuelto)

**Solución:**
- Crear mocks usando mockall para las interfaces segregadas

### 3. update_policy
**Problemas:**
- MockPolicyUpdaterImpl no existe
- MockPolicyUpdateValidatorImpl no existe  
- MockPolicyUpdateEventPublisherImpl no existe
- ValidationResult::invalid() espera ValidationError, no String
- IamError::ValidationError no existe
- IamError::EventPublishError no existe

**Solución:**
- Crear mocks usando mockall
- Usar ValidationError en lugar de String
- Usar IamError variants correctos

### 4. Infrastructure Tests
**Problemas:**
- cedar_validator tests con ambigüedad ✅ (ya resuelto)
- ValidationResult tests necesitan ValidationError

## Plan de Ejecución

### Fase 1: Arreglar ValidationError y IamError
1. Revisar IamError variants disponibles
2. Actualizar tests para usar tipos correctos

### Fase 2: Arreglar list_policies tests
1. Crear mocks con mockall
2. Usar builders para PolicyFilter y PolicyList

### Fase 3: Arreglar delete_policy tests  
1. Crear mocks con mockall
2. Actualizar PolicyId::new() calls

### Fase 4: Arreglar update_policy tests
1. Crear mocks con mockall
2. Arreglar ValidationError types
3. Arreglar IamError variants

### Fase 5: Verificación Final
1. Ejecutar todos los tests
2. Verificar cobertura
3. Documentar resultados