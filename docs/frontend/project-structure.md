# Project Structure

## Estructura del Proyecto Frontend

La estructura del proyecto frontend sigue las mejores prácticas modernas de React, implementando **Component-Based Architecture**, **Atomic Design**, **Feature-Based Organization** y patrones de la comunidad React para una organización escalable y mantenible.

```
frontend/                           # Directorio raíz del frontend
├── public/                         # Assets estáticos
│   ├── favicon.ico
│   ├── logo.svg
│   ├── manifest.json
│   └── robots.txt
├── src/                           # Código fuente principal
│   ├── App.tsx                    # Componente raíz con providers
│   ├── main.tsx                   # Punto de entrada
│   ├── index.css                  # Estilos globales con Tailwind
│   ├── vite-env.d.ts              # Tipos de Vite
│   ├── components/                # Design System - Atomic Design
│   │   ├── ui/                    # Componentes base (Atoms)
│   │   │   ├── button/           # Componente Button con variants
│   │   │   │   ├── Button.tsx
│   │   │   │   ├── Button.test.tsx
│   │   │   │   ├── Button.stories.tsx
│   │   │   │   ├── types.ts
│   │   │   │   └── index.ts
│   │   │   ├── input/            # Componente Input
│   │   │   ├── badge/            # Componente Badge
│   │   │   ├── spinner/          # Componente Spinner
│   │   │   ├── card/             # Componente Card
│   │   │   ├── dialog/           # Componente Modal/Dialog
│   │   │   └── index.ts          # Re-exportaciones
│   │   ├── forms/                # Componentes de formulario (Molecules)
│   │   │   ├── form-field/       # Campo de formulario compuesto
│   │   │   ├── search-box/       # Búsqueda con icono
│   │   │   ├── upload-zone/      # Zona de subida de archivos
│   │   │   └── index.ts
│   │   ├── layout/               # Componentes de layout (Organisms)
│   │   │   ├── header/           # Header con navegación
│   │   │   ├── sidebar/          # Sidebar colapsable
│   │   │   ├── breadcrumb/       # Migas de pan
│   │   │   ├── data-table/       # Tabla de datos con paginación
│   │   │   ├── modal/            # Modal overlay
│   │   │   └── index.ts
│   │   └── templates/            # Templates de página
│   │       ├── main-layout/      # Layout principal
│   │       ├── auth-layout/      # Layout de autenticación
│   │       └── index.ts
│   ├── features/                 # Organización por dominio de negocio
│   │   ├── auth/                 # Autenticación y autorización
│   │   │   ├── components/       # Componentes específicos
│   │   │   │   ├── LoginForm.tsx
│   │   │   │   ├── LogoutButton.tsx
│   │   │   │   ├── AuthProvider.tsx  # Context provider
│   │   │   │   └── index.ts
│   │   │   ├── hooks/            # Custom hooks de negocio
│   │   │   │   ├── useAuth.ts    # Hook principal de autenticación
│   │   │   │   ├── useLogin.ts   # Hook específico para login
│   │   │   │   ├── useLogout.ts  # Hook específico para logout
│   │   │   │   └── index.ts
│   │   │   ├── services/         # Servicios API
│   │   │   │   ├── auth.service.ts
│   │   │   │   └── index.ts
│   │   │   ├── types/            # Tipos específicos
│   │   │   │   ├── auth.types.ts
│   │   │   │   └── index.ts
│   │   │   └── index.ts          # API pública del feature
│   │   ├── repositories/         # Gestión de repositorios
│   │   │   ├── components/
│   │   │   │   ├── RepositoryCard.tsx      # Presentational component
│   │   │   │   ├── RepositoryList.tsx      # Presentational list
│   │   │   │   ├── RepositoryForm.tsx      # Form component
│   │   │   │   ├── CreateRepositoryModal.tsx # Compound component
│   │   │   │   └── index.ts
│   │   │   ├── hooks/
│   │   │   │   ├── useRepositories.ts      # Data fetching hook
│   │   │   │   ├── useRepository.ts        # Single repository hook
│   │   │   │   ├── useCreateRepository.ts  # Mutation hook
│   │   │   │   └── index.ts
│   │   │   ├── services/
│   │   │   │   ├── repository.service.ts
│   │   │   │   └── index.ts
│   │   │   ├── types/
│   │   │   │   ├── repository.types.ts
│   │   │   │   └── index.ts
│   │   │   └── index.ts
│   │   ├── artifacts/            # Gestión de artefactos
│   │   │   ├── components/
│   │   │   │   ├── ArtifactCard.tsx
│   │   │   │   ├── ArtifactList.tsx
│   │   │   │   ├── ArtifactUploadZone.tsx  # Render props component
│   │   │   │   ├── ArtifactViewer.tsx
│   │   │   │   └── index.ts
│   │   │   ├── hooks/
│   │   │   │   ├── useArtifacts.ts
│   │   │   │   ├── useArtifactUpload.ts
│   │   │   │   ├── useArtifactDownload.ts
│   │   │   │   └── index.ts
│   │   │   ├── services/
│   │   │   │   ├── artifact.service.ts
│   │   │   │   └── index.ts
│   │   │   ├── types/
│   │   │   │   ├── artifact.types.ts
│   │   │   │   └── index.ts
│   │   │   └── index.ts
│   │   ├── search/               # Búsqueda y descubrimiento
│   │   │   ├── components/
│   │   │   │   ├── SearchInput.tsx
│   │   │   │   ├── SearchFilters.tsx
│   │   │   │   ├── SearchResults.tsx
│   │   │   │   └── index.ts
│   │   │   ├── hooks/
│   │   │   │   ├── useSearch.ts
│   │   │   │   ├── useSearchFilters.ts
│   │   │   │   └── index.ts
│   │   │   ├── services/
│   │   │   │   ├── search.service.ts
│   │   │   │   └── index.ts
│   │   │   ├── types/
│   │   │   │   ├── search.types.ts
│   │   │   │   └── index.ts
│   │   │   └── index.ts
│   │   ├── users/                # Dominio: Gestión de usuarios
│   │   │   ├── components/
│   │   │   ├── hooks/
│   │   │   ├── services/
│   │   │   ├── types/
│   │   │   └── index.ts
│   │   └── settings/             # Dominio: Configuración
│   │       ├── components/
│   │       ├── hooks/
│   │       ├── services/
│   │       ├── types/
│   │       └── index.ts
│   ├── pages/                    # Route components (páginas)
│   │   ├── dashboard/
│   │   │   ├── DashboardPage.tsx
│   │   │   ├── DashboardPage.test.tsx
│   │   │   └── index.ts
│   │   ├── repositories/
│   │   │   ├── RepositoriesPage.tsx
│   │   │   ├── RepositoryDetailPage.tsx
│   │   │   └── index.ts
│   │   ├── artifacts/
│   │   │   ├── ArtifactsPage.tsx
│   │   │   ├── ArtifactDetailPage.tsx
│   │   │   └── index.ts
│   │   ├── search/
│   │   │   ├── SearchPage.tsx
│   │   │   └── index.ts
│   │   ├── users/
│   │   │   ├── UsersPage.tsx
│   │   │   ├── UserDetailPage.tsx
│   │   │   └── index.ts
│   │   ├── settings/
│   │   │   ├── SettingsPage.tsx
│   │   │   └── index.ts
│   │   ├── auth/
│   │   │   ├── LoginPage.tsx
│   │   │   └── index.ts
│   │   ├── not-found/
│   │   │   ├── NotFoundPage.tsx
│   │   │   └── index.ts
│   │   └── index.ts
│   ├── shared/                   # Utilidades y código compartido
│   │   ├── api/                  # Cliente HTTP y configuración
│   │   │   ├── client.ts         # Axios client configurado
│   │   │   ├── endpoints.ts      # Definición de endpoints
│   │   │   └── index.ts
│   │   ├── hooks/                # Hooks utilitarios reutilizables
│   │   │   ├── useDebounce.ts
│   │   │   ├── useLocalStorage.ts
│   │   │   ├── usePrevious.ts
│   │   │   ├── useErrorHandler.ts
│   │   │   └── index.ts
│   │   ├── stores/               # Estado global con Zustand
│   │   │   ├── ui.store.ts       # UI state (theme, sidebar, notifications)
│   │   │   ├── settings.store.ts # User preferences
│   │   │   └── index.ts
│   │   ├── types/                # Tipos globales
│   │   │   ├── api.types.ts      # Tipos generados de OpenAPI
│   │   │   ├── common.types.ts   # Tipos comunes
│   │   │   └── index.ts
│   │   ├── utils/                # Utilidades puras
│   │   │   ├── formatters.ts     # Formateo de datos
│   │   │   ├── validators.ts     # Validación de formularios
│   │   │   ├── constants.ts      # Constantes de la aplicación
│   │   │   ├── date.ts           # Utilidades de fecha
│   │   │   ├── sanitize.ts       # Sanitización de datos
│   │   │   └── index.ts
│   │   └── lib/                  # Configuración de librerías
│   │       ├── react-query.ts    # Configuración de React Query
│   │       ├── router.ts         # Configuración de React Router
│   │       └── index.ts
│   ├── router/                   # Configuración de rutas
│   │   ├── routes.tsx            # Definición de rutas con lazy loading
│   │   ├── providers.tsx         # Providers globales (Query, Auth, Theme)
│   │   └── index.ts
│   └── __tests__/                # Tests globales y configuración
│       ├── setup.ts
│       ├── mocks/
│       │   ├── handlers/         # MSW handlers por feature
│       │   │   ├── auth.handlers.ts
│       │   │   ├── repository.handlers.ts
│       │   │   └── index.ts
│       │   ├── data/            # Mock data
│       │   │   ├── repositories.ts
│       │   │   ├── artifacts.ts
│       │   │   └── index.ts
│       │   └── server.ts        # MSW server
│       ├── utils/
│       │   ├── test-utils.tsx   # Testing utilities
│       │   ├── render-with-providers.tsx
│       │   └── index.ts
│       └── fixtures/            # Test fixtures
│           ├── repository.fixtures.ts
│           └── index.ts
├── .env                         # Variables de entorno base
├── .env.local                   # Variables de entorno locales (git ignored)
├── .env.production              # Variables de entorno de producción
├── .gitignore
├── index.html                   # Template HTML principal
├── package.json                 # Dependencias y scripts
├── tailwind.config.js           # Configuración de Tailwind CSS
├── tsconfig.json               # Configuración de TypeScript
├── tsconfig.node.json          # Configuración de TypeScript para Node
├── vite.config.ts              # Configuración de Vite
├── vitest.config.ts            # Configuración de Vitest
├── postcss.config.js           # Configuración de PostCSS
├── eslint.config.js            # Configuración de ESLint
├── prettier.config.js          # Configuración de Prettier
└── README.md                   # Documentación del proyecto
```

## Convenciones de Naming

### Archivos y Directorios
- **Componentes**: PascalCase (`Button.tsx`, `RepositoryCard.tsx`)
- **Hooks**: camelCase con prefijo 'use' (`useAuth.ts`, `useRepositories.ts`)
- **Stores**: camelCase con sufijo 'Store' (`authStore.ts`, `repositoryStore.ts`)
- **Utilidades**: camelCase (`formatters.ts`, `validators.ts`)
- **Tipos**: camelCase (`user.ts`, `repository.ts`)
- **Páginas**: PascalCase con sufijo 'Page' (`DashboardPage.tsx`)
- **Directorios**: camelCase para features, PascalCase para componentes

### Exportaciones
```typescript
// Exportación por defecto para componentes principales
export default Button

// Exportación nombrada para utilidades y hooks
export { useAuth, useLogin }

// Re-exportación en archivos index.ts
export { default as Button } from './Button'
export { default as Input } from './Input'
```

## Estructura de Componentes

### Patrón de Estructura para Componentes
```
ComponentName/
├── ComponentName.tsx           # Componente principal
├── ComponentName.test.tsx      # Tests unitarios
├── ComponentName.stories.tsx   # Storybook stories (opcional)
├── hooks/                      # Hooks específicos del componente
│   ├── useComponentName.ts
│   └── index.ts
├── types.ts                    # Tipos específicos del componente
└── index.ts                    # Exportaciones públicas
```

### Ejemplo de Estructura de Componente con Variants
```typescript
// Button/Button.tsx
import { ButtonHTMLAttributes, forwardRef } from 'react'
import { cn } from '@/shared/utils'
import { ButtonVariant, ButtonSize } from './types'

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant
  size?: ButtonSize
  isLoading?: boolean
  children: React.ReactNode
}

const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = 'primary', size = 'md', isLoading, className, children, ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={cn(
          // Base styles
          'inline-flex items-center justify-center rounded-md font-medium transition-colors',
          // Variant styles
          buttonVariants({ variant, size }),
          // Loading state
          isLoading && 'opacity-50 cursor-not-allowed',
          className
        )}
        disabled={isLoading || props.disabled}
        {...props}
      >
        {isLoading && (
          <>
            <Spinner size="sm" className="mr-2" />
            <span>Loading...</span>
          </>
        )}
        {!isLoading && children}
      </button>
    )
  }
)

Button.displayName = 'Button'

export default Button
```

## Gestión de Assets

### Imágenes y Assets Estáticos
```
public/
├── images/
│   ├── logos/
│   │   ├── logo.svg
│   │   └── logo-dark.svg
│   ├── icons/
│   │   ├── maven.svg
│   │   ├── npm.svg
│   │   └── pypi.svg
│   └── illustrations/
│       ├── empty-state.svg
│       └── error-state.svg
├── fonts/
│   ├── inter/
│   └── jetbrains-mono/
└── favicons/
    ├── favicon.ico
    ├── icon-192.png
    └── icon-512.png
```

### Importación de Assets
```typescript
// Para assets en public/
const logoUrl = '/images/logos/logo.svg'

// Para assets dinámicos
import logoSvg from '@/assets/logo.svg'
```

## Configuración de Path Aliases

### tsconfig.json
```json
{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"],
      "@/shared/*": ["src/shared/*"],
      "@/features/*": ["src/features/*"],
      "@/pages/*": ["src/pages/*"],
      "@/components/*": ["src/shared/components/*"],
      "@/hooks/*": ["src/shared/hooks/*"],
      "@/stores/*": ["src/shared/stores/*"],
      "@/utils/*": ["src/shared/utils/*"],
      "@/types/*": ["src/shared/types/*"]
    }
  }
}
```

### vite.config.ts
```typescript
import { resolve } from 'path'

export default defineConfig({
  resolve: {
    alias: {
      '@': resolve(__dirname, './src'),
      '@/shared': resolve(__dirname, './src/shared'),
      '@/features': resolve(__dirname, './src/features'),
      '@/pages': resolve(__dirname, './src/pages'),
      '@/components': resolve(__dirname, './src/shared/components'),
      '@/hooks': resolve(__dirname, './src/shared/hooks'),
      '@/stores': resolve(__dirname, './src/shared/stores'),
      '@/utils': resolve(__dirname, './src/shared/utils'),
      '@/types': resolve(__dirname, './src/shared/types'),
    }
  }
})
```

## Estructura de Testing

### Organización de Tests
```
src/
├── components/
│   └── Button/
│       ├── Button.tsx
│       └── Button.test.tsx      # Tests unitarios junto al componente
├── features/
│   └── repositories/
│       ├── hooks/
│       │   ├── useRepositories.ts
│       │   └── useRepositories.test.ts
│       └── __tests__/           # Tests de integración de feature
│           └── repositories.integration.test.tsx
└── __tests__/                   # Tests globales
    ├── setup.ts
    ├── e2e/                     # Tests E2E
    │   ├── auth.spec.ts
    │   ├── repositories.spec.ts
    │   └── artifacts.spec.ts
    └── utils/
        └── test-utils.tsx
```

### Configuración de Testing
```typescript
// __tests__/setup.ts
import '@testing-library/jest-dom'
import { server } from './mocks/server'

beforeAll(() => server.listen())
afterEach(() => server.resetHandlers())
afterAll(() => server.close())
```

## Configuración de Herramientas

### package.json Scripts
```json
{
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "test": "vitest",
    "test:e2e": "playwright test",
    "test:coverage": "vitest --coverage",
    "lint": "eslint . --ext ts,tsx --report-unused-disable-directives --max-warnings 0",
    "lint:fix": "eslint . --ext ts,tsx --fix",
    "format": "prettier --write \"src/**/*.{ts,tsx,json,css,md}\"",
    "type-check": "tsc --noEmit",
    "storybook": "storybook dev -p 6006",
    "build-storybook": "storybook build"
  }
}
```

## Patrones de React de la Comunidad

### Custom Hooks Pattern
```typescript
// features/auth/hooks/useAuth.ts
export const useAuth = () => {
  const { data: user } = useQuery({ queryKey: ['user'], queryFn: getUser })
  const loginMutation = useMutation({ mutationFn: login })
  const logoutMutation = useMutation({ mutationFn: logout })

  return {
    user,
    login: loginMutation.mutate,
    logout: logoutMutation.mutate,
    isLoading: loginMutation.isPending || logoutMutation.isPending
  }
}
```

### Compound Components Pattern
```typescript
// components/ui/modal/Modal.tsx
const ModalContext = createContext<ModalContextType>({})

export const Modal = ({ children, isOpen, onClose }: ModalProps) => {
  return (
    <ModalContext.Provider value={{ isOpen, onClose }}>
      <Dialog open={isOpen} onClose={onClose}>
        {children}
      </Dialog>
    </ModalContext.Provider>
  )
}

Modal.Header = ModalHeader
Modal.Body = ModalBody
Modal.Footer = ModalFooter
```

### Render Props Pattern
```typescript
// features/artifacts/components/ArtifactUploadZone.tsx
export const ArtifactUploadZone = ({ children }: ArtifactUploadZoneProps) => {
  const [files, setFiles] = useState<File[]>([])
  
  return children({
    files,
    onFilesChange: setFiles,
    isUploading: false,
    upload: async () => {
      // Upload logic
    }
  })
}
```

### Presentational vs Container Components
```typescript
// Presentational Component
const RepositoryCard = ({ repository, onEdit, onDelete }: RepositoryCardProps) => (
  <Card>
    <CardHeader>
      <CardTitle>{repository.name}</CardTitle>
      <CardDescription>{repository.description}</CardDescription>
    </CardHeader>
    <CardFooter>
      <Button onClick={() => onEdit(repository.id)}>Edit</Button>
      <Button variant="destructive" onClick={() => onDelete(repository.id)}>
        Delete
      </Button>
    </CardFooter>
  </Card>
)

// Container Component
const RepositoryListContainer = () => {
  const { data: repositories, isLoading } = useRepositories()
  const { mutate: deleteRepository } = useDeleteRepository()

  if (isLoading) return <Spinner />

  return (
    <RepositoryList
      repositories={repositories}
      onEdit={(id) => navigate(`/repositories/${id}/edit`)}
      onDelete={deleteRepository}
    />
  )
}
```

### Service Layer Pattern
```typescript
// features/repositories/services/repository.service.ts
export const repositoryService = {
  getAll: async (): Promise<Repository[]> => {
    const response = await api.get('/repositories')
    return response.data
  },
  
  getById: async (id: string): Promise<Repository> => {
    const response = await api.get(`/repositories/${id}`)
    return response.data
  },
  
  create: async (data: CreateRepositoryData): Promise<Repository> => {
    const response = await api.post('/repositories', data)
    return response.data
  }
}
```

Esta estructura proporciona una base sólida y escalable para el desarrollo del frontend, siguiendo las mejores prácticas modernas de React y patrones de la comunidad, permitiendo un crecimiento organizado del proyecto.