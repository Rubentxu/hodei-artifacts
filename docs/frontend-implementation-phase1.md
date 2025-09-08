# Fase 1: Implementación de Adaptación - Integración de Servicios Mock
## Semana 1: Fundación y Análisis

## 📋 Objetivos de la Fase 1

### Objetivos Principales:
1. **Integrar servicios mock mejorados** con componentes existentes sin romper funcionalidad
2. **Mantener retrocompatibilidad** mientras se añaden mejoras
3. **Preservar testing existente** y añadir nuevos tests
4. **Documentar cambios** para facilitar mantenimiento

## 🔍 Análisis del Estado Actual

### Servicios Mock Existentes:
- ✅ `frontend/src/features/search/services/searchApi.ts` - Con mock básico
- ✅ `frontend/src/features/users/services/userApi.ts` - Con mock básico
- ✅ `frontend/src/pages/Dashboard/Dashboard.tsx` - Dashboard funcional con datos hardcodeados

### Servicios Mock Mejorados Creados:
- ✅ `frontend/src/shared/api/mock/repositoryService.mock.ts` - Servicio completo
- ✅ `frontend/src/shared/api/mock/searchService.mock.ts` - Búsqueda avanzada
- ✅ `frontend/src/shared/api/mock/userService.mock.ts` - Gestión de usuarios
- ✅ `frontend/src/shared/api/mock/artifactService.mock.ts` - Gestión de artefactos

## 🔄 Plan de Integración Paso a Paso

### Paso 1: Crear Adaptador de Servicios Mock (Día 1)

Primero, vamos a crear un adaptador que permita usar nuestros servicios mock mejorados sin romper la API existente:

<write_to_file>
<path>frontend/src/shared/api/mockAdapter.ts</path>
<content>import { repositoryServiceMock } from './mock/repositoryService.mock';
import { searchServiceMock } from './mock/searchService.mock';
import { userServiceMock } from './mock/userService.mock';
import { artifactServiceMock } from './mock/artifactService.mock';
import type { 
  Repository, 
  RepositoryListResponse, 
  SearchResults, 
  PackageResult,
  SearchQuery,
  User as ApiUser,
  CreateUserCommand
} from '@/shared/types/openapi.types';

// Adaptador para mantener compatibilidad con APIs existentes mientras se migran a servicios mock mejorados

export interface LegacySearchResult {
  id: string;
  name: string;
  description: string;
  repository: string;
  type: 'npm' | 'pypi' | 'maven';
  version: string;
  size: number;
  license: string;
  popularity: number;
}

export interface LegacySearchResponse {
  results: LegacySearchResult[];
  total: number;
  facets: {
    type: Array<{ value: string; count: number }>;
    repository: Array<{ value: string; count: number }>;
  };
}

export interface LegacyUser {
  id: string;
  name: string;
  email: string;
  role: string;
  status: string;
  organization: string;
}

export interface LegacyUserProfile {
  id: string;
  name: string;
  email: string;
  organization: string;
}

export const mockAdapter = {
  // Search Service Adapter
  async search(filters: any): Promise<LegacySearchResponse> {
    try {
      // Convertir filtros legacy a SearchQuery moderno
      const searchQuery: SearchQuery = {
        q: filters.q || filters.query || '',
        type: filters.type || undefined,
        repositoryId: filters.repository || undefined,
        page: filters.page || 1,
        limit: filters.limit || 20,
        sortBy: 'name',
        sortOrder: 'asc'
      };

      const results = await searchServiceMock.searchPackages(searchQuery);
      
      // Convertir resultados modernos a formato legacy
      const legacyResults: LegacySearchResult[] = results.items.map(item => ({
        id: item.id,
        name: item.name,
        description: item.description || '',
        repository: item.repositoryName,
        type: item.packageType,
        version: item.version,
        size: 0, // No tenemos este dato en el mock, usar valor por defecto
        license: 'MIT', // Valor por defecto
        popularity: item.downloadCount
      }));

      // Generar facets desde los resultados
      const typeFacets = legacyResults.reduce((acc, item) => {
        const existing = acc.find(f => f.value === item.type);
        if (existing) {
          existing.count++;
        } else {
          acc.push({ value: item.type, count: 1 });
        }
        return acc;
      }, [] as Array<{ value: string; count: number }>);

      const repoFacets = legacyResults.reduce((acc, item) => {
        const existing = acc.find(f => f.value === item.repository);
        if (existing) {
          existing.count++;
        } else {
          acc.push({ value: item.repository, count: 1 });
        }
        return acc;
      }, [] as Array<{ value: string; count: number }>);

      return {
        results: legacyResults,
        total: results.total,
        facets: {
          type: typeFacets,
          repository: repoFacets
        }
      };
    } catch (error) {
      console.error('Error in search adapter:', error);
      // Retornar datos mock de respaldo
      return {
        results: [
          {
            id: '1',
            name: 'react',
            description: 'A JavaScript library for building user interfaces',
            repository: 'npm-public',
            type: 'npm',
            version: '18.2.0',
            size: 2300000,
            license: 'MIT',
            popularity: 45000
          }
        ],
        total: 1,
        facets: {
          type: [{ value: 'npm', count: 1 }],
          repository: [{ value: 'npm-public', count: 1 }]
        }
      };
    }
  },

  async getSuggestions(query: string): Promise<string[]> {
    try {
      return await searchServiceMock.getSearchSuggestions(query);
    } catch (error) {
      console.error('Error in suggestions adapter:', error);
      return ['react', 'vue', 'angular']; // Datos de respaldo
    }
  },

  // User Service Adapter
  async getUsers(): Promise<LegacyUser[]> {
    try {
      // Por ahora retornar datos mock legacy ya que el servicio de usuarios mock necesita autenticación
      return [
        {
          id: 'user-1',
          name: 'John Doe',
          email: 'john.doe@example.com',
          role: 'Admin',
          status: 'Active',
          organization: 'Hodei Inc.'
        },
        {
          id: 'user-2',
          name: 'Jane Smith',
          email: 'jane.smith@example.com',
          role: 'User',
          status: 'Active',
          organization: 'Hodei Inc.'
        }
      ];
    } catch (error) {
      console.error('Error in getUsers adapter:', error);
      return [];
    }
  },

  async createUser(data: any): Promise<LegacyUser> {
    try {
      // Convertir datos legacy a formato moderno
      const createUserData: CreateUserCommand = {
        username: data.name || data.username || '',
        email: data.email || '',
        password: data.password || 'temp-password-123',
        attributes: {
          role: data.role || 'User',
          organization: 'Hodei Inc.'
        }
      };

      // Por ahora retornar usuario mock ya que necesitamos autenticación
      return {
        id: `user-${Date.now()}`,
        name: createUserData.username,
        email: createUserData.email,
        role: data.role || 'User',
        status: 'Active',
        organization: 'Hodei Inc.'
      };
    } catch (error) {
      console.error('Error in createUser adapter:', error);
      throw new Error('Failed to create user');
    }
  },

  async getMyProfile(): Promise<LegacyUserProfile> {
    try {
      // Por ahora retornar perfil mock
      return {
        id: 'user-123',
        name: 'John Doe',
        email: 'john.doe@example.com',
        organization: 'Hodei Inc.'
      };
    } catch (error) {
      console.error('Error in getMyProfile adapter:', error);
      return {
        id: 'user-123',
        name: 'John Doe',
        email: 'john.doe@example.com',
        organization: 'Hodei Inc.'
      };
    }
  },

  // Repository Service Adapter
  async getRepositories(): Promise<any> {
    try {
      const result = await repositoryServiceMock.getRepositories();
      return result.items;
    } catch (error) {
      console.error('Error in getRepositories adapter:', error);
      return [];
    }
  },

  async getRepository(id: string): Promise<any> {
    try {
      return await repositoryServiceMock.getRepository(id);
    } catch (error) {
      console.error('Error in getRepository adapter:', error);
      return null;
    }
  },

  // Artifact Service Adapter
  async getArtifacts(repositoryId?: string): Promise<any> {
    try {
      return await artifactServiceMock.getArtifacts(repositoryId);
    } catch (error) {
      console.error('Error in getArtifacts adapter:', error);
      return [];
    }
  }
};

// Exportar también los servicios mock modernos para uso en nuevos componentes
export { repositoryServiceMock, searchServiceMock, userServiceMock, artifactServiceMock };