/**
 * Claves de consulta para React Query en el módulo de artefactos
 * Centraliza las claves para mantener consistencia y evitar duplicación
 */

/**
 * Genera claves de consulta para artefactos
 */
export const ARTIFACT_QUERY_KEYS = {
  /**
   * Clave para información de artefacto
   */
  ARTIFACT: (id: string, presignado: boolean = false) =>
    ['artifacts', 'info', id, { presignado }] as const,

  /**
   * Clave para URL presignada de artefacto
   */
  PRESIGNED_URL: (id: string) => ['artifacts', 'presigned-url', id] as const,

  /**
   * Clave para descarga de artefacto
   */
  DOWNLOAD: (id: string) => ['artifacts', 'download', id] as const,

  /**
   * Clave para lista de artefactos
   */
  LIST: (filters?: Record<string, any>) =>
    ['artifacts', 'list', filters] as const,

  /**
   * Clave para artefactos por repositorio
   */
  BY_REPOSITORY: (repositoryId: string) =>
    ['artifacts', 'by-repository', repositoryId] as const,
} as const;
