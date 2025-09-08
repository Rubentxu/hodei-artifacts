/**
 * Puerto (interfaz) para el adaptador de búsqueda
 * Define el contrato que deben implementar los adaptadores de datos
 * Siguiendo el principio de Inversión de Dependencias (DIP) de SOLID
 */

import type {
  SearchArtifactsParams,
  SearchResults,
} from '@/shared/types/openapi-generated.types';

/**
 * Puerto que define las operaciones de acceso a datos para búsqueda
 * Los adaptadores (HTTP, Mock, etc.) deben implementar esta interfaz
 */
export interface SearchPort {
  /**
   * Busca artefactos/paquetes con parámetros específicos
   */
  buscarArtifatos(params: SearchArtifactsParams): Promise<SearchResults>;
}
