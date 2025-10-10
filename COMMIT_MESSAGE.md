refactor(iam): estandarizar manejo de errores en puertos de get_effective_policies

## Cambios Realizados

### Problema Resuelto
- Eliminada inconsistencia en manejo de errores entre puertos
- Reemplazado `Box<dyn std::error::Error + Send + Sync>` por enums específicos
- Establecido patrón consistente siguiendo ejemplo de `delete_policy`

### Archivos Modificados
- **Puertos**: `crates/hodei-iam/src/features/get_effective_policies/ports.rs`
- **Mocks**: `crates/hodei-iam/src/features/get_effective_policies/mocks.rs`
- **Adaptadores**:
  - `crates/hodei-iam/src/infrastructure/surreal/user_adapter.rs`
  - `crates/hodei-iam/src/infrastructure/surreal/group_adapter.rs`
  - `crates/hodei-iam/src/infrastructure/surreal/policy_adapter.rs`

### Beneficios
- ✅ Seguridad de tipos mejorada
- ✅ Contratos explícitos en puertos
- ✅ Testing más robusto
- ✅ Mantenibilidad mejorada

### Documentación
- Creado `docs/arquitectura/REFACTORIZACION_ERRORES_PUERTOS.md`
- Establecido patrón estándar para nuevas features
- Checklist de implementación incluido

### Breaking Changes
- Ninguno - cambios internos que mantienen compatibilidad de API
