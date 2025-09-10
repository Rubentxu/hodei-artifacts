# Búsqueda Básica

Implementación del motor de búsqueda básico para Hodei Artifacts.

## Descripción

Esta carpeta contiene la implementación del motor de búsqueda básico que permite a los usuarios buscar artefactos por nombre y versión. La implementación sigue los principios de Arquitectura Vertical Slice (VSA) y Clean Architecture con segregación de interfaces.

## Características

- Búsqueda por nombre de artefacto
- Búsqueda por versión de artefacto
- Búsqueda case-insensitive
- Paginación de resultados
- Manejo de búsquedas vacías (devuelve todos los artefactos)
- Publicación de eventos de búsqueda

## Arquitectura

La implementación sigue una arquitectura hexagonal con los siguientes componentes:

- **Puertos (Ports)**: Interfaces definidas en `ports.rs`
- **Caso de Uso (Use Case)**: Lógica de negocio en `use_case.rs`
- **Adaptadores (Adapters)**: Implementaciones concretas en `adapter.rs`, `event_adapter.rs`, etc.
- **Inyección de Dependencias**: Configuración en `di.rs`

## Pruebas

Las pruebas se encuentran en `basic_search_test.rs` y cubren los casos de uso principales:

- Búsqueda con resultados
- Búsqueda vacía
- Búsqueda case-insensitive

Para ejecutar las pruebas:

```bash
cargo test -p search
```

## Documentación

Para más detalles sobre la implementación, consulta [IMPLEMENTATION_SUMMARY.md](IMPLEMENTATION_SUMMARY.md).