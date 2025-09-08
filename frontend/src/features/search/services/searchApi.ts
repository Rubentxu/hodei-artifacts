import { mockAdapter } from '@/shared/api/mockAdapter';
import type { SearchResponse, SearchFilters } from '../types/search.types';

class SearchService {
  private readonly basePath = '/search';

  async search(filters: SearchFilters): Promise<SearchResponse> {
    try {
      // Usar el adaptador para obtener datos de servicios mock mejorados
      const legacyResponse = await mockAdapter.search(filters);

      // Convertir respuesta legacy al formato esperado por los componentes actuales
      return {
        results: legacyResponse.results,
        total: legacyResponse.total,
        facets: legacyResponse.facets,
      };
    } catch (error) {
      console.error('Error in enhanced search service:', error);
      // Retornar datos de respaldo si hay error
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
            popularity: 45000,
          },
        ],
        total: 1,
        facets: {
          type: [{ value: 'npm', count: 1 }],
          repository: [{ value: 'npm-public', count: 1 }],
        },
      };
    }
  }

  async getSuggestions(query: string): Promise<string[]> {
    try {
      return await mockAdapter.getSuggestions(query);
    } catch (error) {
      console.error('Error in enhanced suggestions service:', error);
      return ['react', 'vue', 'angular']; // Datos de respaldo
    }
  }
}

export const searchService = new SearchService();
