# Shared Crate

Tipos, errores y utilidades compartidas para consistencia transversal en todo el workspace de Hodei Artifacts.

## Propósito

Este crate contiene:
- **Tipos de dominio comunes** (HRN, PackageCoordinates, etc.)
- **Enums compartidos** (ArtifactType, HashAlgorithm, etc.)
- **Estructuras de datos** (ContentHash, Lifecycle, etc.)
- **Utilidades de seguridad** (validación, autorización)
- **Modelos base** reutilizables entre crates

## Estructura

```
src/
  enums.rs           # Enums compartidos (ArtifactType, HashAlgorithm, etc.)
  hrn.rs             # Hodei Resource Names (identificadores únicos)
  lifecycle.rs       # Metadatos de auditoría (created_by, updated_at, etc.)
  models.rs          # Estructuras de datos comunes
  security/          # Utilidades de seguridad y autorización
    mod.rs
  lib.rs             # Re-exports públicos
```

## Tests

### Tests Unitarios

Los tests unitarios validan la lógica de construcción, validación y serialización de tipos compartidos:

```bash
# Ejecutar todos los tests unitarios del crate shared
cargo test --lib -p shared

# Ejecutar tests con logs detallados
RUST_LOG=debug cargo test --lib -p shared -- --nocapture

# Ejecutar tests de un módulo específico
cargo test -p shared hrn
cargo test -p shared models
cargo test -p shared enums
```

**Cobertura típica**:
- ✅ **Validación de HRN** - Formato correcto, parsing, construcción
- ✅ **Serialización JSON** - Serde para DTOs
- ✅ **Validación de tipos** - Enums, constraints de negocio
- ✅ **Lifecycle metadata** - Timestamps, user tracking

### Tests de Documentación

```bash
# Ejecutar doctests (ejemplos en comentarios ///)
cargo test --doc -p shared
```

## Desarrollo

### Agregar nuevos tipos compartidos

1. **Definir en el módulo apropiado** (`models.rs`, `enums.rs`, etc.)
2. **Añadir validación** si es necesario
3. **Incluir doctests** con ejemplos de uso
4. **Re-exportar** en `lib.rs` si es público
5. **Añadir tests unitarios** para casos edge

### Ejemplo de test unitario

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_coordinates_validation() {
        let coords = PackageCoordinates {
            namespace: Some("com.example".to_string()),
            name: "my-package".to_string(),
            version: "1.0.0".to_string(),
            qualifiers: Default::default(),
        };
        
        assert!(coords.is_valid());
        assert_eq!(coords.to_string(), "com.example:my-package:1.0.0");
    }
}
```

## Dependencies

- **Core**: `serde`, `time`, `uuid`, `thiserror`
- **Security**: `cedar-policy` (para ABAC)
- **Minimal external deps** para mantener el crate ligero

Ver `Cargo.toml` para versiones específicas.
