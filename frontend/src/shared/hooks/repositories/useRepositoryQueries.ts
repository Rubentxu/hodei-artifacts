/**
 * Hooks de React Query para consultas de repositorios
 * Sigue principios SOLID y separación de responsabilidades
 */

import { useQuery } from '@tanstack/react-query';
import type { Repository, RepositoryFilters, PaginatedResponse } from '@/shared/types';
import { useRepositoryService } from './useRepositoryService';
import { QUERY_KEYS } from '@/shared/constants';

/**
 * Hook para obtener lista paginada de repositorios
 * Principio de Responsabilidad Única: Solo maneja la consulta de repositorios
 */
export function useRepositoryList(filters: RepositoryFilters) {
  const repositoryService = useRepositoryService();

  return useQuery<PaginatedResponse<Repository>, Error>({
    queryKey: QUERY_KEYS.REPOSITORIES,
    queryFn: () => repositoryService.obtenerRepositoriosPaginados(filters),
    staleTime: 5 * 60 * 1000, // 5 minutos
    gcTime: 10 * 60 * 1000, // 10 minutos (nuevo nombre de cacheTime)
    retry: 2,
    retryDelay: (attemptIndex) => Math.min(1000 * 2 ** attemptIndex, 30000),
  });
}

/**
 * Hook para obtener un repositorio específico por ID
 * Principio de Responsabilidad Única: Solo maneja la consulta de un repositorio
 */
export function useRepositoryById(id: string | undefined) {
  const repositoryService = useRepositoryService();

  return useQuery<Repository, Error>({
    queryKey: QUERY_KEYS.REPOSITORY(id || ''),
    queryFn: () => {
      if (!id) throw new Error('ID de repositorio requerido');
      return repositoryService.obtenerRepositorioPorId(id);
    },
    enabled: !!id, // Solo ejecutar si hay un ID válido
    staleTime: 3 * 60 * 1000, // 3 minutos
    gcTime: 5 * 60 * 1000, // 5 minutos (nuevo nombre de cacheTime)
    retry: 1,
  });
}

/**
 * Hook para obtener repositorios por tipo
 * Principio de Responsabilidad Única: Solo maneja la consulta por tipo
 */
export function useRepositoriesByType(type: Repository['type']) {
  const repositoryService = useRepositoryService();

  return useQuery<Repository[], Error>({
    queryKey: ['repositories', 'by-type', type],
    queryFn: () => repositoryService.obtenerRepositoriosPorTipo(type),
    staleTime: 5 * 60 * 1000, // 5 minutos
    gcTime: 10 * 60 * 1000, // 10 minutos (nuevo nombre de cacheTime)
    retry: 2,
  });
}

/**
 * Hook para obtener métricas de repositorios
 * Principio de Responsabilidad Única: Solo maneja la consulta de métricas
 */
export function useRepositoryMetrics() {
  const repositoryService = useRepositoryService();

  return useQuery<{
    total: number;
    porTipo: Record<Repository['type'], number>;
    activos: number;
    inactivos: number;
  }, Error>({
    queryKey: ['repositories', 'metrics'],
    queryFn: () => repositoryService.obtenerMetricasRepositorios(),
    staleTime: 10 * 60 * 1000, // 10 minutos
    gcTime: 30 * 60 * 1000, // 30 minutos (nuevo nombre de cacheTime)
    retry: 1,
  });
}