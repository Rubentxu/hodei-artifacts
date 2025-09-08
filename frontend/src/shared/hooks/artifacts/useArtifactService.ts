/**
 * Hook para obtener el servicio de artefactos
 * Implementa inyección de dependencias para facilitar testing
 */

import { useMemo } from 'react';
import { ArtifactService } from '@/shared/services/artifacts/ArtifactService';
import { OpenAPIArtifactAdapter } from '@/shared/services/artifacts/adapters/OpenAPIArtifactAdapter';

/**
 * Hook que proporciona el servicio de artefactos
 * Usa inyección de dependencias para facilitar el testing
 */
export function useArtifactService(): ArtifactService {
  return useMemo(() => {
    // Inyectar el adaptador OpenAPI como dependencia
    const artifactAdapter = new OpenAPIArtifactAdapter();
    return new ArtifactService(artifactAdapter);
  }, []);
}