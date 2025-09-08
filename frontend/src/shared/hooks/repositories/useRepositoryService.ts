/**
 * Hook para obtener el servicio de repositorios
 * Implementa inyección de dependencias para facilitar testing
 */

import { useMemo } from 'react';
import { RepositoryService } from '@/shared/services/repositories/RepositoryService';
import { OpenAPIRepositoryAdapter } from '@/shared/services/repositories/adapters/OpenAPIRepositoryAdapter';

/**
 * Hook que proporciona el servicio de repositorios
 * Usa inyección de dependencias para facilitar el testing
 */
export function useRepositoryService(): RepositoryService {
  return useMemo(() => {
    // Inyectar el adaptador OpenAPI como dependencia
    const repositoryAdapter = new OpenAPIRepositoryAdapter();
    return new RepositoryService(repositoryAdapter);
  }, []);
}
