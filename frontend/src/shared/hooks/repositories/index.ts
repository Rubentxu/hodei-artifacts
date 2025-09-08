/**
 * Exportaciones de hooks de repositorios
 * Arquitectura Clean Code con separación de responsabilidades
 */

// Queries (lectura de datos) - Nueva arquitectura Clean Code
export {
  useRepositoryList,
  useRepositoryById,
  useRepositoriesByType,
  useRepositoryMetrics
} from './useRepositoryQueries';

// Mutations (escritura de datos) - Nueva arquitectura Clean Code
export {
  useCreateRepository,
  useUpdateRepository,
  useDeleteRepository
} from './useRepositoryMutations';

// Servicio - Nueva arquitectura Clean Code
export { useRepositoryService } from './useRepositoryService';

// Legacy hooks (adaptadores para compatibilidad)
export {
  useRepositories,
  useRepository,
  useRepositoryFilters
} from './legacyAdapter';

// Tipos
export type {
  RepositoryFilters,
  Repository,
  PaginatedResponse,
  CreateRepositoryRequest,
  UpdateRepositoryRequest
} from '@/shared/types';