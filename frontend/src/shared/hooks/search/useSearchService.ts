/**
 * Hook para obtener el servicio de búsqueda
 * Implementa inyección de dependencias para facilitar testing
 */

import { useMemo } from 'react';
import { SearchService } from '@/shared/services/search/SearchService';
import { OpenAPISearchAdapter } from '@/shared/services/search/adapters/OpenAPISearchAdapter';

/**
 * Hook que proporciona el servicio de búsqueda
 * Usa inyección de dependencias para facilitar el testing
 */
export function useSearchService(): SearchService {
  return useMemo(() => {
    // Inyectar el adaptador OpenAPI como dependencia
    const searchAdapter = new OpenAPISearchAdapter();
    return new SearchService(searchAdapter);
  }, []);
}
