# Tests Deshabilitados Temporalmente

Estos tests de integración están deshabilitados temporalmente porque requieren refactorización para alinearse con la nueva arquitectura de bounded contexts.

## Tests en esta carpeta:

- `governance_auth_e2e_test.rs` - Test E2E de autorización con governance
- `governance_authorization_flow_test.rs` - Test de flujo de autorización con organizaciones

## Problemas a resolver:

1. **Imports incorrectos**: Los tests usan módulos que ya no existen o han cambiado de ubicación
2. **API changes**: Algunos tipos y métodos han cambiado su firma (ej: `Hrn::new` ahora requiere 5 parámetros)
3. **Exports faltantes**: Necesitan re-exportaciones de tipos internos
4. **SurrealDB types**: Conflictos entre tipos `Surreal<Any>` y `Surreal<Db>`

## Plan de acción:

1. Refactorizar los tests para usar la nueva API de bounded contexts
2. Actualizar las construcciones de HRN con los nuevos parámetros
3. Asegurar que todos los tipos exportados necesarios estén disponibles
4. Resolver conflictos de tipos de SurrealDB

## Para re-habilitar:

Cuando los tests estén actualizados, moverlos de vuelta a `tests/`:
```bash
mv tests/.disabled/*.rs tests/
```
