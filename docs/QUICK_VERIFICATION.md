# Guía de Verificación Rápida - Refactorización hodei-iam

**Propósito:** Verificar que la refactorización de encapsulamiento se completó correctamente  
**Tiempo estimado:** 5-10 minutos  
**Última actualización:** 2024-01-XX

---

## ✅ Verificación Rápida (2 minutos)

### 1. Compilación del Crate

```bash
cd /home/Ruben/Proyectos/rust/hodei-artifacts
cargo check -p hodei-iam
```

**Resultado esperado:**
```
✅ Finished `dev` profile [unoptimized + debuginfo] target(s) in ~2s
⚠️  10 warnings (menores, no críticos)
```

**Si falla:** Revisar errores de compilación y consultar `REFACTOR_PROGRESS.md`

---

### 2. Verificar Encapsulamiento

```bash
# Verificar que 'internal' NO es público
grep -n "^pub mod internal" crates/hodei-iam/src/lib.rs
```

**Resultado esperado:**
```
(vacío - no debe haber coincidencias)
```

**Si encuentra coincidencias:** El módulo interno está expuesto - FALLO DE ENCAPSULAMIENTO

---

### 3. Verificar API Pública

```bash
# Ver exportaciones públicas
grep "^pub use features::" crates/hodei-iam/src/lib.rs | head -10
```

**Resultado esperado:**
```
pub use features::create_user::{CreateUserUseCase, CreateUserCommand};
pub use features::add_user_to_group::{AddUserToGroupUseCase, AddUserToGroupCommand};
pub use features::create_group::{CreateGroupCommand, CreateGroupUseCase};
...
```

**Verificar:** Solo casos de uso y DTOs exportados, NO infraestructura

---

## 📋 Verificación Completa (10 minutos)

### 1. Estructura de Directorios

```bash
# Verificar que 'shared' fue renombrado a 'internal'
ls -la crates/hodei-iam/src/ | grep -E "(shared|internal)"
```

**Resultado esperado:**
```
drwxrwxr-x 5 user user 4096 date time internal
```

**NO debe aparecer:** directorio `shared`

---

### 2. Verificar Módulo Interno es Privado

```bash
# Buscar declaración del módulo internal
head -100 crates/hodei-iam/src/lib.rs | grep -A 5 "mod internal"
```

**Resultado esperado:**
```rust
// INTERNAL MODULE - NOT PUBLIC
mod internal;
```

**NO debe aparecer:** `pub mod internal`

---

### 3. Verificar Deprecaciones

```bash
# Verificar que exports problemáticos están deprecados
grep -A 3 "#\[deprecated" crates/hodei-iam/src/lib.rs
```

**Resultado esperado:**
```rust
#[deprecated(
    since = "0.1.0",
    note = "Direct infrastructure access violates encapsulation..."
)]
pub mod infrastructure { ... }

#[deprecated(
    since = "0.1.0",
    note = "Direct port access violates encapsulation..."
)]
pub mod ports { ... }
```

---

### 4. Verificar Features Exportadas

```bash
# Listar todas las features públicas
grep "pub use features::" crates/hodei-iam/src/lib.rs
```

**Resultado esperado:** Lista de casos de uso exportados:
- CreateUserUseCase
- AddUserToGroupUseCase
- CreateGroupUseCase
- GetEffectivePoliciesForPrincipalUseCase
- EvaluateIamPoliciesUseCase

**NO debe aparecer:** `create_policy` (temporalmente deshabilitada)

---

### 5. Verificar Documentación

```bash
# Ver documentación del módulo
head -70 crates/hodei-iam/src/lib.rs
```

**Debe contener:**
- Descripción del bounded context
- Principios arquitectónicos
- Lista de features disponibles
- Ejemplos de uso
- Advertencias sobre API interna

---

### 6. Verificar Tests Inline

```bash
# Ejecutar tests inline en las features
cargo test -p hodei-iam --lib 2>&1 | grep -E "(test result|passed)"
```

**Resultado esperado:**
```
test result: ok. X passed; 0 failed; Y ignored
```

**Si falla:** Ver detalles en `REFACTOR_PROGRESS.md` - algunos tests comentados temporalmente

---

## 🔍 Verificación de Calidad de Código

### 1. Clippy (Warnings)

```bash
cargo clippy -p hodei-iam 2>&1 | grep -E "(warning|error)" | head -20
```

**Resultado esperado:**
```
⚠️  ~10 warnings menores (imports no usados, structs no construidos)
❌ 0 errores
```

**Aceptable:** Warnings menores que serán limpiados en Phase 2

---

### 2. Formato de Código

```bash
cargo fmt -p hodei-iam -- --check
```

**Resultado esperado:**
```
(sin salida = código formateado correctamente)
```

**Si falla:** Ejecutar `cargo fmt -p hodei-iam` para formatear

---

### 3. Referencias a Módulo Interno

```bash
# Verificar que NO hay referencias a 'crate::shared'
grep -r "crate::shared" crates/hodei-iam/src/
```

**Resultado esperado:**
```
(vacío - no debe haber coincidencias)
```

**Si encuentra coincidencias:** Referencias antiguas no actualizadas - ERROR

---

### 4. Referencias Correctas a Módulo Interno

```bash
# Verificar referencias correctas a 'crate::internal'
grep -r "crate::internal" crates/hodei-iam/src/ | wc -l
```

**Resultado esperado:**
```
~40-50 referencias
```

---

## 🎯 Checklist de Verificación Final

Marcar con ✅ cuando se verifique:

### Encapsulamiento
- [ ] Módulo `internal` es privado (no `pub mod`)
- [ ] No hay exportaciones directas de `internal`
- [ ] Infraestructura y ports deprecados
- [ ] Solo casos de uso en API pública

### Estructura
- [ ] Directorio `shared` renombrado a `internal`
- [ ] 0 referencias a `crate::shared`
- [ ] ~45 referencias a `crate::internal`
- [ ] Features organizadas en `features/`

### Código
- [ ] Compila sin errores (`cargo check`)
- [ ] Clippy con warnings menores aceptables
- [ ] Código formateado correctamente
- [ ] Documentación completa en `lib.rs`

### Tests (parcial)
- [ ] Tests inline pasan (mayoría)
- [ ] Tests de integración requieren actualización (documentado)
- [ ] Estrategia de testing documentada

---

## 🚨 Indicadores de Problema

### ❌ CRÍTICO - Revisar Inmediatamente

1. **Módulo interno público**
   ```bash
   grep "^pub mod internal" crates/hodei-iam/src/lib.rs
   ```
   Si encuentra coincidencias → FALLO DE ENCAPSULAMIENTO

2. **Errores de compilación**
   ```bash
   cargo check -p hodei-iam
   ```
   Si falla → REGRESIÓN INTRODUCIDA

3. **Infraestructura expuesta sin deprecación**
   ```bash
   grep "^pub mod infrastructure" crates/hodei-iam/src/lib.rs
   ```
   Si NO tiene `#[deprecated]` → VIOLACIÓN DE ENCAPSULAMIENTO

---

## 📊 Métricas de Referencia

| Métrica | Valor Objetivo | Valor Actual | Estado |
|---------|----------------|--------------|--------|
| Compilación | ✅ Exitosa | ✅ Exitosa | 🟢 |
| Warnings Clippy | < 15 | ~10 | 🟢 |
| Errores Compilación | 0 | 0 | 🟢 |
| Módulos Públicos Innecesarios | 0 | 0 | 🟢 |
| Deprecaciones Activas | 2 | 2 | 🟢 |
| Tests Passing | 100% | ~70% | 🟡 |

---

## 🔧 Solución Rápida de Problemas

### Problema: "No compila"

```bash
# Limpiar y recompilar
cargo clean
cargo build -p hodei-iam
```

### Problema: "Demasiados warnings"

```bash
# Aplicar fixes automáticos de clippy
cargo clippy -p hodei-iam --fix --allow-dirty
```

### Problema: "Tests fallan"

**Causa conocida:** Tests antiguos usan APIs deprecadas  
**Solución:** Ver `REFACTOR_PROGRESS.md` sección "Limitaciones"  
**Estado:** Actualización planificada para Phase 2

---

## 📞 Contacto y Soporte

**Si encuentra problemas:**
1. Revisar `docs/REFACTOR_PROGRESS.md`
2. Revisar `docs/REFACTOR_SESSION_SUMMARY.md`
3. Consultar `docs/ARCHITECTURAL_REFACTOR_PLAN.md`

**Para reportar issues:**
- Incluir output de comandos de verificación
- Especificar qué checklist falló
- Adjuntar logs de compilación si aplica

---

## ✅ Confirmación Final

**Una vez completadas todas las verificaciones:**

```bash
echo "✅ Verificación completada - hodei-iam refactorizado correctamente"
```

**Si todo está en verde (🟢):**
La refactorización de encapsulamiento es exitosa y el crate está listo para continuar con Phase 2.

---

**Última verificación:** [FECHA]  
**Verificado por:** [NOMBRE]  
**Estado:** [ ] EXITOSO  [ ] REQUIERE REVISIÓN  [ ] FALLO

---

*Documento de verificación generado como parte de la refactorización arquitectónica de Hodei Artifacts.*