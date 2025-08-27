# Frontend Architecture Documentation

Este directorio contiene toda la documentación relacionada con el frontend de Hodei Artifacts, una aplicación web moderna construida con React, Tailwind CSS y Zustand.

## Índice de Documentación

- [**architecture.md**](./architecture.md) - Arquitectura del frontend, patrones y principios de diseño
- [**style-guide.json**](./style-guide.json) - Guía de estilos en formato JSON para tokens de diseño
- [**project-structure.md**](./project-structure.md) - Estructura del proyecto y organización de archivos
- [**pages-specification.md**](./pages-specification.md) - Especificación detallada de páginas y componentes
- [**roadmap.md**](./roadmap.md) - Roadmap de desarrollo y planificación por épicas
- [**component-library.md**](./component-library.md) - Biblioteca de componentes reutilizables
- [**api-integration.md**](./api-integration.md) - Integración con el backend y manejo de APIs

## Tecnologías Base

- **React 18+** - Framework principal con hooks y Concurrent Features
- **TypeScript 5+** - Tipado estático para mayor robustez
- **Tailwind CSS 3+** - Framework de utilidades CSS
- **Zustand 4+** - Gestión de estado simple y escalable
- **React Router 6+** - Navegación y ruteo
- **React Query** - Gestión de estado servidor y cache
- **React Hook Form** - Manejo de formularios
- **Headless UI** - Componentes accesibles sin estilos

## Principios de Diseño

1. **Component-Driven Development** - Desarrollo basado en componentes reutilizables
2. **Mobile-First** - Diseño responsivo que prioriza dispositivos móviles
3. **Accessibility First** - Cumplimiento WCAG 2.1 AA como estándar mínimo
4. **Design System** - Sistema de diseño consistente con tokens de diseño
5. **Performance** - Optimización de renderizado y carga de recursos
6. **Type Safety** - Tipado estricto en TypeScript para prevenir errores

## Arquitectura de Alto Nivel

```
frontend/
├── src/
│   ├── components/       # Componentes reutilizables
│   ├── pages/           # Páginas principales de la aplicación
│   ├── features/        # Funcionalidades específicas (slices)
│   ├── hooks/           # Custom hooks reutilizables
│   ├── stores/          # Stores de Zustand
│   ├── services/        # Servicios de API y utilidades
│   ├── types/           # Definiciones de tipos TypeScript
│   └── utils/           # Utilidades y helpers
├── public/              # Assets estáticos
├── docs/               # Documentación específica del frontend
└── tests/              # Tests E2E y de integración
```

## Patrones de UI Inspirados en Artifactory

Basándose en el análisis de interfaces similares como JFrog Artifactory, el frontend implementará:

- **Layout de Dashboard** con sidebar navegacional y área principal de contenido
- **Navegación por pestañas** para diferentes vistas de repositorios
- **Tablas de datos avanzadas** con filtrado, ordenamiento y paginación
- **Búsqueda global** con filtros avanzados y autocompletado
- **Modales y drawers** para formularios y detalles
- **Breadcrumbs** para navegación jerárquica en repositorios
- **Cards de información** para métricas y resúmenes
- **Listados de artefactos** con vista de árbol y metadatos

## Filosofía de Desarrollo

- **Atomic Design** - Organización de componentes en átomos, moléculas y organismos
- **Feature-Sliced Design** - Organización por funcionalidades de negocio
- **Convention over Configuration** - Convenciones claras para reducir decisiones repetitivas
- **Progressive Enhancement** - Funcionalidad básica que se mejora progresivamente
- **Error Boundaries** - Manejo robusto de errores en la UI
- **Loading States** - Estados de carga claros y consistentes