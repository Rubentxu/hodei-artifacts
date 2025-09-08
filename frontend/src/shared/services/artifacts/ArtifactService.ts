/**
 * Servicio de dominio para gestión de artefactos
 * Sigue principios SOLID y Clean Code
 */

import type { 
  ArtifactUploadResponse,
  PresignedUrlResponse,
  UploadArtifactBody,
  GetArtifactParams
} from '@/shared/types/openapi-generated.types';
import type { ArtifactPort } from './ports/ArtifactPort.js';

/**
 * Servicio de aplicación para operaciones de artefactos
 * Implementa la lógica de negocio específica del dominio
 */
export class ArtifactService {
  constructor(artifactPort: ArtifactPort) {
    this.artifactPort = artifactPort;
  }

  private readonly artifactPort: ArtifactPort;

  /**
   * Upload an artifact to the repository
   */
  async uploadArtifact(
    file: File,
    metadata: Record<string, any>,
    repositoryId?: string
  ): Promise<ArtifactUploadResponse> {
    if (!file) {
      throw new Error('File is required');
    }

    if (file.size === 0) {
      throw new Error('File cannot be empty');
    }

    // Validate maximum size (100MB)
    const MAX_FILE_SIZE = 100 * 1024 * 1024; // 100MB
    if (file.size > MAX_FILE_SIZE) {
      throw new Error('File exceeds the maximum allowed size of 100MB');
    }

    try {
      const body: UploadArtifactBody = {
        file: file,
        metadata: JSON.stringify(metadata)
      };

      return await this.artifactPort.uploadArtifact(body);
    } catch (error) {
      console.error('Error uploading artifact:', error);
      throw new Error('Could not upload artifact');
    }
  }

  /**
   * Get an artifact by its ID
   */
  async getArtifact(
    id: string,
    presigned: boolean = false
  ): Promise<Blob | PresignedUrlResponse> {
    if (!id || id.trim().length === 0) {
      throw new Error('Artifact ID is required');
    }

    try {
      const params: GetArtifactParams = {
        id,
        presigned: presigned
      };

      return await this.artifactPort.getArtifact(params);
    } catch (error) {
      console.error(`Error getting artifact ${id}:`, error);
      throw new Error('Could not get artifact');
    }
  }

  /**
   * Get a presigned URL to download an artifact
   */
  async getPresignedUrl(id: string): Promise<PresignedUrlResponse> {
    const result = await this.getArtifact(id, true);
    
    if (result instanceof Blob) {
      throw new Error('Expected a presigned URL, but received a blob');
    }

    return result;
  }

  /**
   * Download an artifact directly
   */
  async downloadArtifact(id: string): Promise<Blob> {
    const result = await this.getArtifact(id, false);
    
    if (!(result instanceof Blob)) {
      throw new Error('Expected a blob, but received a presigned URL');
    }

    return result;
  }

  /**
   * Validate a file before uploading
   */
  validateFile(file: File): { valid: boolean; errors: string[] } {
    const errors: string[] = [];

    if (!file) {
      errors.push('File is required');
      return { valid: false, errors };
    }

    if (file.size === 0) {
      errors.push('File cannot be empty');
    }

    // Validate maximum size
    const MAX_FILE_SIZE = 100 * 1024 * 1024; // 100MB
    if (file.size > MAX_FILE_SIZE) {
      errors.push('File exceeds the maximum allowed size of 100MB');
    }

    // Validate allowed extensions
    const allowedExtensions = [
      '.jar', '.war', '.ear', '.pom',     // Maven
      '.tgz', '.tar.gz',                  // NPM
      '.whl', '.egg', '.zip'              // PyPI
    ];

    const extension = file.name.toLowerCase().substring(file.name.lastIndexOf('.'));
    if (!allowedExtensions.some(ext => file.name.toLowerCase().endsWith(ext))) {
      errors.push(`File type not allowed. Valid extensions: ${allowedExtensions.join(', ')}`);
    }

    return {
      valid: errors.length === 0,
      errors
    };
  }

  /**
   * Get package type based on file extension
   */
  getPackageType(filename: string): 'maven' | 'npm' | 'pypi' | 'unknown' {
    const name = filename.toLowerCase();
    
    if (name.endsWith('.jar') || name.endsWith('.war') ||
        name.endsWith('.ear') || name.endsWith('.pom')) {
      return 'maven';
    }
    
    if (name.endsWith('.tgz') || name.endsWith('.tar.gz')) {
      return 'npm';
    }
    
    if (name.endsWith('.whl') || name.endsWith('.egg') || name.endsWith('.zip')) {
      return 'pypi';
    }
    
    return 'unknown';
  }

  /**
   * Generate standard metadata for an artifact
   */
  generateMetadata(file: File, type?: 'maven' | 'npm' | 'pypi'): Record<string, any> {
    const detectedType = type || this.getPackageType(file.name);
    
    return {
      filename: file.name,
      size: file.size,
      type: file.type,
      packageType: detectedType,
      uploadedAt: new Date().toISOString(),
      checksum: 'pending', // Will be calculated on server
      originalName: file.name
    };
  }
}