import { apiClient } from './client';
import type { ApiResponse } from './types';

export interface SearchFilters {
  query?: string;
  repositoryId?: string;
  type?: string[];
  sizeMin?: number;
  sizeMax?: number;
  createdAfter?: string;
  createdBefore?: string;
  sortBy?: 'relevance' | 'name' | 'size' | 'createdAt' | 'updatedAt';
  sortOrder?: 'asc' | 'desc';
  limit?: number;
  offset?: number;
}

export interface SearchResult {
  id: string;
  name: string;
  path: string;
  repositoryId: string;
  repositoryName: string;
  type: string;
  size: number;
  createdAt: string;
  updatedAt: string;
  downloadUrl?: string;
  metadata?: Record<string, any>;
  highlight?: {
    name?: string[];
    path?: string[];
    content?: string[];
  };
}

export interface SearchFacets {
  repositories: Array<{ id: string; name: string; count: number }>;
  types: Array<{ name: string; count: number }>;
  sizeRanges: Array<{ min: number; max: number; count: number }>;
  dateRanges: Array<{ start: string; end: string; count: number }>;
}

export interface SearchResponse {
  results: SearchResult[];
  total: number;
  facets?: SearchFacets;
  query?: string;
  took?: number;
}

export interface SearchSuggestion {
  text: string;
  type: 'repository' | 'artifact' | 'tag' | 'user';
  score: number;
}

export const searchApi = {
  // Basic search
  search: async (
    filters: SearchFilters
  ): Promise<ApiResponse<SearchResponse>> => {
    const params = new URLSearchParams();

    if (filters.query) params.append('q', filters.query);
    if (filters.repositoryId)
      params.append('repositoryId', filters.repositoryId);
    if (filters.type?.length) {
      filters.type.forEach(type => params.append('type', type));
    }
    if (filters.sizeMin !== undefined)
      params.append('sizeMin', filters.sizeMin.toString());
    if (filters.sizeMax !== undefined)
      params.append('sizeMax', filters.sizeMax.toString());
    if (filters.createdAfter)
      params.append('createdAfter', filters.createdAfter);
    if (filters.createdBefore)
      params.append('createdBefore', filters.createdBefore);
    if (filters.sortBy) params.append('sortBy', filters.sortBy);
    if (filters.sortOrder) params.append('sortOrder', filters.sortOrder);
    if (filters.limit !== undefined)
      params.append('limit', filters.limit.toString());
    if (filters.offset !== undefined)
      params.append('offset', filters.offset.toString());

    const response = await apiClient.get(`/search?${params.toString()}`);
    return response.data;
  },

  // Autocomplete suggestions
  suggest: async (
    query: string,
    limit: number = 5
  ): Promise<ApiResponse<SearchSuggestion[]>> => {
    const response = await apiClient.get(`/search/suggest`, {
      params: { q: query, limit },
    });
    return response.data;
  },

  // Get search facets
  getFacets: async (
    filters: SearchFilters
  ): Promise<ApiResponse<SearchFacets>> => {
    const params = new URLSearchParams();

    if (filters.query) params.append('q', filters.query);
    if (filters.repositoryId)
      params.append('repositoryId', filters.repositoryId);
    if (filters.type?.length) {
      filters.type.forEach(type => params.append('type', type));
    }

    const response = await apiClient.get(`/search/facets?${params.toString()}`);
    return response.data;
  },

  // Advanced search with complex queries
  advancedSearch: async (
    query: string,
    filters: Omit<SearchFilters, 'query'> = {}
  ): Promise<ApiResponse<SearchResponse>> => {
    const params = new URLSearchParams();

    params.append('q', query);
    if (filters.repositoryId)
      params.append('repositoryId', filters.repositoryId);
    if (filters.type?.length) {
      filters.type.forEach(type => params.append('type', type));
    }
    if (filters.sizeMin !== undefined)
      params.append('sizeMin', filters.sizeMin.toString());
    if (filters.sizeMax !== undefined)
      params.append('sizeMax', filters.sizeMax.toString());
    if (filters.createdAfter)
      params.append('createdAfter', filters.createdAfter);
    if (filters.createdBefore)
      params.append('createdBefore', filters.createdBefore);
    if (filters.sortBy) params.append('sortBy', filters.sortBy);
    if (filters.sortOrder) params.append('sortOrder', filters.sortOrder);
    if (filters.limit !== undefined)
      params.append('limit', filters.limit.toString());
    if (filters.offset !== undefined)
      params.append('offset', filters.offset.toString());

    const response = await apiClient.get(
      `/search/advanced?${params.toString()}`
    );
    return response.data;
  },

  // Search within a specific repository
  searchInRepository: async (
    repositoryId: string,
    query: string,
    filters: Omit<SearchFilters, 'repositoryId' | 'query'> = {}
  ): Promise<ApiResponse<SearchResponse>> => {
    const params = new URLSearchParams();

    params.append('q', query);
    if (filters.type?.length) {
      filters.type.forEach(type => params.append('type', type));
    }
    if (filters.sizeMin !== undefined)
      params.append('sizeMin', filters.sizeMin.toString());
    if (filters.sizeMax !== undefined)
      params.append('sizeMax', filters.sizeMax.toString());
    if (filters.createdAfter)
      params.append('createdAfter', filters.createdAfter);
    if (filters.createdBefore)
      params.append('createdBefore', filters.createdBefore);
    if (filters.sortBy) params.append('sortBy', filters.sortBy);
    if (filters.sortOrder) params.append('sortOrder', filters.sortOrder);
    if (filters.limit !== undefined)
      params.append('limit', filters.limit.toString());
    if (filters.offset !== undefined)
      params.append('offset', filters.offset.toString());

    const response = await apiClient.get(
      `/search/repository/${repositoryId}?${params.toString()}`
    );
    return response.data;
  },

  // Get search history
  getSearchHistory: async (): Promise<ApiResponse<string[]>> => {
    const response = await apiClient.get('/search/history');
    return response.data;
  },

  // Clear search history
  clearSearchHistory: async (): Promise<ApiResponse<void>> => {
    const response = await apiClient.delete('/search/history');
    return response.data;
  },

  // Save search as favorite
  saveSearch: async (
    name: string,
    query: string,
    filters: SearchFilters
  ): Promise<ApiResponse<void>> => {
    const response = await apiClient.post('/search/favorites', {
      name,
      query,
      filters,
    });
    return response.data;
  },

  // Get saved searches
  getSavedSearches: async (): Promise<
    ApiResponse<
      Array<{
        id: string;
        name: string;
        query: string;
        filters: SearchFilters;
        createdAt: string;
      }>
    >
  > => {
    const response = await apiClient.get('/search/favorites');
    return response.data;
  },

  // Delete saved search
  deleteSavedSearch: async (id: string): Promise<ApiResponse<void>> => {
    const response = await apiClient.delete(`/search/favorites/${id}`);
    return response.data;
  },
};
