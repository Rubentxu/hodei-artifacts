// Using placeholder types until OpenAPI generation is resolved
// Based on docs/frontend/pages-specification.md for search results

export interface SearchResultItem {
  id: string;
  name: string;
  description: string;
  repository: string;
  type: 'maven' | 'npm' | 'pypi' | 'docker';
  version: string;
  size: number;
  license: string;
  popularity: number; // e.g., star count
}

export interface SearchResponse {
  results: SearchResultItem[];
  total: number;
  facets: Record<string, Array<{ value: string; count: number }>>;
}

export interface SearchFilters {
  query: string;
  type?: string[];
  repository?: string[];
  size_lt?: number;
  size_gt?: number;
  date_from?: string;
  date_to?: string;
  page?: number;
  limit?: number;
  sortBy?: 'relevance' | 'name' | 'date';
}
