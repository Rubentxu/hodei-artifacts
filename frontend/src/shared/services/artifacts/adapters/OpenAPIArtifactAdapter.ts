/**
 * Adaptador de artefactos que implementa el puerto ArtifactPort
 * Utiliza el cliente OpenAPI para comunicación con el backend
 * Sigue el patrón Adapter (Puerto y Adaptador)
 */

import type { ArtifactPort } from '../ports/ArtifactPort';
import type {
  ArtifactUploadResponse,
  PresignedUrlResponse,
  UploadArtifactBody,
  GetArtifactParams,
} from '@/shared/types/openapi-generated.types';
import { openAPIClient } from '@/shared/api/openapi-client';

/**
 * Adaptador que implementa ArtifactPort usando el cliente OpenAPI
 */
export class OpenAPIArtifactAdapter implements ArtifactPort {
  /**
   * Upload an artifact to the repository
   */
  async uploadArtifact(
    body: UploadArtifactBody
  ): Promise<ArtifactUploadResponse> {
    try {
      // Call OpenAPI client
      const result = await openAPIClient.uploadArtifact(
        this.createFormData(body)
      );

      // Result is already in correct format according to OpenAPI contract
      return result;
    } catch (error) {
      console.error('Error uploading artifact:', error);
      throw new Error('Error uploading artifact');
    }
  }

  /**
   * Get an artifact by its ID
   */
  async getArtifact(
    params: GetArtifactParams
  ): Promise<Blob | PresignedUrlResponse> {
    try {
      // Call OpenAPI client
      const result = await openAPIClient.getArtifact(params);

      // Result is already in correct format according to OpenAPI contract
      return result;
    } catch (error) {
      console.error(`Error getting artifact ${params.id}:`, error);
      throw new Error('Error getting artifact');
    }
  }

  /**
   * Create FormData from body for the request
   */
  private createFormData(body: UploadArtifactBody): FormData {
    const formData = new FormData();

    // Agregar el archivo
    formData.append('file', body.file);

    // Agregar los metadatos como string JSON
    formData.append('metadata', body.metadata);

    return formData;
  }
}
