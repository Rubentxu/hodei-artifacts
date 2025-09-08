# Plan de Ingeniería Inversa - Frontend Hodei Artifacts
## Inspiración: JFrog Artifactory & Productos Comerciales de Gestión de Artefactos

## 📋 Objetivo
Transformar el frontend actual de Hodei Artifacts en una interfaz profesional, moderna y funcional, basándonos en productos comerciales líderes como JFrog Artifactory, GitHub Packages y Azure Artifacts, mockeando todos los servicios de backend según la especificación OpenAPI.

## 🔍 Análisis de la Especificación OpenAPI

### Endpoints Principales Identificados:
- **Repositorios**: `/v1/repositories`, `/v1/repositories/{id}`
- **Artefactos**: `/v1/artifacts`, `/v1/artifacts/{id}`
- **Búsqueda**: `/v1/search`
- **Usuarios**: `/v1/users`, `/v1/users/{id}/attributes`
- **Políticas**: `/v1/policies`
- **Autenticación**: `/auth/tokens`, `/auth/tokens/{tokenId}`
- **Ecosistemas**: 
  - Maven: `/maven2/{groupId}/{artifactId}/{version}/{fileName}`
  - npm: `/{packageName}`, `/{packageName}/-/{fileName}`
  - PyPI: `/simple/{packageName}/`, `/packages/{fileName}`, `/packages`

### Esquemas de Datos Principales:
- **Repository**: `{id, name, description, createdAt}`
- **ArtifactUploadResponse**: `{id, status, repositoryId}`
- **User**: `{username, email, password, attributes}`
- **SearchResults**: Resultados de búsqueda
- **Token**: Gestión de autenticación

## 🎯 Plan de Implementación por Fases

### FASE 1: Infraestructura y Mock Services (Semana 1)

#### 1.1 Servicios Mock Basados en OpenAPI

<write_to_file>
<path>frontend/src/shared/api/mock/repositoryService.mock.ts</path>
<content>import type { Repository, RepositoryListResponse, CreateRepositoryRequest } from '@/shared/types/openapi.types';

// Mock data basado en el esquema OpenAPI
const mockRepositories: Repository[] = [
  {
    id: '550e8400-e29b-41d4-a716-446655440001',
    name: 'maven-central',
    description: 'Repositorio Maven central para dependencias Java',
    createdAt: '2024-01-15T10:30:00Z'
  },
  {
    id: '550e8400-e29b-41d4-a716-446655440002',
    name: 'npm-public',
    description: 'Repositorio npm público para paquetes JavaScript',
    createdAt: '2024-01-16T14:20:00Z'
  },
  {
    id: '550e8400-e29b-41d4-a716-446655440003',
    name: 'pypi-internal',
    description: 'Repositorio PyPI interno para paquetes Python',
    createdAt: '2024-01-17T09:15:00Z'
  },
  {
    id: '550e8400-e29b-41d4-a716-446655440004',
    name: 'docker-registry',
    description: 'Registro Docker para imágenes de contenedores',
    createdAt: '2024-01-18T16:45:00Z'
  }
];

export const repositoryServiceMock = {
  async getRepositories(): Promise<RepositoryListResponse> {
    await new Promise(resolve => setTimeout(resolve, 500)); // Simular delay de red
    return {
      total: mockRepositories.length,
      items: mockRepositories
    };
  },

  async getRepository(id: string): Promise<Repository | null> {
    await new Promise(resolve => setTimeout(resolve, 300));
    return mockRepositories.find(repo => repo.id === id) || null;
  },

  async createRepository(data: CreateRepositoryRequest): Promise<Repository> {
    await new Promise(resolve => setTimeout(resolve, 800));
    const newRepository: Repository = {
      id: `550e8400-e29b-41d4-a716-4466554400${mockRepositories.length + 1}`,
      name: data.name,
      description: data.description || '',
      createdAt: new Date().toISOString()
    };
    mockRepositories.push(newRepository);
    return newRepository;
  },

  async updateRepository(id: string, data: Partial<Repository>): Promise<Repository> {
    await new Promise(resolve => setTimeout(resolve, 600));
    const index = mockRepositories.findIndex(repo => repo.id === id);
    if (index === -1) throw new Error('Repository not found');
    
    mockRepositories[index] = { ...mockRepositories[index], ...data };
    return mockRepositories[index];
  },

  async deleteRepository(id: string): Promise<void> {
    await new Promise(resolve => setTimeout(resolve, 400));
    const index = mockRepositories.findIndex(repo => repo.id === id);
    if (index === -1) throw new Error('Repository not found');
    
    mockRepositories.splice(index, 1);
  }
};