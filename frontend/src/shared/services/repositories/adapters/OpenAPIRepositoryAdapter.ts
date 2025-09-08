/**
 * Adaptador de repositorio que implementa el puerto RepositoryPort
 * Utiliza el cliente OpenAPI para comunicación con el backend
 * Sigue el patrón Adapter (Puerto y Adaptador)
 */

import type { RepositoryPort } from '../ports/RepositoryPort';
import type { 
  Repository, 
  RepositoryFilters, 
  PaginatedResponse,
  CreateRepositoryRequest,
  UpdateRepositoryRequest 
} from '@/shared/types';
import { openAPIClient } from '@/shared/api/openapi-client';
import { RepositoryMapper } from '../mappers/RepositoryMapper';

/**
 * Adaptador que implementa RepositoryPort usando el cliente OpenAPI
 * Traduce entre el dominio de la aplicación y el contrato OpenAPI
 */
export class OpenAPIRepositoryAdapter implements RepositoryPort {
  private readonly mapper: RepositoryMapper;

  constructor() {
    this.mapper = new RepositoryMapper();
  }

  /**
   * Busca repositorios con filtros aplicados
   */
  async buscarRepositorios(filtros: RepositoryFilters): Promise<PaginatedResponse<Repository>> {
    try {
      // Convertir filtros del dominio al formato OpenAPI
      const queryParams = this.mapper.filtrosADominioOpenAPI(filtros);
      
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.listRepositories(queryParams);
      
      // Mapear la respuesta al formato del dominio
      return this.mapper.respuestaOpenAPIADominio(resultado);
    } catch (error) {
      console.error('Error al buscar repositorios:', error);
      throw new Error('Error al buscar repositorios');
    }
  }

  /**
   * Obtiene un repositorio específico por su ID
   */
  async obtenerRepositorio(id: string): Promise<Repository> {
    try {
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.getRepository({ id });
      
      if (!resultado) {
        throw new Error('Repositorio no encontrado');
      }
      
      // Mapear la respuesta al formato del dominio
      return this.mapper.repositorioOpenAPIADominio(resultado);
    } catch (error) {
      console.error(`Error al obtener repositorio ${id}:`, error);
      throw new Error('Error al obtener el repositorio');
    }
  }

  /**
   * Crea un nuevo repositorio
   */
  async crearRepositorio(datos: CreateRepositoryRequest): Promise<Repository> {
    try {
      // Convertir datos del dominio al formato OpenAPI
      const datosOpenAPI = this.mapper.solicitudCreacionADominioOpenAPI(datos);
      
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.createRepository(datosOpenAPI);
      
      // Mapear la respuesta al formato del dominio
      return this.mapper.repositorioOpenAPIADominio(resultado);
    } catch (error) {
      console.error('Error al crear repositorio:', error);
      throw new Error('Error al crear el repositorio');
    }
  }

  /**
   * Actualiza un repositorio existente
   */
  async actualizarRepositorio(
    id: string, 
    datos: UpdateRepositoryRequest
  ): Promise<Repository> {
    try {
      // Convertir datos del dominio al formato OpenAPI
      const datosOpenAPI = this.mapper.solicitudActualizacionADominioOpenAPI(datos);
      
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.updateRepository({ id }, datosOpenAPI);
      
      // Mapear la respuesta al formato del dominio
      return this.mapper.repositorioOpenAPIADominio(resultado);
    } catch (error) {
      console.error(`Error al actualizar repositorio ${id}:`, error);
      throw new Error('Error al actualizar el repositorio');
    }
  }

  /**
   * Elimina un repositorio por su ID
   */
  async eliminarRepositorio(id: string): Promise<void> {
    try {
      await openAPIClient.deleteRepository({ id });
    } catch (error) {
      console.error(`Error al eliminar repositorio ${id}:`, error);
      throw new Error('Error al eliminar el repositorio');
    }
  }
}