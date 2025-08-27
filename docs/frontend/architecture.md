# Frontend Architecture

## Visión General

El frontend de Hodei Artifacts adopta una arquitectura moderna específicamente diseñada para aplicaciones React, siguiendo las mejores prácticas de la comunidad React. La arquitectura combina **Component-Based Architecture**, **Atomic Design**, y **Feature-Based Organization** para crear una base sólida, escalable y mantenible.

## Patrones Arquitectónicos Principales

### 1. Component-Based Architecture + Atomic Design

Organización jerárquica de componentes reutilizables siguiendo los principios de Atomic Design:

```
src/components/
├── ui/                 # Design System - Componentes base
│   ├── button/        # Atoms - Componentes primitivos
│   ├── input/
│   ├── badge/
│   └── spinner/
├── forms/             # Molecules - Combinaciones simples  
│   ├── form-field/
│   ├── search-box/
│   └── upload-zone/
├── layout/            # Organisms - Componentes complejos
│   ├── header/
│   ├── sidebar/
│   ├── data-table/
│   └── modal/
└── templates/         # Templates - Layouts de página
    ├── main-layout/
    └── auth-layout/
```

### 2. Feature-Based Organization (Domain-Driven)

Organización por dominio de negocio, no por tipo técnico, siguiendo el patrón de organización por features:

```
src/features/
├── auth/              # Dominio: Autenticación
│   ├── components/    # Componentes específicos del dominio
│   ├── hooks/         # Lógica de negocio encapsulada
│   ├── services/      # Comunicación con APIs
│   ├── stores/        # Estado específico del dominio
│   └── types/         # Tipos específicos
├── repositories/      # Dominio: Gestión de repositorios
│   ├── components/
│   ├── hooks/
│   ├── services/
│   ├── stores/
│   └── types/
├── artifacts/         # Dominio: Gestión de artefactos
├── search/           # Dominio: Búsqueda y descubrimiento  
└── users/            # Dominio: Gestión de usuarios
```

### 3. Layered Architecture para React

Arquitectura en capas adaptada específicamente para aplicaciones React modernas:

```
┌─────────────────────────────────────┐
│        Presentation Layer           │ ← Pages + Route Components
├─────────────────────────────────────┤
│         Component Layer             │ ← UI Components + Feature Components
├─────────────────────────────────────┤
│       Business Logic Layer          │ ← Custom Hooks + State Management
├─────────────────────────────────────┤
│         Service Layer               │ ← API Services + External Integrations
├─────────────────────────────────────┤
│          Data Layer                 │ ← State Stores + Cache + Local Storage
└─────────────────────────────────────┘
```

## Arquitectura de Estado

### 1. State Management Strategy

**Multi-Store Approach** siguiendo las mejores prácticas de la comunidad React, utilizando diferentes herramientas según el tipo de estado:

```typescript
// Estado del servidor (cache, sincronización) - React Query
@tanstack/react-query
├── Queries (operaciones GET)
├── Mutations (operaciones POST/PUT/DELETE)  
├── Gestión de cache automática
└── Background refetching inteligente

// Estado global de aplicación - Zustand
Zustand stores
├── authStore (usuario, autenticación, permisos)
├── uiStore (tema, sidebar, notificaciones, modals)
├── settingsStore (preferencias de usuario)
└── Feature-specific stores cuando sea necesario

// Estado local de componente - React hooks nativos
React useState/useReducer
├── Form state (estado temporal de formularios)
├── UI state (estado local de componentes)
├── Component-specific state
└── Temporary state (estado efímero)
```

### 2. Custom Hooks Pattern (Business Logic Encapsulation)

Encapsulación de lógica de negocio en hooks reutilizables, siguiendo el patrón de custom hooks de la comunidad React:

```typescript
// Business logic hook para gestión de repositorios
export const useRepositoryManagement = () => {
  const queryClient = useQueryClient()
  const { addNotification } = useNotificationStore()
  
  // Query para obtener repositorios
  const repositories = useQuery({
    queryKey: ['repositories'],
    queryFn: repositoryService.getAll,
    staleTime: 5 * 60 * 1000 // 5 minutos
  })
  
  // Mutation para crear repositorio
  const createRepository = useMutation({
    mutationFn: repositoryService.create,
    onSuccess: (newRepository) => {
      // Invalidar cache y actualizar queries
      queryClient.invalidateQueries(['repositories'])
      queryClient.setQueryData(['repository', newRepository.id], newRepository)
      
      // Mostrar notificación
      addNotification({
        type: 'success',
        title: 'Repository Created',
        message: `Repository "${newRepository.name}" created successfully`
      })
    },
    onError: (error) => {
      addNotification({
        type: 'error',
        title: 'Creation Failed',
        message: error.message
      })
    }
  })
  
  // Mutation para eliminar repositorio
  const deleteRepository = useMutation({
    mutationFn: repositoryService.delete,
    onSuccess: (_, id) => {
      // Actualizar cache optimísticamente
      queryClient.setQueryData(['repositories'], (old: any) => 
        old?.filter((repo: Repository) => repo.id !== id)
      )
      queryClient.removeQueries(['repository', id])
    }
  })
  
  return {
    repositories: repositories.data,
    isLoading: repositories.isLoading,
    error: repositories.error,
    createRepository: createRepository.mutate,
    deleteRepository: deleteRepository.mutate,
    isCreating: createRepository.isPending,
    isDeleting: deleteRepository.isPending
  }
}
```

### 3. Compound Components Pattern

Para componentes complejos que requieren múltiples partes, siguiendo el patrón de compound components de React:

```typescript
// components/data-table/DataTable.tsx
interface DataTableProps {
  children: React.ReactNode
  className?: string
}

const DataTable = ({ children, className }: DataTableProps) => {
  return (
    <div className={cn('data-table', className)}>
      {children}
    </div>
  )
}

// Subcomponentes como propiedades estáticas
DataTable.Header = ({ children }: { children: React.ReactNode }) => (
  <thead className="data-table-header">{children}</thead>
)

DataTable.Body = ({ children }: { children: React.ReactNode }) => (
  <tbody className="data-table-body">{children}</tbody>
)

DataTable.Row = ({ children, onClick }: { 
  children: React.ReactNode 
  onClick?: () => void 
}) => (
  <tr 
    className="data-table-row" 
    onClick={onClick}
    style={{ cursor: onClick ? 'pointer' : 'default' }}
  >
    {children}
  </tr>
)

DataTable.Cell = ({ children, align = 'left' }: { 
  children: React.ReactNode 
  align?: 'left' | 'center' | 'right' 
}) => (
  <td className={`data-table-cell text-${align}`}>
    {children}
  </td>
)

// Uso con compound components
<DataTable>
  <DataTable.Header>
    <DataTable.Row>
      <DataTable.Cell>Name</DataTable.Cell>
      <DataTable.Cell>Type</DataTable.Cell>
      <DataTable.Cell align="right">Size</DataTable.Cell>
    </DataTable.Row>
  </DataTable.Header>
  <DataTable.Body>
    {repositories.map(repo => (
      <DataTable.Row 
        key={repo.id} 
        onClick={() => navigateToRepository(repo.id)}
      >
        <DataTable.Cell>{repo.name}</DataTable.Cell>
        <DataTable.Cell>
          <Badge variant="primary">{repo.type}</Badge>
        </DataTable.Cell>
        <DataTable.Cell align="right">
          {formatBytes(repo.size)}
        </DataTable.Cell>
      </DataTable.Row>
    ))}
  </DataTable.Body>
</DataTable>
```

### 4. Render Props / Children as Function Pattern

Para máxima flexibilidad en componentes de datos, utilizando el patrón render props de React:

```typescript
// components/data-fetcher/DataFetcher.tsx
interface DataFetcherProps<T> {
  queryKey: string[]
  queryFn: () => Promise<T>
  children: (props: {
    data: T | undefined
    isLoading: boolean
    error: Error | null
    refetch: () => void
    isRefetching: boolean
  }) => React.ReactNode
  fallback?: React.ReactNode
}

const DataFetcher = <T,>({ 
  queryKey, 
  queryFn, 
  children, 
  fallback 
}: DataFetcherProps<T>) => {
  const { data, isLoading, error, refetch, isRefetching } = useQuery({
    queryKey,
    queryFn,
    retry: 2,
    staleTime: 30000
  })
  
  if (isLoading && fallback) {
    return <>{fallback}</>
  }
  
  return (
    <>
      {children({
        data,
        isLoading,
        error,
        refetch,
        isRefetching
      })}
    </>
  )
}

// Uso con render props
<DataFetcher 
  queryKey={['repositories']} 
  queryFn={repositoryService.getAll}
  fallback={<RepositoryListSkeleton count={5} />}
>
  {({ data: repositories, isLoading, error, refetch, isRefetching }) => (
    <div>
      <div className="flex justify-between items-center mb-4">
        <h2>Repositories</h2>
        <Button 
          variant="ghost" 
          onClick={refetch}
          isLoading={isRefetching}
          leftIcon={<RefreshIcon />}
        >
          Refresh
        </Button>
      </div>
      
      {isLoading && <RepositoryListSkeleton count={3} />}
      {error && (
        <ErrorMessage 
          error={error}
          onRetry={refetch}
        />
      )}
      {repositories && (
        <RepositoryList 
          repositories={repositories}
          onRepositoryClick={navigateToRepository}
        />
      )}
    </div>
  )}
</DataFetcher>
```

## Separación de Responsabilidades

### 1. Presentational vs Container Components

**Presentational Components** (Dumb/Pure Components) - Componentes de presentación:
- Solo reciben props y renderizan UI
- No manejan estado de negocio ni efectos secundarios
- Fáciles de testear y reutilizar
- Altamente desacoplados del contexto de la aplicación

```typescript
// components/repository-card/RepositoryCard.tsx
interface RepositoryCardProps {
  repository: Repository
  onEdit?: (id: string) => void
  onDelete?: (id: string) => void
  onView?: (id: string) => void
  className?: string
}

const RepositoryCard = ({ 
  repository, 
  onEdit, 
  onDelete, 
  onView, 
  className 
}: RepositoryCardProps) => {
  return (
    <Card className={cn('repository-card', className)}>
      <Card.Header>
        <div className="flex items-center justify-between">
          <h3 className="text-lg font-semibold">{repository.name}</h3>
          <Badge variant="primary">{repository.type}</Badge>
        </div>
      </Card.Header>
      <Card.Body>
        {repository.description && (
          <p className="text-gray-600">{repository.description}</p>
        )}
        <div className="mt-2 flex items-center space-x-2 text-sm text-gray-500">
          <span>Artifacts: {repository.artifactCount}</span>
          <span>•</span>
          <span>Size: {formatBytes(repository.totalSize)}</span>
        </div>
      </Card.Body>
      <Card.Footer className="flex justify-end space-x-2">
        {onView && (
          <Button 
            variant="ghost" 
            size="sm"
            onClick={() => onView(repository.id)}
          >
            View
          </Button>
        )}
        {onEdit && (
          <Button 
            variant="secondary" 
            size="sm"
            onClick={() => onEdit(repository.id)}
          >
            Edit
          </Button>
        )}
        {onDelete && (
          <Button 
            variant="danger" 
            size="sm"
            onClick={() => onDelete(repository.id)}
          >
            Delete
          </Button>
        )}
      </Card.Footer>
    </Card>
  )
}
```

**Container Components** (Smart Components) - Componentes contenedores:
- Manejan estado y lógica de negocio
- Conectan con APIs, stores y servicios externos
- Pasan datos a presentational components
- Gestionan efectos secundarios y ciclo de vida

```typescript
// features/repositories/components/RepositoryListContainer.tsx
const RepositoryListContainer = () => {
  const { 
    repositories, 
    isLoading, 
    error,
    createRepository, 
    deleteRepository,
    refetch
  } = useRepositoryManagement()
  
  const navigate = useNavigate()
  const { handleError } = useErrorHandler()
  
  const handleEdit = (id: string) => {
    navigate(`/repositories/${id}/edit`)
  }
  
  const handleDelete = async (id: string) => {
    try {
      await deleteRepository(id)
    } catch (error) {
      handleError(error, 'Repository deletion')
    }
  }
  
  const handleView = (id: string) => {
    navigate(`/repositories/${id}`)
  }
  
  if (isLoading) {
    return <RepositoryListSkeleton count={5} />
  }
  
  if (error) {
    return (
      <ErrorMessage 
        error={error}
        onRetry={refetch}
        title="Failed to load repositories"
      />
    )
  }
  
  return (
    <div className="space-y-4">
      {repositories?.map(repo => (
        <RepositoryCard 
          key={repo.id}
          repository={repo}
          onEdit={handleEdit}
          onDelete={handleDelete}
          onView={handleView}
        />
      ))}
    </div>
  )
}
```

### 2. Service Layer Pattern

Encapsulación de lógica de comunicación con APIs, siguiendo el patrón de service layer de React:

```typescript
// services/api/repository.service.ts
import { apiClient } from '@/shared/api/client'
import type { 
  Repository, 
  CreateRepositoryRequest, 
  UpdateRepositoryRequest,
  RepositoryListResponse 
} from '@/shared/types/api'

export interface RepositoryFilters {
  type?: 'maven' | 'npm' | 'pypi'
  status?: 'active' | 'inactive'
  search?: string
  limit?: number
  offset?: number
}

class RepositoryService {
  private readonly basePath = '/repositories'

  async getAll(filters?: RepositoryFilters): Promise<RepositoryListResponse> {
    const params = new URLSearchParams()
    
    if (filters) {
      Object.entries(filters).forEach(([key, value]) => {
        if (value !== undefined && value !== '') {
          params.append(key, String(value))
        }
      })
    }

    const response = await apiClient.get<RepositoryListResponse>(
      `${this.basePath}?${params.toString()}`
    )
    return response.data
  }

  async getById(id: string): Promise<Repository> {
    const response = await apiClient.get<Repository>(`${this.basePath}/${id}`)
    return response.data
  }

  async create(data: CreateRepositoryRequest): Promise<Repository> {
    const response = await apiClient.post<Repository>(this.basePath, data)
    return response.data
  }

  async update(id: string, data: UpdateRepositoryRequest): Promise<Repository> {
    const response = await apiClient.put<Repository>(`${this.basePath}/${id}`, data)
    return response.data
  }

  async delete(id: string): Promise<void> {
    await apiClient.delete(`${this.basePath}/${id}`)
  }

  async checkNameAvailability(name: string): Promise<{ available: boolean }> {
    const response = await apiClient.get<{ available: boolean }>(
      `${this.basePath}/check-name?name=${encodeURIComponent(name)}`
    )
    return response.data
  }
}

export const repositoryService = new RepositoryService()
```

## Router Architecture

### 1. Route-Based Code Splitting

Organización de rutas con lazy loading automático:

```typescript
// app/router.tsx
import { createBrowserRouter } from 'react-router-dom'
import { lazy, Suspense } from 'react'
import { PageLoader } from '@/components/ui/page-loader'

// Lazy loading de páginas
const DashboardPage = lazy(() => import('@/pages/dashboard/DashboardPage'))
const RepositoriesPage = lazy(() => import('@/pages/repositories/RepositoriesPage'))
const RepositoryDetailPage = lazy(() => import('@/pages/repositories/RepositoryDetailPage'))

const SuspenseWrapper = ({ children }: { children: React.ReactNode }) => (
  <Suspense fallback={<PageLoader />}>{children}</Suspense>
)

export const router = createBrowserRouter([
  {
    path: '/',
    element: <MainLayout />,
    children: [
      {
        index: true,
        element: <SuspenseWrapper><DashboardPage /></SuspenseWrapper>
      },
      {
        path: 'repositories',
        children: [
          {
            index: true,
            element: <SuspenseWrapper><RepositoriesPage /></SuspenseWrapper>
          },
          {
            path: ':id',
            element: <SuspenseWrapper><RepositoryDetailPage /></SuspenseWrapper>
          }
        ]
      }
    ]
  }
])
```

### 2. Protected Routes Pattern

```typescript
// components/auth/ProtectedRoute.tsx
interface ProtectedRouteProps {
  children: React.ReactNode
  requiredPermissions?: string[]
}

const ProtectedRoute = ({ children, requiredPermissions = [] }: ProtectedRouteProps) => {
  const { isAuthenticated, user } = useAuthStore()
  const location = useLocation()
  
  if (!isAuthenticated) {
    return <Navigate to="/auth/login" state={{ from: location }} replace />
  }
  
  if (requiredPermissions.length > 0 && !hasPermissions(user, requiredPermissions)) {
    return <Navigate to="/unauthorized" replace />
  }
  
  return <>{children}</>
}
```

## Error Handling Architecture

### 1. Error Boundary Strategy

```typescript
// components/error/ErrorBoundary.tsx
class ErrorBoundary extends Component<Props, State> {
  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    // Log to monitoring service
    console.error('Error caught:', error, errorInfo)
    
    if (import.meta.env.PROD) {
      // Send to Sentry or similar
      this.logErrorToService(error, errorInfo)
    }
  }

  render() {
    if (this.state.hasError) {
      return <ErrorFallback error={this.state.error} />
    }
    return this.props.children
  }
}

// Uso jerárquico
<ErrorBoundary fallback={<AppErrorFallback />}>
  <App>
    <ErrorBoundary fallback={<PageErrorFallback />}>
      <Routes />
    </ErrorBoundary>
  </App>
</ErrorBoundary>
```

### 2. Global Error Handler Hook

```typescript
// hooks/useErrorHandler.ts
export const useErrorHandler = () => {
  const { addNotification } = useNotificationStore()
  
  return useCallback((error: unknown, context?: string) => {
    // Log error
    console.error(`Error in ${context}:`, error)
    
    // Extract user-friendly message
    const message = extractErrorMessage(error)
    
    // Show notification
    addNotification({
      type: 'error',
      title: context ? `${context} Error` : 'Error',
      message
    })
    
    // Report to monitoring if in production
    if (import.meta.env.PROD) {
      reportError(error, context)
    }
  }, [addNotification])
}
```

## Performance Architecture

### 1. Code Splitting Strategies

**Route-Level Splitting**:
```typescript
// Automático con React.lazy() y Suspense
const RepositoriesPage = lazy(() => import('@/pages/repositories/RepositoriesPage'))

// Chunking manual con webpack comments
const RepositoriesPage = lazy(() => 
  import(
    /* webpackChunkName: "repositories" */ 
    '@/pages/repositories/RepositoriesPage'
  )
)
```

**Component-Level Splitting**:
```typescript
// Para componentes pesados que no siempre se renderizan
const DataVisualization = lazy(() => import('@/components/data-visualization/DataVisualization'))

const Dashboard = () => {
  const [showChart, setShowChart] = useState(false)
  
  return (
    <div>
      <h1>Dashboard</h1>
      {showChart && (
        <Suspense fallback={<ChartSkeleton />}>
          <DataVisualization />
        </Suspense>
      )}
    </div>
  )
}
```

### 2. Memoization Strategy

**Component Memoization**:
```typescript
// React.memo para componentes puros
const RepositoryCard = memo(({ repository, onEdit, onDelete }: RepositoryCardProps) => {
  return (
    <Card>
      {/* ... */}
    </Card>
  )
})

// Custom comparison function
const RepositoryCard = memo(
  ({ repository, onEdit, onDelete }: RepositoryCardProps) => {
    return <Card>{/* ... */}</Card>
  },
  (prevProps, nextProps) => {
    return prevProps.repository.id === nextProps.repository.id &&
           prevProps.repository.updatedAt === nextProps.repository.updatedAt
  }
)
```

**Hook Memoization**:
```typescript
// useMemo para cálculos costosos
const useRepositoryStats = (repositories: Repository[]) => {
  const stats = useMemo(() => {
    return {
      total: repositories.length,
      byType: groupBy(repositories, 'type'),
      totalSize: repositories.reduce((acc, repo) => acc + repo.size, 0)
    }
  }, [repositories])
  
  return stats
}

// useCallback para funciones que se pasan como props
const RepositoryList = ({ repositories }: RepositoryListProps) => {
  const handleEdit = useCallback((id: string) => {
    // Lógica de edición
  }, [])
  
  const handleDelete = useCallback((id: string) => {
    // Lógica de eliminación
  }, [])
  
  return (
    <div>
      {repositories.map(repo => (
        <RepositoryCard 
          key={repo.id}
          repository={repo}
          onEdit={handleEdit}
          onDelete={handleDelete}
        />
      ))}
    </div>
  )
}
```

### 3. Virtual Scrolling

Para listas grandes de elementos:

```typescript
// hooks/useVirtualScroll.ts
export const useVirtualScroll = <T>({
  items,
  itemHeight,
  containerHeight,
  overscan = 5
}: VirtualScrollOptions<T>) => {
  const [scrollTop, setScrollTop] = useState(0)
  
  const visibleItems = useMemo(() => {
    const startIndex = Math.floor(scrollTop / itemHeight)
    const endIndex = Math.min(
      startIndex + Math.ceil(containerHeight / itemHeight) + overscan,
      items.length
    )
    
    return items.slice(startIndex, endIndex).map((item, index) => ({
      item,
      index: startIndex + index,
      top: (startIndex + index) * itemHeight
    }))
  }, [items, scrollTop, itemHeight, containerHeight, overscan])
  
  return { visibleItems, setScrollTop }
}

// Uso en componente
const VirtualRepositoryList = ({ repositories }: Props) => {
  const { visibleItems, setScrollTop } = useVirtualScroll({
    items: repositories,
    itemHeight: 80,
    containerHeight: 600
  })
  
  return (
    <div 
      style={{ height: 600, overflow: 'auto' }}
      onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
    >
      <div style={{ height: repositories.length * 80 }}>
        {visibleItems.map(({ item, index, top }) => (
          <div 
            key={item.id}
            style={{ position: 'absolute', top, height: 80 }}
          >
            <RepositoryCard repository={item} />
          </div>
        ))}
      </div>
    </div>
  )
}
```

## Testing Architecture

### 1. Testing Pyramid Strategy

```
                     E2E Tests (Playwright)
                   ├─ User workflows
                   ├─ Cross-browser testing  
                   └─ API integration testing
                   
              Integration Tests (React Testing Library)
            ├─ Feature testing
            ├─ Hook testing with providers
            ├─ API mocking (MSW)
            └─ Component interaction testing
            
        Unit Tests (Vitest + React Testing Library)
      ├─ Individual component testing
      ├─ Custom hook testing  
      ├─ Utility function testing
      └─ Business logic testing
```

### 2. Testing Patterns

**Component Testing**:
```typescript
// __tests__/components/RepositoryCard.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { RepositoryCard } from '../RepositoryCard'

const mockRepository = {
  id: '1',
  name: 'test-repo',
  type: 'maven' as const,
  description: 'Test repository'
}

describe('RepositoryCard', () => {
  it('renders repository information correctly', () => {
    const onEdit = vi.fn()
    const onDelete = vi.fn()
    
    render(
      <RepositoryCard 
        repository={mockRepository} 
        onEdit={onEdit} 
        onDelete={onDelete} 
      />
    )
    
    expect(screen.getByText('test-repo')).toBeInTheDocument()
    expect(screen.getByText('Test repository')).toBeInTheDocument()
    expect(screen.getByText('maven')).toBeInTheDocument()
  })
  
  it('calls onEdit when edit button is clicked', () => {
    const onEdit = vi.fn()
    const onDelete = vi.fn()
    
    render(
      <RepositoryCard 
        repository={mockRepository} 
        onEdit={onEdit} 
        onDelete={onDelete} 
      />
    )
    
    fireEvent.click(screen.getByRole('button', { name: /edit/i }))
    expect(onEdit).toHaveBeenCalledWith('1')
  })
})
```

**Hook Testing**:
```typescript
// __tests__/hooks/useRepositoryManagement.test.ts
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useRepositoryManagement } from '../useRepositoryManagement'

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
  })
  
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      {children}
    </QueryClientProvider>
  )
}

describe('useRepositoryManagement', () => {
  it('loads repositories on mount', async () => {
    const { result } = renderHook(() => useRepositoryManagement(), {
      wrapper: createWrapper()
    })
    
    await waitFor(() => {
      expect(result.current.repositories).toBeDefined()
    })
  })
})
```

## Security Architecture

### 1. Content Security Policy (CSP)

```typescript
// Security headers configuration
const securityHeaders = {
  'Content-Security-Policy': [
    "default-src 'self'",
    "script-src 'self' 'unsafe-inline'", // Only in development
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: https:",
    "connect-src 'self' https://api.hodei-artifacts.com",
    "font-src 'self' https://fonts.gstatic.com"
  ].join('; '),
  'X-Frame-Options': 'DENY',
  'X-Content-Type-Options': 'nosniff',
  'Referrer-Policy': 'strict-origin-when-cross-origin'
}
```

### 2. Input Sanitization

```typescript
// utils/sanitize.ts
import DOMPurify from 'dompurify'

export const sanitizeHtml = (dirty: string): string => {
  return DOMPurify.sanitize(dirty, {
    ALLOWED_TAGS: ['b', 'i', 'em', 'strong', 'code', 'pre'],
    ALLOWED_ATTR: []
  })
}

export const sanitizeInput = (input: string): string => {
  return input.trim().replace(/[<>]/g, '')
}
```

### 3. Token Management

```typescript
// utils/tokenManager.ts
class TokenManager {
  private static readonly TOKEN_KEY = 'auth_token'
  private static readonly REFRESH_TOKEN_KEY = 'refresh_token'
  
  static getToken(): string | null {
    return localStorage.getItem(this.TOKEN_KEY)
  }
  
  static setToken(token: string): void {
    localStorage.setItem(this.TOKEN_KEY, token)
  }
  
  static clearTokens(): void {
    localStorage.removeItem(this.TOKEN_KEY)
    localStorage.removeItem(this.REFRESH_TOKEN_KEY)
  }
  
  static isTokenExpired(token: string): boolean {
    try {
      const payload = JSON.parse(atob(token.split('.')[1]))
      return payload.exp * 1000 < Date.now()
    } catch {
      return true
    }
  }
}
```

## Monitoreo y Observabilidad

### 1. Performance Monitoring

```typescript
// utils/performance.ts
export const measurePerformance = (name: string, fn: () => void | Promise<void>) => {
  const start = performance.now()
  
  if (fn.constructor.name === 'AsyncFunction') {
    return (fn as () => Promise<void>)().finally(() => {
      const duration = performance.now() - start
      console.log(`${name} took ${duration.toFixed(2)}ms`)
      
      // Send to analytics if in production
      if (import.meta.env.PROD) {
        // analytics.track('performance', { name, duration })
      }
    })
  } else {
    const result = (fn as () => void)()
    const duration = performance.now() - start
    console.log(`${name} took ${duration.toFixed(2)}ms`)
    return result
  }
}

// Hook para medir rendering time
export const useRenderTime = (componentName: string) => {
  useEffect(() => {
    const renderTime = performance.now()
    console.log(`${componentName} rendered at ${renderTime}`)
    
    return () => {
      const unmountTime = performance.now()
      console.log(`${componentName} unmounted after ${unmountTime - renderTime}ms`)
    }
  }, [componentName])
}
```

### 2. Error Reporting

```typescript
// utils/errorReporting.ts
interface ErrorReport {
  message: string
  stack?: string
  componentStack?: string
  userId?: string
  url: string
  userAgent: string
  timestamp: number
}

export const reportError = (error: Error, errorInfo?: any) => {
  const report: ErrorReport = {
    message: error.message,
    stack: error.stack,
    componentStack: errorInfo?.componentStack,
    userId: useAuthStore.getState().user?.id,
    url: window.location.href,
    userAgent: navigator.userAgent,
    timestamp: Date.now()
  }
  
  if (import.meta.env.PROD) {
    // Send to error tracking service
    fetch('/api/errors', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(report)
    })
  } else {
    console.error('Error Report:', report)
  }
}
```

Esta arquitectura moderna para React proporciona una base sólida, escalable y mantenible siguiendo las mejores prácticas de la comunidad React actual.