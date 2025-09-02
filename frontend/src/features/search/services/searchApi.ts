import type { SearchResponse, SearchFilters } from '../types/search.types';

class SearchService {
  private readonly basePath = '/search';

  async search(filters: SearchFilters): Promise<SearchResponse> {
    const params = new URLSearchParams();

    Object.entries(filters).forEach(([key, value]) => {
      if (value !== undefined && value !== '' && value !== null) {
        if (Array.isArray(value)) {
          value.forEach(v => params.append(key, v));
        } else {
          params.append(key, String(value));
        }
      }
    });

    // NOTE: Using a placeholder response until the actual API is ready.
    // const response = await apiClient.get<SearchResponse>(`${this.basePath}?${params.toString()}`);
    // return response.data;

    console.log(`[Mock] Searching with filters: ${params.toString()}`);
    return Promise.resolve({
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
        {
          id: '2',
          name: 'vue',
          description: 'The Progressive JavaScript Framework',
          repository: 'npm-public',
          type: 'npm',
          version: '3.2.0',
          size: 1800000,
          license: 'MIT',
          popularity: 38000,
        },
        {
          id: '3',
          name: 'django',
          description: 'The Web framework for perfectionists with deadlines.',
          repository: 'pypi-main',
          type: 'pypi',
          version: '4.1.0',
          size: 890000,
          license: 'BSD',
          popularity: 68000,
        },
      ],
      total: 3,
      facets: {
        type: [
          { value: 'npm', count: 2 },
          { value: 'pypi', count: 1 },
        ],
        repository: [
          { value: 'npm-public', count: 2 },
          { value: 'pypi-main', count: 1 },
        ],
      },
    });
  }

  async getSuggestions(query: string): Promise<string[]> {
    // const response = await apiClient.get<string[]>(`${this.basePath}/suggestions?q=${query}`);
    // return response.data;

    console.log(`[Mock] Getting suggestions for: ${query}`);
    const allItems = ['react', 'react-dom', 'vue', 'vite', 'django', 'docker'];
    return Promise.resolve(
      allItems.filter(item => item.toLowerCase().includes(query.toLowerCase()))
    );
  }
}

export const searchService = new SearchService();
