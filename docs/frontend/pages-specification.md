# Pages Specification

## Especificación Detallada de Páginas

Esta especificación define todas las páginas de la aplicación web de Hodei Artifacts, siguiendo patrones de UI inspirados en soluciones como JFrog Artifactory y adaptados a las capacidades específicas de nuestro backend.

## Mapa de Sitio

```
/                           # Dashboard principal
├── /repositories           # Gestión de repositorios
│   ├── /new               # Crear nuevo repositorio
│   └── /:id               # Detalle de repositorio
│       ├── /artifacts     # Listado de artefactos del repositorio
│       ├── /settings      # Configuración del repositorio
│       └── /permissions   # Permisos del repositorio
├── /artifacts             # Exploración global de artefactos
│   ├── /upload           # Subida de artefactos
│   └── /:id              # Detalle de artefacto
├── /search                # Búsqueda avanzada
├── /users                 # Gestión de usuarios (admin)
│   ├── /new              # Crear nuevo usuario
│   └── /:id              # Detalle/edición de usuario
├── /settings              # Configuración global
│   ├── /policies         # Gestión de políticas ABAC
│   ├── /tokens           # Gestión de tokens API
│   └── /system           # Configuración del sistema
└── /auth
    ├── /login            # Página de inicio de sesión
    └── /logout           # Cierre de sesión
```

## 1. Dashboard Principal (`/`)

### Objetivo
Página de inicio que proporciona una vista general del estado del sistema y acceso rápido a las funcionalidades principales.

### Componentes Principales
- **Header Global** con navegación y usuario
- **Sidebar** con menú principal
- **Métricas Dashboard** con cards de información
- **Lista de Repositorios Recientes**
- **Actividad Reciente** (uploads, downloads)

### Layout
```
┌─────────────────────────────────────────────────────────┐
│ Header (Logo, Search, User Menu)                       │
├─────────┬───────────────────────────────────────────────┤
│ Sidebar │ Dashboard Content                             │
│         │ ┌─────────┬─────────┬─────────┬─────────┐     │
│ • Home  │ │Total    │Repos    │Users    │Storage  │     │
│ • Repos │ │Packages │Active   │Online   │Used     │     │
│ • Search│ └─────────┴─────────┴─────────┴─────────┘     │
│ • Users │                                               │
│ • Sets  │ Recent Repositories                           │
│         │ ┌─────────────────────────────────────────────┐ │
│         │ │ 📦 maven-central    Updated 2h ago        │ │
│         │ │ 📦 npm-private      Updated 5h ago        │ │
│         │ │ 📦 pypi-local       Updated 1d ago        │ │
│         │ └─────────────────────────────────────────────┘ │
│         │                                               │
│         │ Recent Activity                               │
│         │ ┌─────────────────────────────────────────────┐ │
│         │ │ ⬆️ user@org uploaded package.jar           │ │
│         │ │ ⬇️ user@org downloaded react@18.0.0        │ │
│         │ │ 👤 admin created user 'developer'          │ │
│         │ └─────────────────────────────────────────────┘ │
└─────────┴───────────────────────────────────────────────┘
```

### Funcionalidades
- Métricas en tiempo real (total packages, storage usado, usuarios activos)
- Lista de repositorios con acceso rápido
- Feed de actividad reciente
- Búsqueda global desde header
- Navegación rápida a funcionalidades principales

### Datos Requeridos
```typescript
interface DashboardData {
  metrics: {
    totalPackages: number
    activeRepositories: number
    onlineUsers: number
    storageUsed: { value: number; unit: string }
  }
  recentRepositories: Repository[]
  recentActivity: ActivityEvent[]
}
```

## 2. Gestión de Repositorios (`/repositories`)

### 2.1 Lista de Repositorios (`/repositories`)

### Objetivo
Vista principal para gestionar todos los repositorios del sistema con capacidades de filtrado, búsqueda y administración.

### Layout
```
┌─────────────────────────────────────────────────────────┐
│ Header (Breadcrumb: Home > Repositories)               │
├─────────┬───────────────────────────────────────────────┤
│ Sidebar │ ┌─────────────────────────────────────────────┐ │
│         │ │ Repositories                    [+ New Repo]│ │
│         │ ├─────────────────────────────────────────────┤ │
│         │ │ 🔍 Search repositories...   [Filter ▼]     │ │
│         │ ├─────────────────────────────────────────────┤ │
│         │ │ Type: [All ▼] Format: [All ▼] Status: [All]│ │
│         │ └─────────────────────────────────────────────┘ │
│         │                                               │
│         │ ┌─────────────────────────────────────────────┐ │
│         │ │ 📦 maven-central (Public)         [⋯ Menu] │ │
│         │ │    Maven repository for OSS packages       │ │
│         │ │    📊 1,234 packages • 5.2 GB • Updated 2h │ │
│         │ │                                             │ │
│         │ │ 📦 npm-private (Private)          [⋯ Menu] │ │
│         │ │    Internal npm packages                   │ │
│         │ │    📊 456 packages • 890 MB • Updated 5h   │ │
│         │ │                                             │ │
│         │ │ 📦 pypi-local (Local)             [⋯ Menu] │ │
│         │ │    Python packages mirror                  │ │
│         │ │    📊 2,345 packages • 12 GB • Updated 1d  │ │
│         │ └─────────────────────────────────────────────┘ │
│         │                                               │
│         │ [Previous] [1] [2] [3] [Next]                │
└─────────┴───────────────────────────────────────────────┘
```

### Funcionalidades
- Lista paginada de repositorios con tarjetas informativas
- Filtros por tipo (Maven, npm, PyPI), formato, y estado
- Búsqueda en tiempo real por nombre y descripción
- Acciones rápidas: ver, editar, eliminar repositorio
- Creación de nuevo repositorio
- Métricas por repositorio (número de packages, tamaño, última actualización)

### 2.2 Detalle de Repositorio (`/repositories/:id`)

### Objetivo
Vista detallada de un repositorio específico con pestañas para diferentes aspectos de gestión.

### Layout
```
┌─────────────────────────────────────────────────────────┐
│ Header (Breadcrumb: Repositories > maven-central)      │
├─────────┬───────────────────────────────────────────────┤
│ Sidebar │ ┌─────────────────────────────────────────────┐ │
│         │ │ 📦 maven-central                           │ │
│         │ │    Maven repository for OSS packages       │ │
│         │ │    🔗 https://repo.hodei.dev/maven-central  │ │
│         │ │    📊 1,234 packages • 5.2 GB             │ │
│         │ │    [🔧 Settings] [🔐 Permissions] [📋 Copy URL] │
│         │ └─────────────────────────────────────────────┘ │
│         │                                               │
│         │ [Artifacts] [Settings] [Permissions] [Activity]│
│         │                                               │
│         │ ┌─────────────────────────────────────────────┐ │
│         │ │ 🔍 Search artifacts...         [Upload ⬆️] │ │
│         │ ├─────────────────────────────────────────────┤ │
│         │ │ 📁 com/                                     │ │
│         │ │   📁 example/                               │ │
│         │ │     📁 myapp/                               │ │
│         │ │       📁 1.0.0/                             │ │
│         │ │         📄 myapp-1.0.0.jar     2.3 MB      │ │
│         │ │         📄 myapp-1.0.0.pom     1.2 KB      │ │
│         │ │         📄 myapp-1.0.0.jar.md5 32 B        │ │
│         │ │       📁 1.1.0/                             │ │
│         │ │         📄 myapp-1.1.0.jar     2.4 MB      │ │
│         │ └─────────────────────────────────────────────┘ │
└─────────┴───────────────────────────────────────────────┘
```

### Funcionalidades
- **Tab Artifacts**: Navegación tipo árbol de artefactos
- **Tab Settings**: Configuración del repositorio
- **Tab Permissions**: Gestión de permisos ABAC
- **Tab Activity**: Log de actividad del repositorio
- Subida de artefactos mediante drag & drop o formulario
- Descarga de artefactos individuales
- Información detallada de metadatos

## 3. Gestión de Artefactos (`/artifacts`)

### 3.1 Exploración Global (`/artifacts`)

### Objetivo
Vista global de todos los artefactos del sistema con capacidades avanzadas de filtrado y búsqueda.

### Layout
```
┌─────────────────────────────────────────────────────────┐
│ Header (Breadcrumb: Home > Artifacts)                  │
├─────────┬───────────────────────────────────────────────┤
│ Sidebar │ ┌─────────────────────────────────────────────┐ │
│         │ │ All Artifacts                   [Upload ⬆️] │ │
│         │ ├─────────────────────────────────────────────┤ │
│         │ │ 🔍 Search artifacts...                      │ │
│         │ ├─────────────────────────────────────────────┤ │
│         │ │ Filters:                                    │ │
│         │ │ Type: [All ▼] Repository: [All ▼]          │ │
│         │ │ Size: [Any ▼] Date: [Any ▼]                │ │
│         │ └─────────────────────────────────────────────┘ │
│         │                                               │
│         │ ┌─────────────────────────────────────────────┐ │
│         │ │ Name ↕️ │ Type │ Repository │ Size │ Modified │ │
│         │ ├─────────┼──────┼────────────┼──────┼─────────┤ │
│         │ │📦 react │ npm  │npm-private │2.3MB │2h ago   │ │
│         │ │📦 junit │maven │maven-centr │1.2MB │5h ago   │ │
│         │ │🐍 django│ pypi │pypi-local  │890KB │1d ago   │ │
│         │ │📦 lodash│ npm  │npm-public  │567KB │2d ago   │ │
│         │ └─────────┴──────┴────────────┴──────┴─────────┘ │
│         │                                               │
│         │ [Previous] [1] [2] [3] [Next]                │
└─────────┴───────────────────────────────────────────────┘
```

### Funcionalidades
- Tabla de artefactos con ordenamiento por columnas
- Filtros avanzados por tipo, repositorio, tamaño, fecha
- Búsqueda en tiempo real por nombre de artefacto
- Paginación optimizada para grandes volúmenes
- Vista previa rápida de metadatos
- Acciones en lote (eliminar, mover, etc.)

### 3.2 Detalle de Artefacto (`/artifacts/:id`)

### Layout
```
┌─────────────────────────────────────────────────────────┐
│ Header (Breadcrumb: Artifacts > react@18.0.0)          │
├─────────┬───────────────────────────────────────────────┤
│ Sidebar │ ┌─────────────────────────────────────────────┐ │
│         │ │ 📦 react@18.0.0                           │ │
│         │ │    npm package from npm-private            │ │
│         │ │    [Download] [Delete] [Copy Install Cmd]   │ │
│         │ └─────────────────────────────────────────────┘ │
│         │                                               │
│         │ [Overview] [Dependencies] [Versions] [Security]│
│         │                                               │
│         │ ┌─────────────────────────────────────────────┐ │
│         │ │ Package Information                         │ │
│         │ │ • Name: react                               │ │
│         │ │ • Version: 18.0.0                          │ │
│         │ │ • Type: npm                                │ │
│         │ │ • Size: 2.3 MB                             │ │
│         │ │ • Checksum: sha256:abc123...               │ │
│         │ │ • Uploaded: 2024-01-15 14:30 UTC          │ │
│         │ │ • Uploaded by: developer@company.com       │ │
│         │ │                                             │ │
│         │ │ Dependencies (12)                           │ │
│         │ │ • loose-envify@^1.4.0                      │ │
│         │ │ • object-assign@^4.1.1                     │ │
│         │ │ • scheduler@^0.23.0                        │ │
│         │ └─────────────────────────────────────────────┘ │
└─────────┴───────────────────────────────────────────────┘
```

## 4. Búsqueda Avanzada (`/search`)

### Objetivo
Página dedicada a búsqueda avanzada con múltiples filtros y facetas para descubrimiento de artefactos.

### Layout
```
┌─────────────────────────────────────────────────────────┐
│ Header (Breadcrumb: Home > Search)                     │
├─────────┬───────────────────────────────────────────────┤
│ Sidebar │ ┌─────────────────────────────────────────────┐ │
│ Filters │ │ 🔍 Search packages, keywords, descriptions  │ │
│         │ └─────────────────────────────────────────────┘ │
│ Type    │                                               │
│ ☑️ Maven │ Results (234 found)              [Sort: Name▼]│
│ ☑️ npm   │                                               │
│ ☑️ PyPI  │ ┌─────────────────────────────────────────────┐ │
│ ☑️ Docker│ │ 📦 react                          ⭐ 45K   │ │
│         │ │    A JavaScript library for building UIs   │ │
│ Repos   │ │    npm • 18.0.0 • 2.3 MB • MIT License    │ │
│ ☑️ Public│ │    npm install react                       │ │
│ ☑️ Private│ │                                             │ │
│         │ │ 📦 vue                            ⭐ 38K   │ │
│ Size    │ │    Progressive framework for building UIs   │ │
│ ○ < 1MB │ │    npm • 3.2.0 • 1.8 MB • MIT License     │ │
│ ● 1-10MB│ │    npm install vue                         │ │
│ ○ > 10MB│ │                                             │ │
│         │ │ 📦 angular                        ⭐ 42K   │ │
│ Date    │ │    Platform for building mobile/desktop    │ │
│ ○ Today │ │    npm • 15.0.0 • 3.1 MB • MIT License    │ │
│ ● Week  │ └─────────────────────────────────────────────┘ │
│ ○ Month │                                               │
│ ○ Year  │ [Previous] [1] [2] [3] [Next]                │
└─────────┴───────────────────────────────────────────────┘
```

### Funcionalidades
- Búsqueda de texto completo en nombres, descripciones y metadatos
- Filtros por tipo de package (Maven, npm, PyPI, Docker)
- Filtros por repositorio (público, privado, específico)
- Filtros por tamaño y fecha de modificación
- Ordenamiento por relevancia, nombre, popularidad, fecha
- Vista de resultados con metadatos esenciales
- Comandos de instalación copiables

## 5. Gestión de Usuarios (`/users`)

### 5.1 Lista de Usuarios (`/users`)

### Objetivo
Administración de usuarios del sistema con capacidades de gestión de roles y permisos.

### Layout
```
┌─────────────────────────────────────────────────────────┐
│ Header (Breadcrumb: Home > Users)                      │
├─────────┬───────────────────────────────────────────────┤
│ Sidebar │ ┌─────────────────────────────────────────────┐ │
│         │ │ Users                           [+ Add User] │ │
│         │ ├─────────────────────────────────────────────┤ │
│         │ │ 🔍 Search users...       [Filter ▼]        │ │
│         │ ├─────────────────────────────────────────────┤ │
│         │ │ Role: [All ▼] Status: [All ▼]              │ │
│         │ └─────────────────────────────────────────────┘ │
│         │                                               │
│         │ ┌─────────────────────────────────────────────┐ │
│         │ │ Name │ Email │ Role │ Status │ Last Login │ Actions │
│         │ ├──────┼───────┼──────┼────────┼───────────┼─────────┤
│         │ │👤 John│john@co│Admin │Active  │2h ago     │[Edit][⋯]│
│         │ │👤 Jane│jane@co│User  │Active  │1d ago     │[Edit][⋯]│
│         │ │👤 Bob │bob@co │Read  │Inactive│1w ago     │[Edit][⋯]│
│         │ └──────┴───────┴──────┴────────┴───────────┴─────────┘ │
│         │                                               │
│         │ [Previous] [1] [2] [3] [Next]                │
└─────────┴───────────────────────────────────────────────┘
```

### Funcionalidades
- Lista de usuarios con información básica
- Filtros por rol y estado
- Búsqueda por nombre/email
- Creación y edición de usuarios
- Asignación de roles y políticas ABAC
- Activación/desactivación de usuarios

## 6. Configuración (`/settings`)

### 6.1 Gestión de Políticas (`/settings/policies`)

### Layout
```
┌─────────────────────────────────────────────────────────┐
│ Header (Breadcrumb: Settings > Policies)               │
├─────────┬───────────────────────────────────────────────┤
│ Sidebar │ ┌─────────────────────────────────────────────┐ │
│         │ │ ABAC Policies                 [+ New Policy]│ │
│         │ ├─────────────────────────────────────────────┤ │
│         │ │ 🔍 Search policies...                       │ │
│         │ └─────────────────────────────────────────────┘ │
│         │                                               │
│         │ ┌─────────────────────────────────────────────┐ │
│         │ │ 📋 Admin Full Access                       │ │
│         │ │    Allows full access to all resources     │ │
│         │ │    permit(principal has role "admin")      │ │
│         │ │    [Edit] [Duplicate] [Delete]             │ │
│         │ │                                             │ │
│         │ │ 📋 Repository Read Access                  │ │
│         │ │    Read access to specific repositories    │ │
│         │ │    permit(principal, action, resource)     │ │
│         │ │    when { resource.type == "repository" }  │ │
│         │ │    [Edit] [Duplicate] [Delete]             │ │
│         │ └─────────────────────────────────────────────┘ │
└─────────┴───────────────────────────────────────────────┘
```

### 6.2 Gestión de Tokens (`/settings/tokens`)

### Layout
```
┌─────────────────────────────────────────────────────────┐
│ Header (Breadcrumb: Settings > API Tokens)             │
├─────────┬───────────────────────────────────────────────┤
│ Sidebar │ ┌─────────────────────────────────────────────┐ │
│         │ │ API Tokens                    [+ New Token] │ │
│         │ ├─────────────────────────────────────────────┤ │
│         │ │ 🔍 Search tokens...                         │ │
│         │ └─────────────────────────────────────────────┘ │
│         │                                               │
│         │ ┌─────────────────────────────────────────────┐ │
│         │ │ 🔑 CI/CD Pipeline Token                    │ │
│         │ │    Created: 2024-01-15                     │ │
│         │ │    Last used: 2h ago                       │ │
│         │ │    Permissions: repository:read,write       │ │
│         │ │    [Regenerate] [Revoke] [Copy]            │ │
│         │ │                                             │ │
│         │ │ 🔑 Developer Access Token                  │ │
│         │ │    Created: 2024-01-10                     │ │
│         │ │    Last used: 5h ago                       │ │
│         │ │    Permissions: artifact:read               │ │
│         │ │    [Regenerate] [Revoke] [Copy]            │ │
│         │ └─────────────────────────────────────────────┘ │
└─────────┴───────────────────────────────────────────────┘
```

## 7. Autenticación (`/auth`)

### 7.1 Login (`/auth/login`)

### Layout
```
┌─────────────────────────────────────────────────────────┐
│                                                         │
│              ┌─────────────────────────┐                │
│              │                         │                │
│              │  🏢 Hodei Artifacts    │                │
│              │                         │                │
│              │  ┌───────────────────┐  │                │
│              │  │ Email             │  │                │
│              │  │ john@company.com  │  │                │
│              │  └───────────────────┘  │                │
│              │                         │                │
│              │  ┌───────────────────┐  │                │
│              │  │ Password          │  │                │
│              │  │ ●●●●●●●●●●●●●●   │  │                │
│              │  └───────────────────┘  │                │
│              │                         │                │
│              │  ☑️ Remember me        │                │
│              │                         │                │
│              │  [    Sign In    ]     │                │
│              │                         │                │
│              │  Forgot password?       │                │
│              └─────────────────────────┘                │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### Funcionalidades
- Formulario de login con email/password
- Opción "Remember me"
- Enlace para recuperación de contraseña
- Validación en tiempo real
- Redirección automática tras login exitoso

## Componentes Compartidos

### Header Global
```typescript
interface HeaderProps {
  user: User | null
  onLogout: () => void
}

// Funcionalidades:
// - Logo y nombre de la aplicación
// - Búsqueda global
// - Menú de usuario con perfil y logout
// - Breadcrumbs de navegación
```

### Sidebar Navigation
```typescript
interface NavigationItem {
  id: string
  label: string
  icon: string
  path: string
  children?: NavigationItem[]
  requiredPermissions?: string[]
}

// Funcionalidades:
// - Navegación jerárquica
// - Estado activo/inactivo
// - Colapsar/expandir
// - Filtrado por permisos de usuario
```

### Data Table
```typescript
interface DataTableProps<T> {
  data: T[]
  columns: Column<T>[]
  loading?: boolean
  onSort?: (field: keyof T, direction: 'asc' | 'desc') => void
  onFilter?: (filters: Record<string, any>) => void
  pagination?: PaginationProps
}

// Funcionalidades:
// - Ordenamiento por columnas
// - Filtrado avanzado
// - Paginación
// - Selección múltiple
// - Acciones en lote
```

### Upload Component
```typescript
interface UploadProps {
  repositoryId: string
  onUploadComplete: (artifact: Artifact) => void
  acceptedTypes?: string[]
  maxSize?: number
}

// Funcionalidades:
// - Drag & drop
// - Validación de archivos
// - Progress bar
// - Preview de metadatos
// - Validación de checksums
```

Esta especificación proporciona una base sólida para implementar una interfaz de usuario completa e intuitiva que aprovecha todas las capacidades del backend de Hodei Artifacts.