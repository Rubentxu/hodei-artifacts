/**
 * Mapper para transformar entre el dominio de la aplicación y el contrato OpenAPI
 * Sigue el patrón Mapper para mantener separación de responsabilidades
 */

import type { 
  Repository, 
  RepositoryFilters, 
  PaginatedResponse,
  CreateRepositoryRequest,
  UpdateRepositoryRequest 
} from '@/shared/types';
import type {
  Repository as OpenAPIRepository,
  RepositoryListResponse,
  ListRepositoriesParams,
  CreateRepositoryBody,
  UpdateRepositoryBody
} from '@/shared/types/openapi-generated.types';

/**
 * Mapper para transformar entre el dominio de la aplicación y el contrato OpenAPI
 */
export class RepositoryMapper {
  /**
   * Convierte filtros del dominio al formato OpenAPI
   */
  filtrosADominioOpenAPI(filtros: RepositoryFilters): ListRepositoriesParams {
    return {
      limit: filtros.limit || 10,
      offset: filtros.page ? (filtros.page - 1) * (filtros.limit || 10) : 0
    };
  }

  /**
   * Convierte una respuesta OpenAPI al formato del dominio
   */
  respuestaOpenAPIADominio(respuesta: RepositoryListResponse): PaginatedResponse<Repository> {
    const items = respuesta.items || [];
    const total = respuesta.total || 0;
    const limit = 10; // Valor por defecto ya que no viene en la respuesta
    
    return {
      data: items.map(repo => this.repositorioOpenAPIADominio(repo)),
      total: total,
      page: 1, // Valor por defecto
      limit: limit,
      hasNext: items.length < total,
      hasPrev: false
    };
  }

  /**
   * Convierte un repositorio OpenAPI al formato del dominio
   */
  repositorioOpenAPIADominio(repo: OpenAPIRepository): Repository {
    return {
      id: repo.id,
      name: repo.name,
      description: repo.description,
      type: 'maven', // Valor por defecto ya que el tipo no está en el OpenAPI
      visibility: 'public', // Valor por defecto
      isPublic: true, // Valor por defecto
      packageCount: 0, // Valor por defecto
      size: 0, // Valor por defecto
      lastUpdated: repo.createdAt,
      url: `https://api.repo-manager.com/v2/repositories/${repo.id}` // URL por defecto
    };
  }

  /**
   * Convierte datos de creación del dominio al formato OpenAPI
   */
  solicitudCreacionADominioOpenAPI(datos: CreateRepositoryRequest): CreateRepositoryBody {
    return {
      name: datos.name,
      description: datos.description
    };
  }

  /**
   * Convierte datos de actualización del dominio al formato OpenAPI
   */
  solicitudActualizacionADominioOpenAPI(datos: UpdateRepositoryRequest): UpdateRepositoryBody {
    return {
      name: datos.name,
      description: datos.description
    };
  }

  /**
   * Convierte una respuesta única de OpenAPI al formato del dominio
   */
  repositorioUnicoOpenAPIADominio(repo: OpenAPIRepository): Repository {
    return this.repositorioOpenAPIADominio(repo);
  }
}