# API Integration

## Integración con Backend API

Esta documentación describe la estrategia de integración con el backend de Hodei Artifacts, incluyendo configuración de clientes HTTP, manejo de estado servidor, tipado automático y patrones de error handling.

## Arquitectura de Integración

### Overview
```
Frontend (React)
├── API Client Layer (Axios + Interceptors)
├── Service Layer (Feature-specific APIs)
├── Hook Layer (React Query + Custom Hooks)
├── Store Layer (Zustand for global state)
└── Component Layer (UI Components)
```

### Principios de Diseño
1. **Type Safety**: Tipos automáticos desde OpenAPI spec
2. **Centralized Configuration**: Un cliente HTTP configurado globalmente
3. **Error Handling**: Manejo consistente de errores en toda la app
4. **Caching**: Cache inteligente con React Query
5. **Optimistic Updates**: Updates optimistas donde sea apropiado

## Configuración del Cliente HTTP

### Base API Client

```typescript
// src/shared/api/client.ts
import axios, { AxiosInstance, AxiosError, AxiosResponse } from 'axios'
import { useAuthStore } from '@/shared/stores/authStore'
import { useNotificationStore } from '@/shared/stores/notificationStore'

const API_BASE_URL = import.meta.env.VITE_API_URL || 'http://localhost:8080/v2'

class ApiClient {
  private instance: AxiosInstance

  constructor() {
    this.instance = axios.create({
      baseURL: API_BASE_URL,
      timeout: 10000,
      headers: {
        'Content-Type': 'application/json'
      }
    })

    this.setupInterceptors()
  }

  private setupInterceptors() {
    // Request interceptor para auth
    this.instance.interceptors.request.use(
      (config) => {
        const token = useAuthStore.getState().token
        if (token) {
          config.headers.Authorization = `Bearer ${token}`
        }
        return config
      },
      (error) => Promise.reject(error)
    )

    // Response interceptor para errores globales
    this.instance.interceptors.response.use(
      (response: AxiosResponse) => response,
      (error: AxiosError) => {
        this.handleGlobalError(error)
        return Promise.reject(error)
      }
    )
  }

  private handleGlobalError(error: AxiosError) {
    const { addNotification } = useNotificationStore.getState()
    
    if (error.response?.status === 401) {
      // Token expirado o inválido
      useAuthStore.getState().logout()
      window.location.href = '/auth/login'
      return
    }

    if (error.response?.status === 403) {
      addNotification({
        type: 'error',
        title: 'Access Denied',
        message: 'You don\'t have permission to perform this action'
      })
      return
    }

    if (error.response?.status >= 500) {
      addNotification({
        type: 'error',
        title: 'Server Error',
        message: 'Something went wrong. Please try again later.'
      })
      return
    }

    // Network errors
    if (!error.response) {
      addNotification({
        type: 'error',
        title: 'Network Error',
        message: 'Please check your internet connection'
      })
    }
  }

  // Métodos públicos
  public get = this.instance.get
  public post = this.instance.post
  public put = this.instance.put
  public patch = this.instance.patch
  public delete = this.instance.delete
}

export const apiClient = new ApiClient()
```

### Type Generation from OpenAPI

```bash
# Script para generar tipos TypeScript
npm install -D openapi-typescript

# package.json scripts
{
  "scripts": {
    "generate-types": "openapi-typescript ../docs/openapi/openapi.yaml --output src/shared/types/api.ts",
    "dev": "npm run generate-types && vite",
    "build": "npm run generate-types && tsc && vite build"
  }
}
```

```typescript
// src/shared/types/api.ts (generado automáticamente)
export interface paths {
  '/repositories': {
    get: operations['listRepositories']
    post: operations['createRepository']
  }
  '/repositories/{id}': {
    get: operations['getRepository']
    put: operations['updateRepository']
    delete: operations['deleteRepository']
  }
  // ... más endpoints
}

export interface components {
  schemas: {
    Repository: {
      id: string
      name: string
      type: 'maven' | 'npm' | 'pypi'
      description?: string
      // ... más propiedades
    }
    // ... más schemas
  }
}

// Helper types
export type Repository = components['schemas']['Repository']
export type CreateRepositoryRequest = components['schemas']['CreateRepositoryRequest']
export type RepositoryListResponse = components['schemas']['RepositoryListResponse']
```

## Service Layer Implementation

### Repository Service

```typescript
// src/features/repositories/services/repositoryApi.ts
import { apiClient } from '@/shared/api/client'
import type { 
  Repository, 
  CreateRepositoryRequest, 
  UpdateRepositoryRequest,
  RepositoryListResponse 
} from '@/shared/types/api'

export interface RepositoryFilters {
  type?: string
  status?: string
  search?: string
  limit?: number
  offset?: number
}

class RepositoryService {
  private readonly basePath = '/repositories'

  async list(filters: RepositoryFilters = {}): Promise<RepositoryListResponse> {
    const params = new URLSearchParams()
    
    Object.entries(filters).forEach(([key, value]) => {
      if (value !== undefined && value !== '') {
        params.append(key, String(value))
      }
    })

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

  async getArtifacts(id: string, filters: ArtifactFilters = {}) {
    const params = new URLSearchParams()
    Object.entries(filters).forEach(([key, value]) => {
      if (value !== undefined) {
        params.append(key, String(value))
      }
    })

    const response = await apiClient.get<ArtifactListResponse>(
      `${this.basePath}/${id}/artifacts?${params.toString()}`
    )
    return response.data
  }
}

export const repositoryService = new RepositoryService()
```

### Artifact Service

```typescript
// src/features/artifacts/services/artifactApi.ts
import { apiClient } from '@/shared/api/client'
import type { 
  Artifact, 
  ArtifactUploadResponse,
  PresignedUrlResponse 
} from '@/shared/types/api'

export interface UploadMetadata {
  repositoryId: string
  fileName: string
  checksum?: string
  metadata?: Record<string, any>
}

class ArtifactService {
  private readonly basePath = '/artifacts'

  async upload(
    file: File, 
    metadata: UploadMetadata,
    onProgress?: (progress: number) => void
  ): Promise<ArtifactUploadResponse> {
    const formData = new FormData()
    formData.append('file', file)
    formData.append('metadata', JSON.stringify(metadata))

    const response = await apiClient.post<ArtifactUploadResponse>(
      this.basePath,
      formData,
      {
        headers: {
          'Content-Type': 'multipart/form-data'
        },
        onUploadProgress: (progressEvent) => {
          if (progressEvent.total && onProgress) {
            const progress = (progressEvent.loaded / progressEvent.total) * 100
            onProgress(Math.round(progress))
          }
        }
      }
    )
    return response.data
  }

  async getById(id: string): Promise<Artifact> {
    const response = await apiClient.get<Artifact>(`${this.basePath}/${id}`)
    return response.data
  }

  async getDownloadUrl(id: string, presigned = false): Promise<string | PresignedUrlResponse> {
    if (presigned) {
      const response = await apiClient.get<PresignedUrlResponse>(
        `${this.basePath}/${id}?presigned=true`
      )
      return response.data
    } else {
      // Direct download
      return `${apiClient.defaults.baseURL}${this.basePath}/${id}`
    }
  }

  async delete(id: string): Promise<void> {
    await apiClient.delete(`${this.basePath}/${id}`)
  }

  async batchDelete(ids: string[]): Promise<void> {
    await apiClient.delete(this.basePath, {
      data: { ids }
    })
  }
}

export const artifactService = new ArtifactService()
```

## React Query Integration

### Query Client Setup

```typescript
// src/app/queryClient.ts
import { QueryClient } from '@tanstack/react-query'

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5 * 60 * 1000, // 5 minutos
      cacheTime: 10 * 60 * 1000, // 10 minutos
      retry: (failureCount, error: any) => {
        // No retry en errores 4xx
        if (error?.response?.status >= 400 && error?.response?.status < 500) {
          return false
        }
        return failureCount < 3
      },
      refetchOnWindowFocus: false
    },
    mutations: {
      retry: 1
    }
  }
})
```

### Custom Hooks for Repositories

```typescript
// src/features/repositories/hooks/useRepositories.ts
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { repositoryService, type RepositoryFilters } from '../services/repositoryApi'
import { useNotificationStore } from '@/shared/stores/notificationStore'

const REPOSITORIES_KEY = 'repositories'

export const useRepositories = (filters: RepositoryFilters = {}) => {
  return useQuery({
    queryKey: [REPOSITORIES_KEY, 'list', filters],
    queryFn: () => repositoryService.list(filters),
    keepPreviousData: true
  })
}

export const useRepository = (id: string) => {
  return useQuery({
    queryKey: [REPOSITORIES_KEY, 'detail', id],
    queryFn: () => repositoryService.getById(id),
    enabled: !!id
  })
}

export const useCreateRepository = () => {
  const queryClient = useQueryClient()
  const { addNotification } = useNotificationStore()

  return useMutation({
    mutationFn: repositoryService.create,
    onSuccess: (newRepository) => {
      // Invalidar cache de lista
      queryClient.invalidateQueries([REPOSITORIES_KEY, 'list'])
      
      // Optimistic update: agregar a cache existente
      queryClient.setQueryData(
        [REPOSITORIES_KEY, 'detail', newRepository.id],
        newRepository
      )

      addNotification({
        type: 'success',
        title: 'Repository Created',
        message: `Repository "${newRepository.name}" was created successfully`
      })
    },
    onError: (error: any) => {
      addNotification({
        type: 'error',
        title: 'Creation Failed',
        message: error.response?.data?.message || 'Failed to create repository'
      })
    }
  })
}

export const useUpdateRepository = () => {
  const queryClient = useQueryClient()
  const { addNotification } = useNotificationStore()

  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateRepositoryRequest }) =>
      repositoryService.update(id, data),
    onSuccess: (updatedRepository) => {
      // Update detail cache
      queryClient.setQueryData(
        [REPOSITORIES_KEY, 'detail', updatedRepository.id],
        updatedRepository
      )

      // Update in list cache
      queryClient.setQueriesData(
        [REPOSITORIES_KEY, 'list'],
        (oldData: any) => {
          if (!oldData?.repositories) return oldData
          
          return {
            ...oldData,
            repositories: oldData.repositories.map((repo: Repository) =>
              repo.id === updatedRepository.id ? updatedRepository : repo
            )
          }
        }
      )

      addNotification({
        type: 'success',
        title: 'Repository Updated',
        message: `Repository "${updatedRepository.name}" was updated successfully`
      })
    },
    onError: (error: any) => {
      addNotification({
        type: 'error',
        title: 'Update Failed',
        message: error.response?.data?.message || 'Failed to update repository'
      })
    }
  })
}

export const useDeleteRepository = () => {
  const queryClient = useQueryClient()
  const { addNotification } = useNotificationStore()

  return useMutation({
    mutationFn: repositoryService.delete,
    onSuccess: (_, deletedId) => {
      // Remove from list cache
      queryClient.setQueriesData(
        [REPOSITORIES_KEY, 'list'],
        (oldData: any) => {
          if (!oldData?.repositories) return oldData
          
          return {
            ...oldData,
            repositories: oldData.repositories.filter(
              (repo: Repository) => repo.id !== deletedId
            ),
            total: oldData.total - 1
          }
        }
      )

      // Remove detail cache
      queryClient.removeQueries([REPOSITORIES_KEY, 'detail', deletedId])

      addNotification({
        type: 'success',
        title: 'Repository Deleted',
        message: 'Repository was deleted successfully'
      })
    },
    onError: (error: any) => {
      addNotification({
        type: 'error',
        title: 'Deletion Failed',
        message: error.response?.data?.message || 'Failed to delete repository'
      })
    }
  })
}
```

### Custom Hooks for Artifacts

```typescript
// src/features/artifacts/hooks/useArtifacts.ts
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { artifactService, type UploadMetadata } from '../services/artifactApi'

const ARTIFACTS_KEY = 'artifacts'

export const useArtifactUpload = () => {
  const queryClient = useQueryClient()
  const { addNotification } = useNotificationStore()

  return useMutation({
    mutationFn: ({ 
      file, 
      metadata, 
      onProgress 
    }: { 
      file: File
      metadata: UploadMetadata
      onProgress?: (progress: number) => void 
    }) => artifactService.upload(file, metadata, onProgress),
    
    onSuccess: (result, { metadata }) => {
      // Invalidar cache de artefactos del repositorio
      queryClient.invalidateQueries([ARTIFACTS_KEY, 'repository', metadata.repositoryId])
      
      addNotification({
        type: 'success',
        title: 'Upload Complete',
        message: `${metadata.fileName} uploaded successfully`
      })
    },
    
    onError: (error: any, { metadata }) => {
      addNotification({
        type: 'error',
        title: 'Upload Failed',
        message: `Failed to upload ${metadata.fileName}: ${error.message}`
      })
    }
  })
}

export const useArtifactBatch = () => {
  const queryClient = useQueryClient()

  const batchDelete = useMutation({
    mutationFn: artifactService.batchDelete,
    onSuccess: () => {
      // Invalidar todas las queries de artefactos
      queryClient.invalidateQueries([ARTIFACTS_KEY])
    }
  })

  return { batchDelete }
}
```

## Error Handling Patterns

### Custom Error Types

```typescript
// src/shared/types/errors.ts
export interface ApiError {
  code: string
  message: string
  details?: Record<string, any>
}

export interface ValidationError {
  field: string
  message: string
}

export class AppError extends Error {
  constructor(
    public code: string,
    message: string,
    public statusCode?: number,
    public validationErrors?: ValidationError[]
  ) {
    super(message)
    this.name = 'AppError'
  }
}

export const isApiError = (error: any): error is ApiError => {
  return error && typeof error === 'object' && 'code' in error && 'message' in error
}
```

### Error Boundary Component

```typescript
// src/shared/components/ErrorBoundary.tsx
import React, { Component, ErrorInfo, ReactNode } from 'react'
import { Button } from '@/shared/components/atoms/Button'
import { Card } from '@/shared/components/molecules/Card'

interface Props {
  children: ReactNode
  fallback?: ReactNode
}

interface State {
  hasError: boolean
  error?: Error
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props)
    this.state = { hasError: false }
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Error caught by boundary:', error, errorInfo)
    
    // Enviar error a servicio de monitoreo
    if (import.meta.env.PROD) {
      // Sentry.captureException(error, { extra: errorInfo })
    }
  }

  handleReset = () => {
    this.setState({ hasError: false, error: undefined })
  }

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) {
        return this.props.fallback
      }

      return (
        <div className="flex items-center justify-center min-h-screen p-4">
          <Card className="max-w-md">
            <Card.Header>
              <h2 className="text-lg font-semibold text-red-600">
                Something went wrong
              </h2>
            </Card.Header>
            <Card.Body>
              <p className="text-gray-600 mb-4">
                An unexpected error occurred. Please try refreshing the page.
              </p>
              {import.meta.env.DEV && this.state.error && (
                <details className="mt-4">
                  <summary className="cursor-pointer text-sm text-gray-500">
                    Error details
                  </summary>
                  <pre className="mt-2 text-xs bg-gray-100 p-2 rounded overflow-auto">
                    {this.state.error.stack}
                  </pre>
                </details>
              )}
            </Card.Body>
            <Card.Footer>
              <Button onClick={this.handleReset} className="w-full">
                Try Again
              </Button>
            </Card.Footer>
          </Card>
        </div>
      )
    }

    return this.props.children
  }
}
```

### Query Error Handling

```typescript
// src/shared/hooks/useErrorHandler.ts
import { useCallback } from 'react'
import { useNotificationStore } from '@/shared/stores/notificationStore'
import { AppError, isApiError } from '@/shared/types/errors'

export const useErrorHandler = () => {
  const { addNotification } = useNotificationStore()

  const handleError = useCallback((error: unknown, context?: string) => {
    console.error(`Error in ${context}:`, error)

    let title = 'Error'
    let message = 'An unexpected error occurred'

    if (error instanceof AppError) {
      title = `${context} Error`
      message = error.message
    } else if (isApiError(error)) {
      title = `API Error`
      message = error.message
    } else if (error instanceof Error) {
      message = error.message
    }

    addNotification({
      type: 'error',
      title,
      message
    })
  }, [addNotification])

  return { handleError }
}
```

## Authentication Integration

### Auth Store with API Integration

```typescript
// src/shared/stores/authStore.ts
import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { authService } from '@/features/auth/services/authApi'
import type { User, LoginCredentials } from '@/shared/types/api'

interface AuthState {
  user: User | null
  token: string | null
  isAuthenticated: boolean
  isLoading: boolean
  login: (credentials: LoginCredentials) => Promise<void>
  logout: () => void
  refreshToken: () => Promise<void>
  checkAuth: () => Promise<void>
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      user: null,
      token: null,
      isAuthenticated: false,
      isLoading: false,

      login: async (credentials) => {
        try {
          set({ isLoading: true })
          const response = await authService.login(credentials)
          
          set({
            user: response.user,
            token: response.token,
            isAuthenticated: true,
            isLoading: false
          })
        } catch (error) {
          set({ isLoading: false })
          throw error
        }
      },

      logout: () => {
        set({
          user: null,
          token: null,
          isAuthenticated: false
        })
        
        // Clear all stored data
        localStorage.removeItem('auth-store')
        
        // Redirect to login
        window.location.href = '/auth/login'
      },

      refreshToken: async () => {
        try {
          const currentToken = get().token
          if (!currentToken) throw new Error('No token available')

          const response = await authService.refreshToken(currentToken)
          
          set({
            token: response.token,
            user: response.user
          })
        } catch (error) {
          get().logout()
          throw error
        }
      },

      checkAuth: async () => {
        try {
          const token = get().token
          if (!token) return

          const user = await authService.getCurrentUser()
          set({ user, isAuthenticated: true })
        } catch (error) {
          get().logout()
        }
      }
    }),
    {
      name: 'auth-store',
      partialize: (state) => ({
        user: state.user,
        token: state.token,
        isAuthenticated: state.isAuthenticated
      })
    }
  )
)
```

## Environment Configuration

### Environment Variables

```typescript
// src/shared/config/env.ts
interface EnvironmentConfig {
  API_URL: string
  NODE_ENV: 'development' | 'production' | 'test'
  DEBUG: boolean
  SENTRY_DSN?: string
}

const validateEnv = (): EnvironmentConfig => {
  const config = {
    API_URL: import.meta.env.VITE_API_URL || 'http://localhost:8080/v2',
    NODE_ENV: import.meta.env.NODE_ENV as 'development' | 'production' | 'test',
    DEBUG: import.meta.env.VITE_DEBUG === 'true',
    SENTRY_DSN: import.meta.env.VITE_SENTRY_DSN
  }

  // Validations
  if (!config.API_URL) {
    throw new Error('VITE_API_URL is required')
  }

  return config
}

export const env = validateEnv()
```

Esta arquitectura de integración proporciona una base sólida para comunicarse con el backend de manera type-safe, eficiente y robusta, con manejo de errores consistente y caching inteligente.