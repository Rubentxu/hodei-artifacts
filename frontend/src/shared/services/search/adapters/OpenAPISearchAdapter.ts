/**
 * Adaptador de búsqueda que implementa el puerto SearchPort
 * Utiliza el cliente OpenAPI para comunicación con el backend
 * Sigue el patrón Adapter (Puerto y Adaptador)
 */

import type { SearchPort } from '../ports/SearchPort';
import type {
  SearchArtifactsParams,
  SearchResults,
} from '@/shared/types/openapi-generated.types';
import { openAPIClient } from '@/shared/api/openapi-client';

/**
 * Adaptador que implementa SearchPort usando el cliente OpenAPI
 */
export class OpenAPISearchAdapter implements SearchPort {
  /**
   * Busca artefactos/paquetes con parámetros específicos
   */
  async buscarArtifatos(params: SearchArtifactsParams): Promise<SearchResults> {
    try {
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.searchArtifacts(params);

      // El resultado ya está en el formato correcto según el contrato OpenAPI
      return resultado;
    } catch (error) {
      console.error('Error al buscar artefactos:', error);
      throw new Error('Error al buscar artefactos');
    }
  }
}
