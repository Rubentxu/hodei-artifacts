/**
 * Servicio de dominio para gestión de repositorios
 * Sigue principios SOLID y Clean Code
 */

import type { 
  Repository, 
  RepositoryFilters, 
  PaginatedResponse,
  CreateRepositoryRequest,
  UpdateRepositoryRequest 
} from '@/shared/types';
import type { RepositoryPort } from './ports/RepositoryPort';

/**
 * Servicio de aplicación para operaciones de repositorio
 * Implementa la lógica de negocio específica del dominio
 */
export class RepositoryService {
  constructor(private readonly repositoryPort: RepositoryPort) {}

  /**
   * Obtiene lista paginada de repositorios con filtros aplicados
   */
  async obtenerRepositoriosPaginados(
    filtros: RepositoryFilters
  ): Promise<PaginatedResponse<Repository>> {
    try {
      return await this.repositoryPort.buscarRepositorios(filtros);
    } catch (error) {
      console.error('Error al obtener repositorios:', error);
      throw new Error('No se pudieron cargar los repositorios');
    }
  }

  /**
   * Obtiene un repositorio específico por su ID
   */
  async obtenerRepositorioPorId(id: string): Promise<Repository> {
    if (!id) {
      throw new Error('ID de repositorio requerido');
    }

    try {
      return await this.repositoryPort.obtenerRepositorio(id);
    } catch (error) {
      console.error(`Error al obtener repositorio ${id}:`, error);
      throw new Error('Repositorio no encontrado');
    }
  }

  /**
   * Crea un nuevo repositorio con validaciones de negocio
   */
  async crearRepositorio(
    datos: CreateRepositoryRequest
  ): Promise<Repository> {
    this.validarDatosRepositorio(datos);
    
    try {
      return await this.repositoryPort.crearRepositorio(datos);
    } catch (error) {
      console.error('Error al crear repositorio:', error);
      throw new Error('No se pudo crear el repositorio');
    }
  }

  /**
   * Actualiza un repositorio existente
   */
  async actualizarRepositorio(
    id: string,
    datos: UpdateRepositoryRequest
  ): Promise<Repository> {
    if (!id) {
      throw new Error('ID de repositorio requerido');
    }

    this.validarActualizacionRepositorio(datos);
    
    try {
      return await this.repositoryPort.actualizarRepositorio(id, datos);
    } catch (error) {
      console.error(`Error al actualizar repositorio ${id}:`, error);
      throw new Error('No se pudo actualizar el repositorio');
    }
  }

  /**
   * Elimina un repositorio por su ID
   */
  async eliminarRepositorio(id: string): Promise<void> {
    if (!id) {
      throw new Error('ID de repositorio requerido');
    }

    try {
      await this.repositoryPort.eliminarRepositorio(id);
    } catch (error) {
      console.error(`Error al eliminar repositorio ${id}:`, error);
      throw new Error('No se pudo eliminar el repositorio');
    }
  }

  /**
   * Obtiene repositorios por tipo (maven, npm, pypi, docker)
   */
  async obtenerRepositoriosPorTipo(
    tipo: Repository['type']
  ): Promise<Repository[]> {
    if (!tipo) {
      throw new Error('Tipo de repositorio requerido');
    }

    try {
      const resultado = await this.repositoryPort.buscarRepositorios({
        type: [tipo],
        limit: 100 // Límite razonable para obtener todos los de un tipo
      });

      return resultado.data;
    } catch (error) {
      console.error(`Error al obtener repositorios de tipo ${tipo}:`, error);
      throw new Error(`No se pudieron cargar los repositorios ${tipo}`);
    }
  }

  /**
   * Obtiene métricas de uso de repositorios
   */
  async obtenerMetricasRepositorios(): Promise<{
    total: number;
    porTipo: Record<Repository['type'], number>;
    activos: number;
    inactivos: number;
  }> {
    try {
      // Obtener todos los repositorios para calcular métricas
      const todosRepositorios = await this.repositoryPort.buscarRepositorios({
        limit: 1000 // Límite alto para obtener todos
      });

      const repositorios = todosRepositorios.data;
      
      const metricas = {
        total: repositorios.length,
        porTipo: {
          maven: 0,
          npm: 0,
          pypi: 0,
          docker: 0
        },
        activos: 0,
        inactivos: 0
      };

      repositorios.forEach(repo => {
        // Contar por tipo
        if (repo.type in metricas.porTipo) {
          metricas.porTipo[repo.type]++;
        }
        
        // Contar activos/inactivos basado en la fecha de actualización
        const diasDesdeActualizacion = Math.floor(
          (Date.now() - new Date(repo.lastUpdated).getTime()) / (1000 * 60 * 60 * 24)
        );
        
        if (diasDesdeActualizacion < 30) {
          metricas.activos++;
        } else {
          metricas.inactivos++;
        }
      });

      return metricas;
    } catch (error) {
      console.error('Error al obtener métricas:', error);
      throw new Error('No se pudieron calcular las métricas');
    }
  }

  // ===== MÉTODOS PRIVADOS =====

  /**
   * Valida los datos para crear un repositorio
   */
  private validarDatosRepositorio(datos: CreateRepositoryRequest): void {
    if (!datos.name || datos.name.trim().length === 0) {
      throw new Error('El nombre del repositorio es requerido');
    }

    if (datos.name.length < 3) {
      throw new Error('El nombre debe tener al menos 3 caracteres');
    }

    if (datos.name.length > 50) {
      throw new Error('El nombre no puede exceder 50 caracteres');
    }

    if (!/^[a-zA-Z0-9-_]+$/.test(datos.name)) {
      throw new Error('El nombre solo puede contener letras, números, guiones y guiones bajos');
    }

    if (!datos.type) {
      throw new Error('El tipo de repositorio es requerido');
    }

    const tiposValidos: Repository['type'][] = ['maven', 'npm', 'pypi', 'docker'];
    if (!tiposValidos.includes(datos.type)) {
      throw new Error(`Tipo de repositorio inválido. Debe ser uno de: ${tiposValidos.join(', ')}`);
    }
  }

  /**
   * Valida los datos para actualizar un repositorio
   */
  private validarActualizacionRepositorio(datos: UpdateRepositoryRequest): void {
    if (datos.name !== undefined) {
      if (datos.name.trim().length === 0) {
        throw new Error('El nombre del repositorio no puede estar vacío');
      }

      if (datos.name.length < 3) {
        throw new Error('El nombre debe tener al menos 3 caracteres');
      }

      if (datos.name.length > 50) {
        throw new Error('El nombre no puede exceder 50 caracteres');
      }

      if (!/^[a-zA-Z0-9-_]+$/.test(datos.name)) {
        throw new Error('El nombre solo puede contener letras, números, guiones y guiones bajos');
      }
    }
  }
}

/**
 * Interfaz para el puerto de repositorio (adaptador)
 * Define el contrato que deben implementar los adaptadores de datos
 */
export interface RepositoryPort {
  buscarRepositorios(filtros: RepositoryFilters): Promise<PaginatedResponse<Repository>>;
  obtenerRepositorio(id: string): Promise<Repository>;
  crearRepositorio(datos: CreateRepositoryRequest): Promise<Repository>;
  actualizarRepositorio(id: string, datos: UpdateRepositoryRequest): Promise<Repository>;
  eliminarRepositorio(id: string): Promise<void>;
}