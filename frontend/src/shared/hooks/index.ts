export { useDebounce } from './useDebounce';

// Repository hooks (Clean Code architecture)
export {
  useRepositoryList,
  useRepositoryById,
  useRepositoriesByType,
  useRepositoryMetrics,
  useCreateRepository,
  useUpdateRepository,
  useDeleteRepository,
  useRepositoryService,
} from './repositories/index';

// Search hooks (Clean Code architecture)
export {
  useSearchPackages,
  useSearchSuggestions,
  usePopularPackages,
  useRecentPackages,
  useAdvancedSearch,
  useSearchService,
  SEARCH_QUERY_KEYS,
} from './search';

// Legacy hooks (for backward compatibility during migration)
export {
  useRepositories,
  useRepository,
  useRepositoryFilters,
} from './repositories/index';
