/**
 * Exportaciones de hooks de búsqueda
 * Arquitectura Clean Code con separación de responsabilidades
 */

// Queries (lectura de datos) - Nueva arquitectura Clean Code
export {
  useSearchPackages,
  useSearchSuggestions,
  usePopularPackages,
  useRecentPackages,
  useAdvancedSearch
} from './useSearchQueries';

// Servicio - Nueva arquitectura Clean Code
export { useSearchService } from './useSearchService';

// Claves de consulta
export { SEARCH_QUERY_KEYS } from './searchQueryKeys';

// Tipos
export type {
  SearchResults,
  PackageResult,
  SearchArtifactsParams
} from '@/shared/types/openapi-generated.types';