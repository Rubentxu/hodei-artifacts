# Plan Detallado de Implementación Frontend (WBS Ejecutable) - Hodei Artifacts

## 0. Objetivo
Generar un desglose accionable (work breakdown structure) directamente implementable para el frontend de Hodei Artifacts, basado en [`roadmap.md`](docs/frontend/roadmap.md) y la arquitectura descrita en [`architecture.md`](docs/frontend/architecture.md). Se prioriza desarrollo incremental y calidad desde el primer día.

## 1. Convenciones
- Formato tareas: ID (FE-EPIC-TN) + Tipo (CODE/CONFIG/TEST/DOC/OPS).
- Sección DoD específica por épica (ref cruza con criterios de aceptación del roadmap).
- Referencias a archivos existentes con líneas cuando aplica.
- Nuevos archivos sin línea inicial (se crearán durante implementación).

## 2. Vista Global de Épicas (Ruta Crítica)
1. Foundation & Tooling (E-FOUNDATION)
2. Design System & Core Components (E-DESIGN)
3. Authentication & Routing (E-AUTH)
4. Repository Management (E-REPOSITORIES)
5. Artifact Management (E-ARTIFACTS)
6. Search & Discovery (E-SEARCH)
7. User Management & Security (E-USERS)
8. Polish & Deployment (E-POLISH)

## 3. WBS por Épica

### 3.1 E-FOUNDATION (Fase 1: Foundation & Core Infrastructure)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| FE-FOUNDATION-T1 | Crear proyecto Vite + React + TypeScript en `/frontend` | CONFIG | Estructura base |
| FE-FOUNDATION-T2 | Configurar path aliases (@/, @/components, @/features) | CONFIG | `vite.config.ts`, `tsconfig.json` |
| FE-FOUNDATION-T3 | Configurar Tailwind CSS + PostCSS + theme personalizado | CONFIG | `tailwind.config.js`, archivos CSS |
| FE-FOUNDATION-T4 | Setup ESLint + Prettier + Husky pre-commit hooks | CONFIG | Configs de linting |
| FE-FOUNDATION-T5 | Configurar Vitest + React Testing Library + MSW | CONFIG | Setup de testing |
| FE-FOUNDATION-T6 | Configurar GitHub Actions para build/test/lint | OPS | `.github/workflows/frontend.yml` |
| FE-FOUNDATION-T7 | Crear estructura de carpetas feature-based | CODE | Directorios organizados |
| FE-FOUNDATION-T8 | Configurar variables de entorno (.env files) | CONFIG | Archivos de configuración |

### 3.2 E-DESIGN (Fase 1: Design System)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| FE-DESIGN-T1 | Implementar Atoms: Button, Input, Badge, Icon, Spinner | CODE | `src/components/ui/` |
| FE-DESIGN-T2 | Implementar Molecules: FormField, SearchBox, Card, Pagination | CODE | `src/components/forms/` |
| FE-DESIGN-T3 | Implementar Organisms: DataTable, Modal, Header, Sidebar | CODE | `src/components/layout/` |
| FE-DESIGN-T4 | Implementar Templates: MainLayout, AuthLayout | CODE | `src/components/templates/` |
| FE-DESIGN-T5 | Configurar Storybook + stories para todos los componentes | CONFIG | `.storybook/`, `*.stories.tsx` |
| FE-DESIGN-T6 | Implementar sistema de tokens de diseño (spacing, colors) | CODE | `tailwind.config.js` tokens |
| FE-DESIGN-T7 | Tests unitarios para componentes críticos | TEST | `*.test.tsx` archivos |
| FE-DESIGN-T8 | Documentación de componentes en Storybook | DOC | Stories con documentación |

### 3.3 E-AUTH (Fase 1: Authentication Foundation)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| FE-AUTH-T1 | Configurar React Router v6 con lazy loading | CONFIG | `src/app/router.tsx` |
| FE-AUTH-T2 | Implementar AuthStore con Zustand + persistencia | CODE | `src/shared/stores/auth.store.ts` |
| FE-AUTH-T3 | Configurar API client (Axios + interceptors) | CODE | `src/shared/api/client.ts` |
| FE-AUTH-T4 | Generar tipos TypeScript desde OpenAPI spec | CONFIG | Script + tipos generados |
| FE-AUTH-T5 | Configurar React Query + providers globales | CONFIG | `src/app/providers.tsx` |
| FE-AUTH-T6 | Implementar ProtectedRoute component | CODE | `src/shared/components/ProtectedRoute.tsx` |
| FE-AUTH-T7 | Crear LoginPage + formulario de autenticación | CODE | `src/pages/auth/LoginPage.tsx` |
| FE-AUTH-T8 | Tests de integración para auth flow | TEST | Tests de autenticación |

### 3.4 E-REPOSITORIES (Fase 2: Repository Management)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| FE-REPO-T1 | Implementar repositoryApi.ts service layer | CODE | `src/features/repositories/services/` |
| FE-REPO-T2 | Custom hooks con React Query (useRepositories, useRepository) | CODE | `src/features/repositories/hooks/` |
| FE-REPO-T3 | Dashboard principal con métricas y overview | CODE | `src/pages/dashboard/DashboardPage.tsx` |
| FE-REPO-T4 | RepositoriesPage con lista paginada + filtros | CODE | `src/pages/repositories/RepositoriesPage.tsx` |
| FE-REPO-T5 | RepositoryCard component con acciones | CODE | `src/features/repositories/components/` |
| FE-REPO-T6 | CreateRepositoryModal + formulario | CODE | `src/features/repositories/components/` |
| FE-REPO-T7 | RepositoryDetailPage con tabs | CODE | `src/pages/repositories/RepositoryDetailPage.tsx` |
| FE-REPO-T8 | Repository CRUD operations (edit, delete) | CODE | Componentes y hooks |
| FE-REPO-T9 | Search y filtering en repositories | CODE | Integración con SearchBox |
| FE-REPO-T10 | Tests E2E para repository management | TEST | Playwright tests |

### 3.5 E-ARTIFACTS (Fase 3: Artifact Management)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| FE-ARTIFACT-T1 | Implementar artifactApi.ts con upload/download | CODE | `src/features/artifacts/services/` |
| FE-ARTIFACT-T2 | Custom hooks (useArtifacts, useArtifactUpload) | CODE | `src/features/artifacts/hooks/` |
| FE-ARTIFACT-T3 | FileUpload component con drag & drop | CODE | `src/features/artifacts/components/` |
| FE-ARTIFACT-T4 | ArtifactsPage global con tabla avanzada | CODE | `src/pages/artifacts/ArtifactsPage.tsx` |
| FE-ARTIFACT-T5 | Artifacts tab en Repository Detail | CODE | Integración en repository page |
| FE-ARTIFACT-T6 | ArtifactDetailPage con metadata completa | CODE | `src/pages/artifacts/ArtifactDetailPage.tsx` |
| FE-ARTIFACT-T7 | Progress tracking para uploads | CODE | Hook useUploadProgress |
| FE-ARTIFACT-T8 | Download actions (direct + presigned URLs) | CODE | Download handlers |
| FE-ARTIFACT-T9 | Batch operations (multi-select) | CODE | Componentes de selección múltiple |
| FE-ARTIFACT-T10 | Upload validations por tipo de repositorio | CODE | Validadores específicos |
| FE-ARTIFACT-T11 | Tests de upload/download flow | TEST | Tests de integración |

### 3.6 E-SEARCH (Fase 4: Search & Discovery)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| FE-SEARCH-T1 | Implementar searchApi.ts con queries complejas | CODE | `src/features/search/services/` |
| FE-SEARCH-T2 | Custom hooks con React Query (useSearch, useSearchSuggestions) | CODE | `src/features/search/hooks/` |
| FE-SEARCH-T3 | SearchPage con filtros facetados | CODE | `src/pages/search/SearchPage.tsx` |
| FE-SEARCH-T4 | Global search en Header con autocomplete | CODE | Integración en Header component |
| FE-SEARCH-T5 | Search state management (Zustand + React Query) | CODE | `src/features/search/stores/` |
| FE-SEARCH-T6 | Advanced filters sidebar | CODE | Componentes de filtros |
| FE-SEARCH-T7 | Search results con infinite scroll | CODE | Hook useInfiniteQuery |
| FE-SEARCH-T8 | Search result highlighting | CODE | Utility para highlight |
| FE-SEARCH-T9 | Search history y favorites | CODE | Persistencia local |
| FE-SEARCH-T10 | Performance optimization (debounce, virtualization) | CODE | Optimizaciones |

### 3.7 E-USERS (Fase 5: User Management & Security)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| FE-USERS-T1 | User profile page + edit functionality | CODE | `src/pages/profile/ProfilePage.tsx` |
| FE-USERS-T2 | Token management interface | CODE | `src/features/auth/components/` |
| FE-USERS-T3 | UsersPage para administradores | CODE | `src/pages/users/UsersPage.tsx` |
| FE-USERS-T4 | Create/Edit User forms | CODE | `src/features/users/components/` |
| FE-USERS-T5 | User permissions interface (ABAC) | CODE | Componentes de políticas |
| FE-USERS-T6 | ABAC Policies management page | CODE | `src/pages/settings/PoliciesPage.tsx` |
| FE-USERS-T7 | Cedar policy editor con syntax highlighting | CODE | Editor especializado |
| FE-USERS-T8 | User activity monitoring dashboard | CODE | Componentes de auditoría |
| FE-USERS-T9 | Bulk user operations | CODE | Operaciones en lote |
| FE-USERS-T10 | Role-based UI (mostrar/ocultar según permisos) | CODE | HOCs y hooks de permisos |

### 3.8 E-POLISH (Fase 6: Polish & Deployment)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| FE-POLISH-T1 | Sistema de notificaciones global (toasts) | CODE | `src/shared/stores/notification.store.ts` |
| FE-POLISH-T2 | Loading states optimization (skeletons) | CODE | Componentes skeleton |
| FE-POLISH-T3 | Error boundaries + error pages | CODE | Error handling robusto |
| FE-POLISH-T4 | Accessibility audit + improvements | TEST | Compliance WCAG 2.1 AA |
| FE-POLISH-T5 | Performance optimization (code splitting, bundling) | OPS | Optimizaciones Vite |
| FE-POLISH-T6 | Mobile responsiveness polish | CODE | Responsive design |
| FE-POLISH-T7 | PWA features (service worker, manifest) | CONFIG | PWA setup |
| FE-POLISH-T8 | E2E test suite completo | TEST | Playwright tests comprehensivos |
| FE-POLISH-T9 | Production deployment setup | OPS | CI/CD para deployment |
| FE-POLISH-T10 | Documentation y user guides | DOC | Documentación completa |

## 4. Matriz Dependencias Clave
- **Repositories** depende de: Foundation + Design System + Auth
- **Artifacts** depende de: Repositories funcionando + Upload APIs backend
- **Search** depende de: Backend search API + artifact indexing
- **Users** depende de: IAM backend APIs + ABAC implementation
- **Polish** se aplica incrementalmente durante desarrollo

## 5. Paralelización Recomendada
- **Stream A**: Foundation → Design System → Auth (secuencial, base crítica)
- **Stream B**: Repositories (tras Stream A completo)
- **Stream C**: Artifacts (en paralelo parcial con Repositories)
- **Stream D**: Search (tras artifacts base + backend search ready)
- **Stream E**: Users/Security (tras auth sólido + backend IAM)
- **Stream F**: Polish (continuo durante todo el desarrollo)

## 6. Stack Tecnológico Detallado

### Core Framework
- **React 18**: Concurrent features, Suspense, Error Boundaries
- **TypeScript 5**: Strict mode, path mapping, generated types
- **Vite 5**: Fast builds, HMR, code splitting optimizado

### UI & Styling
- **Tailwind CSS 3**: Utility-first, custom theme, responsive
- **Headless UI**: Componentes accesibles base
- **Lucide React**: Iconografía consistente

### State Management
- **React Query 5**: Server state, cache, background sync
- **Zustand 4**: Global client state, persistencia
- **React Hook Form**: Form state, validación

### Testing
- **Vitest**: Unit testing runner
- **React Testing Library**: Component testing
- **MSW**: API mocking
- **Playwright**: E2E testing

### Build & Development
- **ESLint**: Linting con reglas React/TypeScript
- **Prettier**: Code formatting
- **Husky**: Pre-commit hooks
- **GitHub Actions**: CI/CD pipeline

## 7. Estructura de Carpetas Implementada

```
frontend/
├── public/                     # Assets estáticos
├── src/
│   ├── app/                    # Configuración global
│   │   ├── App.tsx            # Componente raíz
│   │   ├── router.tsx         # Configuración rutas
│   │   └── providers.tsx      # React Query, Auth providers
│   ├── components/            # Design System
│   │   ├── ui/               # Atoms (Button, Input, Badge)
│   │   ├── forms/            # Molecules (FormField, SearchBox)
│   │   ├── layout/           # Organisms (DataTable, Modal)
│   │   └── templates/        # Templates (MainLayout, AuthLayout)
│   ├── features/             # Organización por dominio
│   │   ├── auth/             # Autenticación
│   │   ├── repositories/     # Gestión repositorios
│   │   ├── artifacts/        # Gestión artefactos
│   │   ├── search/           # Búsqueda y filtros
│   │   └── users/            # Gestión usuarios
│   ├── pages/                # Route components
│   │   ├── dashboard/
│   │   ├── repositories/
│   │   ├── artifacts/
│   │   ├── search/
│   │   ├── users/
│   │   └── auth/
│   ├── shared/               # Código compartido
│   │   ├── api/              # Cliente HTTP
│   │   ├── hooks/            # Hooks utilitarios
│   │   ├── stores/           # Stores globales
│   │   ├── types/            # Tipos generados + comunes
│   │   └── utils/            # Utilidades puras
│   └── __tests__/            # Tests globales y setup
├── .storybook/               # Configuración Storybook
└── e2e/                      # Tests E2E Playwright
```

## 8. Métricas de Calidad por Épica

### Performance
| Métrica | Target | Responsable Epic |
|---------|---------|------------------|
| Bundle Size (inicial) | < 500KB | E-FOUNDATION, E-POLISH |
| Time to Interactive | < 3s | E-FOUNDATION, E-POLISH |
| Lighthouse Score | > 90 | E-POLISH |
| Core Web Vitals | Todos en verde | E-POLISH |

### Testing
| Métrica | Target | Responsable Epic |
|---------|---------|------------------|
| Component Coverage | > 80% | E-DESIGN |
| Feature Coverage | > 85% | E-REPOSITORIES, E-ARTIFACTS |
| E2E Coverage | Flujos críticos | E-POLISH |

### Accessibility
| Métrica | Target | Responsable Epic |
|---------|---------|------------------|
| WCAG 2.1 AA | 100% compliance | E-DESIGN, E-POLISH |
| Keyboard Navigation | Total | E-DESIGN |
| Screen Reader Support | Completo | E-DESIGN |

## 9. DoD Extendida por Épica

### E-FOUNDATION
- ✅ Proyecto Vite configurado y builds sin errores
- ✅ Tooling completo (lint, format, test) funcionando
- ✅ CI/CD pipeline verde con quality gates
- ✅ Estructura de carpetas feature-based implementada

### E-DESIGN
- ✅ Design System completo (Atoms → Templates)
- ✅ Storybook funcional con documentación
- ✅ Componentes responsive y accesibles
- ✅ Tests unitarios para componentes críticos (>80% coverage)

### E-AUTH
- ✅ Autenticación funcional con persistencia
- ✅ Rutas protegidas y redirección automática
- ✅ API client configurado con manejo de errores
- ✅ Tipos TypeScript generados desde OpenAPI

### E-REPOSITORIES
- ✅ CRUD completo de repositorios funcional
- ✅ Dashboard con métricas reales
- ✅ Búsqueda y filtros efectivos
- ✅ Estado global sincronizado (React Query + Zustand)

### E-ARTIFACTS
- ✅ Upload/download robusto con progress tracking
- ✅ Navegación intuitiva tipo file explorer
- ✅ Validaciones por tipo de repositorio
- ✅ Operaciones en lote funcionales

### E-SEARCH
- ✅ Búsqueda global rápida y precisa
- ✅ Filtros facetados y autocomplete
- ✅ Performance optimizada (infinite scroll, virtualization)
- ✅ Historial y favoritos persistentes

### E-USERS
- ✅ Gestión completa de usuarios y permisos
- ✅ Editor de políticas ABAC funcional
- ✅ Interface intuitiva para administradores
- ✅ Auditoría de actividad completa

### E-POLISH
- ✅ Performance targets alcanzados (Lighthouse >90)
- ✅ Accessibility WCAG 2.1 AA compliance
- ✅ PWA features implementadas
- ✅ Deployment a producción funcional

## 10. Riesgos y Mitigaciones

### Riesgos Técnicos
| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|-------------|---------|------------|
| Cambios API backend | Media | Alto | Tipos auto-generados + MSW mocking |
| Performance con datasets grandes | Alta | Medio | Virtualización + paginación desde inicio |
| Complejidad ABAC UI | Media | Alto | Prototipo temprano + feedback iterativo |
| Bundle size crecimiento | Alta | Medio | Code splitting + tree shaking + monitoring |

### Riesgos de Proyecto
| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|-------------|---------|------------|
| Requirements change | Media | Medio | Desarrollo iterativo + demos frecuentes |
| Recursos limitados | Alta | Alto | MVP bien definido + priorización clara |
| Integración backend | Media | Alto | Desarrollo con mocks + integración incremental |

## 11. Checklist Ejecución por Sprint

### Sprint 1 (Foundation)
1. FE-FOUNDATION-T1..T8 (Setup completo)
2. FE-DESIGN-T1..T4 (Componentes base)
3. FE-DESIGN-T5 (Storybook setup)

### Sprint 2 (Design System + Auth)
1. FE-DESIGN-T6..T8 (Design system completado)
2. FE-AUTH-T1..T4 (Routing + API setup)
3. FE-AUTH-T5..T7 (Auth implementation)

### Sprint 3 (Repositories Foundation)
1. FE-AUTH-T8 (Auth testing)
2. FE-REPO-T1..T4 (Repository base)
3. FE-REPO-T5..T6 (Repository UI)

### Sprint 4 (Repositories Complete)
1. FE-REPO-T7..T9 (Repository features)
2. FE-REPO-T10 (Repository testing)
3. FE-ARTIFACT-T1..T3 (Artifact foundation)

### Sprint 5 (Artifacts Core)
1. FE-ARTIFACT-T4..T7 (Artifact UI)
2. FE-ARTIFACT-T8..T10 (Artifact features)
3. FE-SEARCH-T1..T2 (Search foundation)

### Sprint 6 (Search Implementation)
1. FE-ARTIFACT-T11 (Artifact testing)
2. FE-SEARCH-T3..T6 (Search UI)
3. FE-SEARCH-T7..T9 (Search features)

### Sprint 7 (Search Complete + Users Start)
1. FE-SEARCH-T10 (Search optimization)
2. FE-USERS-T1..T3 (Users foundation)
3. FE-USERS-T4..T5 (User management)

### Sprint 8 (Users & Security)
1. FE-USERS-T6..T8 (ABAC UI)
2. FE-USERS-T9..T10 (User features)
3. FE-POLISH-T1..T3 (Polish start)

### Sprint 9 (Polish & Optimization)
1. FE-POLISH-T4..T6 (Accessibility + Performance)
2. FE-POLISH-T7..T8 (PWA + Testing)
3. FE-POLISH-T9 (Deployment)

### Sprint 10 (Final Polish)
1. FE-POLISH-T10 (Documentation)
2. Bug fixes y refinamiento
3. Production readiness

## 12. Indicadores de Listo para Release

### Funcionales
- ✅ Todos los flujos de usuario principales funcionan end-to-end
- ✅ Integración completa con APIs backend
- ✅ Autenticación y autorización robustas
- ✅ Upload/download de artefactos estable

### Técnicos
- ✅ Performance: Lighthouse score > 90
- ✅ Accessibility: WCAG 2.1 AA compliance
- ✅ Testing: > 80% coverage componentes críticos
- ✅ Bundle: < 500KB initial, code splitting efectivo

### Operacionales
- ✅ CI/CD pipeline funcionando
- ✅ Deployment automático configurado
- ✅ Monitoring y error tracking activo
- ✅ Documentación completa para usuarios y desarrolladores

## 13. Herramientas de Desarrollo

### Required Tools
- Node.js 18+ y npm/yarn
- VS Code con extensiones React/TypeScript
- Git para control de versiones

### Recommended Extensions
- ES7+ React/Redux/React-Native snippets
- Tailwind CSS IntelliSense
- ESLint + Prettier
- Auto Rename Tag
- Bracket Pair Colorizer

### Browser DevTools
- React Developer Tools
- Redux DevTools (para Zustand)
- Lighthouse para performance
- axe DevTools para accessibility

## 14. Resumen Ejecutivo (TL;DR)

Implementar frontend React moderno en 10 sprints:
1. **Fundación** (tooling + design system)
2. **Autenticación** (routing + auth flow)
3. **Repositorios** (CRUD + dashboard)
4. **Artefactos** (upload/download + gestión)
5. **Búsqueda** (search global + filtros)
6. **Usuarios** (gestión + ABAC UI)
7. **Optimización** (performance + accessibility)

Arquitectura: Component-Based + Atomic Design + Feature-Based Organization
Stack: React 18 + TypeScript + Vite + Tailwind + Zustand + React Query
Calidad: >80% coverage, WCAG 2.1 AA, Lighthouse >90, bundle <500KB

Fin del documento.