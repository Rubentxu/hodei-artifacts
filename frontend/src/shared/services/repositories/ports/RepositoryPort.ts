/**
 * Puerto (interfaz) para el adaptador de repositorios
 * Define el contrato que deben implementar los adaptadores de datos
 * Siguiendo el principio de Inversión de Dependencias (DIP) de SOLID
 */

import type {
  Repository,
  RepositoryFilters,
  PaginatedResponse,
  CreateRepositoryRequest,
  UpdateRepositoryRequest,
} from '@/shared/types';

/**
 * Puerto que define las operaciones de acceso a datos para repositorios
 * Los adaptadores (HTTP, Mock, etc.) deben implementar esta interfaz
 */
export interface RepositoryPort {
  /**
   * Busca repositorios con filtros aplicados
   */
  buscarRepositorios(
    filtros: RepositoryFilters
  ): Promise<PaginatedResponse<Repository>>;

  /**
   * Obtiene un repositorio específico por su ID
   */
  obtenerRepositorio(id: string): Promise<Repository>;

  /**
   * Crea un nuevo repositorio
   */
  crearRepositorio(datos: CreateRepositoryRequest): Promise<Repository>;

  /**
   * Actualiza un repositorio existente
   */
  actualizarRepositorio(
    id: string,
    datos: UpdateRepositoryRequest
  ): Promise<Repository>;

  /**
   * Elimina un repositorio por su ID
   */
  eliminarRepositorio(id: string): Promise<void>;
}
