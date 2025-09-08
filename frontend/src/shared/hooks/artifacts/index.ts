/**
 * Exportaciones de hooks de artefactos
 * Arquitectura Clean Code con separación de responsabilidades
 */

// Queries (lectura de datos) - Nueva arquitectura Clean Code
export {
  useArtifactInfo,
  useArtifactPresignedUrl,
  useArtifactDownload,
} from './useArtifactQueries';

// Mutations (escritura de datos) - Nueva arquitectura Clean Code
export {
  useUploadArtifact,
  useValidateArtifact,
  useAnalyzePackageType,
  useGenerateArtifactMetadata,
} from './useArtifactMutations';

// Servicio - Nueva arquitectura Clean Code
export { useArtifactService } from './useArtifactService';

// Claves de consulta
export { ARTIFACT_QUERY_KEYS } from './artifactQueryKeys';

// Tipos
export type {
  ArtifactUploadResponse,
  PresignedUrlResponse,
  UploadArtifactBody,
} from '@/shared/types/openapi-generated.types';
