/**
 * Hooks de React Query para consultas de búsqueda
 * Sigue principios SOLID y separación de responsabilidades
 */

import { useQuery } from '@tanstack/react-query';
import type { SearchResults, PackageResult } from '@/shared/types/openapi-generated.types';
import { useSearchService } from './useSearchService';
import { SEARCH_QUERY_KEYS } from './searchQueryKeys';

/**
 * Hook para buscar paquetes/artefactos
 * Principio de Responsabilidad Única: Solo maneja la búsqueda
 */
export function useSearchPackages(query: string, options?: {
  limit?: number;
  offset?: number;
  type?: 'maven' | 'npm' | 'pypi';
}) {
  const searchService = useSearchService();

  return useQuery<SearchResults, Error>({
    queryKey: SEARCH_QUERY_KEYS.SEARCH(query, options),
    queryFn: () => searchService.buscarPaquetes(query, options),
    enabled: query.length >= 2, // Solo buscar si hay al menos 2 caracteres
    staleTime: 2 * 60 * 1000, // 2 minutos
    gcTime: 5 * 60 * 1000, // 5 minutos
    retry: 1,
  });
}

/**
 * Hook para obtener sugerencias de búsqueda
 * Principio de Responsabilidad Única: Solo maneja las sugerencias
 */
export function useSearchSuggestions(query: string) {
  const searchService = useSearchService();

  return useQuery<string[], Error>({
    queryKey: SEARCH_QUERY_KEYS.SUGGESTIONS(query),
    queryFn: () => searchService.obtenerSugerencias(query),
    enabled: query.length >= 1, // Solo obtener sugerencias si hay al menos 1 carácter
    staleTime: 30 * 1000, // 30 segundos
    gcTime: 2 * 60 * 1000, // 2 minutos
    retry: 0, // No reintentar sugerencias
  });
}

/**
 * Hook para obtener paquetes populares
 * Principio de Responsabilidad Única: Solo maneja paquetes populares
 */
export function usePopularPackages(limit: number = 10, type?: 'maven' | 'npm' | 'pypi') {
  const searchService = useSearchService();

  return useQuery<PackageResult[], Error>({
    queryKey: SEARCH_QUERY_KEYS.POPULAR(limit, type),
    queryFn: () => searchService.obtenerPaquetesPopulares(limit, type),
    staleTime: 10 * 60 * 1000, // 10 minutos
    gcTime: 30 * 60 * 1000, // 30 minutos
    retry: 2,
  });
}

/**
 * Hook para obtener paquetes recientes
 * Principio de Responsabilidad Única: Solo maneja paquetes recientes
 */
export function useRecentPackages(limit: number = 10, type?: 'maven' | 'npm' | 'pypi') {
  const searchService = useSearchService();

  return useQuery<PackageResult[], Error>({
    queryKey: SEARCH_QUERY_KEYS.RECENT(limit, type),
    queryFn: () => searchService.obtenerPaquetesRecientes(limit, type),
    staleTime: 5 * 60 * 1000, // 5 minutos
    gcTime: 15 * 60 * 1000, // 15 minutos
    retry: 2,
  });
}

/**
 * Hook para búsqueda avanzada con múltiples criterios
 * Principio de Responsabilidad Única: Solo maneja búsqueda avanzada
 */
export function useAdvancedSearch(criterios: {
  query: string;
  type?: 'maven' | 'npm' | 'pypi';
  license?: string;
  maintainer?: string;
  keywords?: string[];
  minDownloads?: number;
}) {
  const searchService = useSearchService();

  return useQuery<PackageResult[], Error>({
    queryKey: SEARCH_QUERY_KEYS.ADVANCED(criterios),
    queryFn: () => searchService.busquedaAvanzada(criterios),
    enabled: criterios.query.length >= 2, // Solo buscar si hay al menos 2 caracteres
    staleTime: 3 * 60 * 1000, // 3 minutos
    gcTime: 10 * 60 * 1000, // 10 minutos
    retry: 1,
  });
}