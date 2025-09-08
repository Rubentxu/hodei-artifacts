/**
 * Servicio de dominio para búsqueda de artefactos y paquetes
 * Sigue principios SOLID y Clean Code
 */

import type {
  SearchResults,
  PackageResult,
  SearchArtifactsParams,
} from '@/shared/types/openapi-generated.types';
import type { SearchPort } from './ports/SearchPort.js';

/**
 * Servicio de aplicación para operaciones de búsqueda
 * Implementa la lógica de negocio específica del dominio
 */
export class SearchService {
  constructor(searchPort: SearchPort) {
    this.searchPort = searchPort;
  }

  private readonly searchPort: SearchPort;

  /**
   * Busca paquetes/artefactos por query con filtros opcionales
   */
  async buscarPaquetes(
    query: string,
    opciones?: {
      limite?: number;
      offset?: number;
      tipo?: 'maven' | 'npm' | 'pypi';
    }
  ): Promise<SearchResults> {
    if (!query || query.trim().length === 0) {
      throw new Error('La consulta de búsqueda es requerida');
    }

    if (query.length < 2) {
      throw new Error('La consulta debe tener al menos 2 caracteres');
    }

    try {
      const params: SearchArtifactsParams = {
        q: query.trim(),
        limit: opciones?.limite || 20,
        offset: opciones?.offset || 0,
      };

      return await this.searchPort.buscarArtifatos(params);
    } catch (error) {
      console.error('Error al buscar paquetes:', error);
      throw new Error('No se pudieron buscar los paquetes');
    }
  }

  /**
   * Obtiene sugerencias de búsqueda basadas en una query parcial
   */
  async obtenerSugerencias(query: string): Promise<string[]> {
    if (!query || query.trim().length === 0) {
      return [];
    }

    if (query.length < 2) {
      return [];
    }

    try {
      // Por ahora, generar sugerencias basadas en la query
      // En el futuro, esto puede conectarse a un servicio de sugerencias real
      const sugerencias = this.generarSugerenciasMock(query);
      return sugerencias;
    } catch (error) {
      console.error('Error al obtener sugerencias:', error);
      return [];
    }
  }

  /**
   * Busca paquetes populares (más descargados)
   */
  async obtenerPaquetesPopulares(
    limite: number = 10,
    tipo?: 'maven' | 'npm' | 'pypi'
  ): Promise<PackageResult[]> {
    if (limite < 1 || limite > 50) {
      throw new Error('El límite debe estar entre 1 y 50');
    }

    try {
      // Buscar con query vacía para obtener paquetes populares
      const resultados = await this.buscarPaquetes('', {
        limite: limite * 3, // Buscar más para filtrar
        tipo,
      });

      // Filtrar y ordenar por descargas
      const paquetesPopulares = (resultados.results || [])
        .filter(pkg => pkg.downloads && pkg.downloads > 0)
        .sort((a, b) => (b.downloads || 0) - (a.downloads || 0))
        .slice(0, limite);

      return paquetesPopulares;
    } catch (error) {
      console.error('Error al obtener paquetes populares:', error);
      throw new Error('No se pudieron obtener los paquetes populares');
    }
  }

  /**
   * Busca paquetes recientes (últimas versiones)
   */
  async obtenerPaquetesRecientes(
    limite: number = 10,
    tipo?: 'maven' | 'npm' | 'pypi'
  ): Promise<PackageResult[]> {
    if (limite < 1 || limite > 50) {
      throw new Error('El límite debe estar entre 1 y 50');
    }

    try {
      // Buscar con query vacía para obtener paquetes recientes
      const resultados = await this.buscarPaquetes('', {
        limite: limite * 2, // Buscar más para filtrar
        tipo,
      });

      // Filtrar y ordenar por fecha de modificación
      const paquetesRecientes = (resultados.results || [])
        .filter(pkg => pkg.lastModified)
        .sort((a, b) => {
          const fechaA = new Date(a.lastModified || 0);
          const fechaB = new Date(b.lastModified || 0);
          return fechaB.getTime() - fechaA.getTime();
        })
        .slice(0, limite);

      return paquetesRecientes;
    } catch (error) {
      console.error('Error al obtener paquetes recientes:', error);
      throw new Error('No se pudieron obtener los paquetes recientes');
    }
  }

  /**
   * Realiza búsqueda avanzada con múltiples criterios
   */
  async busquedaAvanzada(criterios: {
    query: string;
    tipo?: 'maven' | 'npm' | 'pypi';
    licencia?: string;
    mantenedor?: string;
    palabrasClave?: string[];
    minDescargas?: number;
  }): Promise<PackageResult[]> {
    if (!criterios.query || criterios.query.trim().length === 0) {
      throw new Error('La consulta de búsqueda es requerida');
    }

    try {
      // Buscar con la query principal
      const resultados = await this.buscarPaquetes(criterios.query, {
        limite: 100, // Buscar más para filtrar
      });

      // Aplicar filtros adicionales
      let paquetesFiltrados = resultados.results || [];

      if (criterios.tipo) {
        paquetesFiltrados = paquetesFiltrados.filter(
          pkg => pkg.type === criterios.tipo
        );
      }

      if (criterios.licencia) {
        paquetesFiltrados = paquetesFiltrados.filter(pkg =>
          pkg.license?.toLowerCase().includes(criterios.licencia!.toLowerCase())
        );
      }

      if (criterios.mantenedor) {
        paquetesFiltrados = paquetesFiltrados.filter(pkg =>
          pkg.maintainers?.some(m =>
            m.toLowerCase().includes(criterios.mantenedor!.toLowerCase())
          )
        );
      }

      if (criterios.palabrasClave && criterios.palabrasClave.length > 0) {
        paquetesFiltrados = paquetesFiltrados.filter(pkg =>
          criterios.palabrasClave!.some(palabra =>
            pkg.keywords?.some(k =>
              k.toLowerCase().includes(palabra.toLowerCase())
            )
          )
        );
      }

      if (criterios.minDescargas) {
        paquetesFiltrados = paquetesFiltrados.filter(
          pkg => (pkg.downloads || 0) >= criterios.minDescargas!
        );
      }

      return paquetesFiltrados;
    } catch (error) {
      console.error('Error en búsqueda avanzada:', error);
      throw new Error('No se pudieron realizar la búsqueda avanzada');
    }
  }

  // ===== MÉTODOS PRIVADOS =====

  /**
   * Genera sugerencias mock basadas en la query
   */
  private generarSugerenciasMock(query: string): string[] {
    const paquetesComunes = [
      'react',
      'vue',
      'angular',
      'svelte',
      'express',
      'fastify',
      'nestjs',
      'lodash',
      'moment',
      'axios',
      'junit',
      'spring-boot',
      'hibernate',
      'requests',
      'flask',
      'django',
    ];

    const queryLower = query.toLowerCase();

    return paquetesComunes
      .filter(pkg => pkg.toLowerCase().includes(queryLower))
      .slice(0, 5);
  }
}

/**
 * Puerto (interfaz) para el adaptador de búsqueda
 * Define el contrato que deben implementar los adaptadores de datos
 */
export interface SearchPort {
  buscarArtifatos(params: SearchArtifactsParams): Promise<SearchResults>;
}
