import { useQuery, useInfiniteQuery } from '@tanstack/react-query';
import { searchService } from '../services/searchApi';
import type { SearchFilters } from '../types/search.types';

const SEARCH_KEY = 'search';

export const useSearch = (filters: SearchFilters) => {
  return useQuery({
    queryKey: [SEARCH_KEY, 'results', filters],
    queryFn: () => searchService.search(filters),
    enabled: !!filters.query, // Only run query if there is a search term
    placeholderData: previousData => previousData,
  });
};

export const useSearchSuggestions = (query: string) => {
  return useQuery({
    queryKey: [SEARCH_KEY, 'suggestions', query],
    queryFn: () => searchService.getSuggestions(query),
    enabled: !!query && query.length > 2, // Only fetch suggestions for longer queries
  });
};

export const useInfiniteSearch = (filters: Omit<SearchFilters, 'page'>) => {
  return useInfiniteQuery({
    queryKey: [SEARCH_KEY, 'infinite', filters],
    queryFn: ({ pageParam = 1 }) =>
      searchService.search({ ...filters, page: pageParam }),
    getNextPageParam: (lastPage, allPages) => {
      const loadedCount = allPages.reduce(
        (acc, page) => acc + page.results.length,
        0
      );
      if (loadedCount < lastPage.total) {
        return allPages.length + 1;
      }
      return undefined;
    },
    enabled: !!filters.query,
    placeholderData: previousData => previousData,
  });
};
