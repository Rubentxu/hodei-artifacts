/**
 * Claves de consulta para React Query en el módulo de búsqueda
 * Centraliza las claves para mantener consistencia y evitar duplicación
 */

/**
 * Genera claves de consulta para búsqueda
 */
export const SEARCH_QUERY_KEYS = {
  /**
   * Clave para búsqueda de paquetes
   */
  SEARCH: (query: string, options?: Record<string, any>) =>
    ['search', 'packages', query, options] as const,

  /**
   * Clave para sugerencias de búsqueda
   */
  SUGGESTIONS: (query: string) => ['search', 'suggestions', query] as const,

  /**
   * Clave para paquetes populares
   */
  POPULAR: (limit: number, type?: string) =>
    ['search', 'popular', limit, type] as const,

  /**
   * Clave para paquetes recientes
   */
  RECENT: (limit: number, type?: string) =>
    ['search', 'recent', limit, type] as const,

  /**
   * Clave para búsqueda avanzada
   */
  ADVANCED: (criterios: Record<string, any>) =>
    ['search', 'advanced', criterios] as const,
} as const;
