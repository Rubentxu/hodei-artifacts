/**
 * Adaptador de compatibilidad para hooks legacy de repositorios
 * Permite la transición gradual hacia la nueva arquitectura Clean Code
 * sin romper los componentes existentes
 */

import { useQuery } from '@tanstack/react-query';
import type {
  Repository,
  RepositoryFilters,
  PaginatedResponse,
} from '@/shared/types';
import { useRepositoryService } from './useRepositoryService';
import { QUERY_KEYS } from '@/shared/constants';

/**
 * Hook legacy para obtener lista de repositorios
 * Mantiene la interfaz antigua pero usa el nuevo servicio internamente
 */
export function useRepositories(filters: RepositoryFilters) {
  const repositoryService = useRepositoryService();

  return useQuery<PaginatedResponse<Repository>, Error>({
    queryKey: QUERY_KEYS.REPOSITORIES,
    queryFn: () => repositoryService.obtenerRepositoriosPaginados(filters),
    staleTime: 5 * 60 * 1000,
    gcTime: 10 * 60 * 1000,
    retry: 2,
  });
}

/**
 * Hook legacy para obtener un repositorio específico
 * Mantiene la interfaz antigua pero usa el nuevo servicio internamente
 */
export function useRepository(id: string) {
  const repositoryService = useRepositoryService();

  return useQuery<{ data: Repository }, Error>({
    queryKey: QUERY_KEYS.REPOSITORY(id),
    queryFn: async () => {
      const repository = await repositoryService.obtenerRepositorioPorId(id);
      return { data: repository };
    },
    enabled: !!id,
    staleTime: 3 * 60 * 1000,
    gcTime: 5 * 60 * 1000,
    retry: 1,
  });
}

/**
 * Hook legacy para gestionar filtros de repositorios
 * Mantiene la interfaz antigua
 */
export function useRepositoryFilters(initialFilters: RepositoryFilters) {
  // Por ahora, retornar un mock simple que mantenga compatibilidad
  // En el futuro, esto puede ser reemplazado por un estado más sofisticado
  const filters = initialFilters;

  const updateFilter = <K extends keyof RepositoryFilters>(
    key: K,
    value: RepositoryFilters[K]
  ) => {
    // Por ahora, no hacer nada ya que los filtros se pasan directamente al servicio
    console.log(`Filter update requested: ${String(key)} = ${value}`);
  };

  const clearFilters = () => {
    // Por ahora, no hacer nada
    console.log('Clear filters requested');
  };

  return {
    filters,
    updateFilter,
    clearFilters,
  };
}
