/**
 * Puerto (interfaz) para el adaptador de artefactos
 * Define el contrato que deben implementar los adaptadores de datos
 * Siguiendo el principio de Inversión de Dependencias (DIP) de SOLID
 */

import type {
  ArtifactUploadResponse,
  PresignedUrlResponse,
  UploadArtifactBody,
  GetArtifactParams,
} from '@/shared/types/openapi-generated.types';

/**
 * Puerto que define las operaciones de acceso a datos para artefactos
 * Los adaptadores (HTTP, Mock, etc.) deben implementar esta interfaz
 */
export interface ArtifactPort {
  /**
   * Upload an artifact to the repository
   */
  uploadArtifact(body: UploadArtifactBody): Promise<ArtifactUploadResponse>;

  /**
   * Get an artifact by its ID
   */
  getArtifact(params: GetArtifactParams): Promise<Blob | PresignedUrlResponse>;
}
