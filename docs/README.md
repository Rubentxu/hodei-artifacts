# Documentación de Hodei Artifacts

**Estado:** ✅ Actualizado  
**Versión:** 1.0  
**Última actualización:** 2024

---

## 📚 Índice de Documentación

### 🔒 Documentos Obligatorios

1. **[ARCHITECTURE_RULES.md](./ARCHITECTURE_RULES.md)** 🔴 LECTURA OBLIGATORIA
   - Reglas de oro de arquitectura
   - Checklist de validación
   - Patrones correctos e incorrectos
   - Señales de alerta

### 📖 Documentación de Arquitectura

2. **[architecture-final-correct.md](./architecture-final-correct.md)**
   - Arquitectura completa del sistema
   - Responsabilidades de cada crate
   - Flujo de comunicación entre crates
   - Principios aplicados

3. **[encapsulation-boundaries.md](./encapsulation-boundaries.md)**
   - Guía detallada de encapsulación
   - Qué exponer y qué NO exponer por crate
   - Patrones correctos vs antipatrones
   - Validación de encapsulación

4. **[architecture-integration-plan.md](./architecture-integration-plan.md)**
   - Plan de integración original
   - Análisis del problema
   - Fases de refactorización
   - Criterios de éxito

### 🔄 Documentación de Refactorización

5. **[single-responsibility-refactoring.md](./single-responsibility-refactoring.md)**
   - Plan detallado de refactorización
   - Principio de responsabilidad única
   - Cambios implementados por crate
   - Comparación antes/después

6. **[refactoring-complete-summary.md](./refactoring-complete-summary.md)**
   - Resumen ejecutivo de cambios
   - Estado final de cada crate
   - Validación de cumplimiento
   - Beneficios obtenidos

7. **[refactoring-phase2-completed.md](./refactoring-phase2-completed.md)**
   - Fase 2 completada (histórico)
   - Eliminación de reimplementación Cedar
   - Delegación correcta a policies

---

## 🎯 Guía Rápida

### Para Desarrolladores Nuevos

**Empieza por aquí (en orden):**

1. 📖 Lee [ARCHITECTURE_RULES.md](./ARCHITECTURE_RULES.md) - **OBLIGATORIO**
2. 📖 Lee [architecture-final-correct.md](./architecture-final-correct.md)
3. 📖 Lee [encapsulation-boundaries.md](./encapsulation-boundaries.md)
4. ✅ Verifica que entiendes las reglas con el checklist

### Para Revisar Código

**Checklist de revisión:**

1. ✅ Verifica que solo se exportan casos de uso (features)
2. ✅ Confirma que entidades son `pub(crate)` (internas)
3. ✅ Valida que NO hay imports de entidades de otros crates
4. ✅ Verifica que casos de uso devuelven DTOs, NO entidades
5. ✅ Confirma que compilación pasa sin errores ni warnings

**Referencia:** [ARCHITECTURE_RULES.md - Checklist](./ARCHITECTURE_RULES.md#-checklist-de-validación)

### Para Agregar Nueva Feature

**Sigue este patrón:**

```rust
// 1. Estructura de directorios (VSA)
crate/src/features/new_feature/
├── mod.rs           # Exports públicos
├── use_case.rs      # Lógica de negocio
├── dto.rs           # Commands, Queries, Responses
├── error.rs         # Errores específicos
├── ports.rs         # Traits (interfaces)
├── adapter.rs       # Implementaciones
└── use_case_test.rs # Tests unitarios

// 2. Exportar desde lib.rs
pub use features::new_feature::{
    NewFeatureUseCase,
    NewFeatureCommand,
    NewFeatureResponse,
};
```

**Referencia:** [ARCHITECTURE_RULES.md - Patrón 1](./ARCHITECTURE_RULES.md#patrón-1-crear-nueva-feature)

---

## 🏗️ Arquitectura del Sistema

### Principio Fundamental

> **"Cada crate expone SOLO casos de uso (features) con Commands/Queries/DTOs.  
> Las entidades de dominio, repositorios y servicios son INTERNOS y NUNCA se exponen."**

### Crates del Sistema

| Crate | Responsabilidad | Expone | NO Expone |
|-------|-----------------|--------|-----------|
| **policies** | Motor de políticas Cedar | `AuthorizationEngine`, casos de uso CRUD | `PolicyStorage`, `SurrealMemStorage` |
| **hodei-iam** | Gestión de identidades | Casos de uso (create_user, get_policies, etc.) | `User`, `Group`, `Policy` |
| **hodei-organizations** | Estructura organizacional | Casos de uso (create_ou, get_scps, etc.) | `Account`, `OU`, `ServiceControlPolicy` |
| **hodei-authorizer** | Orquestador de autorización | `EvaluatePermissionsUseCase` | Nada interno (solo orquesta) |

### Flujo de Comunicación

```
Application Layer (main.rs)
         │
         ├─► hodei-iam (casos de uso)
         ├─► hodei-organizations (casos de uso)
         ├─► hodei-authorizer (orquesta)
         └─► policies (motor)
```

**Referencia:** [architecture-final-correct.md](./architecture-final-correct.md)

---

## ✅ Reglas de Oro (Resumen)

### ✅ HACER

1. Exportar SOLO casos de uso con DTOs
2. Marcar entidades como `pub(crate)` (internas)
3. Comunicarse entre crates vía casos de uso
4. Devolver DTOs desde casos de uso
5. Inyectar casos de uso, NO repositorios

### ❌ NO HACER

1. Exportar entidades de dominio
2. Importar entidades de otros crates
3. Devolver entidades desde casos de uso
4. Crear providers innecesarios
5. Exponer detalles de implementación

**Referencia completa:** [ARCHITECTURE_RULES.md](./ARCHITECTURE_RULES.md)

---

## 🔍 Validación

### Comandos de Verificación

```bash
# 1. Verificar que NO hay imports incorrectos
grep -r "use hodei_iam::.*domain::" hodei-authorizer/src/
# Resultado esperado: 0 matches ✅

# 2. Verificar que NO se expone PolicyStorage
grep -r "PolicyStorage" --include="*.rs" --exclude-dir=policies
# Resultado esperado: 0 matches en producción ✅

# 3. Compilación limpia
cargo check --workspace
cargo clippy --workspace --all-targets
# Resultado esperado: Sin errores ni warnings ✅
```

---

## 📖 Documentos por Tema

### Arquitectura General
- [architecture-final-correct.md](./architecture-final-correct.md)
- [architecture-integration-plan.md](./architecture-integration-plan.md)

### Encapsulación
- [encapsulation-boundaries.md](./encapsulation-boundaries.md)
- [ARCHITECTURE_RULES.md](./ARCHITECTURE_RULES.md)

### Refactorización (Histórico)
- [single-responsibility-refactoring.md](./single-responsibility-refactoring.md)
- [refactoring-complete-summary.md](./refactoring-complete-summary.md)
- [refactoring-phase2-completed.md](./refactoring-phase2-completed.md)

---

## 🆘 Preguntas Frecuentes

### ¿Puedo exportar una entidad de dominio?

**NO.** Las entidades deben ser `pub(crate)`. Crea DTOs en su lugar.

**Ver:** [ARCHITECTURE_RULES.md - Regla 2](./ARCHITECTURE_RULES.md#regla-2-entidades-de-dominio-son-internas)

### ¿Cómo comunico dos crates?

A través de **casos de uso**. Un crate exporta casos de uso, el otro los importa y usa.

**Ver:** [ARCHITECTURE_RULES.md - Regla 4](./ARCHITECTURE_RULES.md#regla-4-comunicación-entre-crates-solo-vía-casos-de-uso)

### ¿Puedo crear un provider que wrappea un caso de uso?

**NO.** Usa el caso de uso directamente. Los providers custom son innecesarios.

**Ver:** [ARCHITECTURE_RULES.md - Regla 5](./ARCHITECTURE_RULES.md#regla-5-no-crear-providerswrappers-innecesarios)

### ¿Dónde construyo las dependencias?

En el **Application Layer** (main.rs), no en los crates.

**Ver:** [ARCHITECTURE_RULES.md - Regla 7](./ARCHITECTURE_RULES.md#regla-7-construcción-de-dependencias-en-application-layer)

---

## 🚀 Estado del Proyecto

| Aspecto | Estado | Validación |
|---------|--------|------------|
| Arquitectura | ✅ Correcta | Sin violaciones |
| Encapsulación | ✅ Perfecta | Entidades internas |
| Compilación | ✅ Limpia | Sin errores |
| Warnings | ✅ Cero | Clippy limpio |
| Tests | ✅ Pasando | CI verde |
| Documentación | ✅ Completa | 7 documentos |

---

## 📞 Contacto

Para preguntas sobre arquitectura:
- Lee primero [ARCHITECTURE_RULES.md](./ARCHITECTURE_RULES.md)
- Consulta [encapsulation-boundaries.md](./encapsulation-boundaries.md)
- Revisa ejemplos en [architecture-final-correct.md](./architecture-final-correct.md)

---

**Última actualización:** 2024  
**Versión de documentación:** 1.0  
**Estado:** ✅ Completo y validado