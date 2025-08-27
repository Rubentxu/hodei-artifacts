import { apiService } from './client';
import type {
  ApiResponse,
  PaginatedResponse,
  ApiError,
} from '@/shared/types';

export interface Artifact {
  id: string;
  name: string;
  path: string;
  repositoryId: string;
  repositoryName: string;
  type: 'file' | 'directory';
  size: number;
  mimeType?: string;
  checksums: {
    md5: string;
    sha1: string;
    sha256: string;
  };
  createdAt: string;
  updatedAt: string;
  createdBy: string;
  downloadCount: number;
  metadata?: Record<string, any>;
}

export interface DirectoryListing {
  path: string;
  artifacts: Artifact[];
  directories: string[];
  totalCount: number;
  totalSize: number;
}

export interface UploadArtifactRequest {
  repositoryId: string;
  path: string;
  file: File;
  metadata?: Record<string, any>;
}

export interface UploadArtifactResponse {
  artifact: Artifact;
  url: string;
  checksums: {
    md5: string;
    sha1: string;
    sha256: string;
  };
}

export interface ArtifactFilters {
  repositoryId?: string;
  path?: string;
  type?: 'file' | 'directory';
  search?: string;
  mimeType?: string;
  minSize?: number;
  maxSize?: number;
  createdAfter?: string;
  createdBefore?: string;
  page?: number;
  limit?: number;
  sortBy?: 'name' | 'size' | 'createdAt' | 'updatedAt' | 'downloadCount';
  sortOrder?: 'asc' | 'desc';
}

export interface BatchOperationRequest {
  artifactIds: string[];
  operation: 'delete' | 'move' | 'copy';
  targetRepositoryId?: string;
  targetPath?: string;
}

export interface BatchOperationResponse {
  success: number;
  failed: number;
  errors?: Array<{
    artifactId: string;
    error: string;
  }>;
}

export interface DownloadUrlResponse {
  url: string;
  expiresAt: string;
  filename: string;
}

export const artifactsApi = {
  // List artifacts in a repository with optional path
  listArtifacts: async (
    repositoryId: string,
    path?: string,
    filters?: Omit<ArtifactFilters, 'repositoryId' | 'path'>
  ): Promise<PaginatedResponse<Artifact>> => {
    const params = new URLSearchParams();
    
    if (path) {
      params.append('path', path);
    }
    
    if (filters?.type) {
      params.append('type', filters.type);
    }
    if (filters?.search) {
      params.append('search', filters.search);
    }
    if (filters?.mimeType) {
      params.append('mimeType', filters.mimeType);
    }
    if (filters?.minSize) {
      params.append('minSize', filters.minSize.toString());
    }
    if (filters?.maxSize) {
      params.append('maxSize', filters.maxSize.toString());
    }
    if (filters?.createdAfter) {
      params.append('createdAfter', filters.createdAfter);
    }
    if (filters?.createdBefore) {
      params.append('createdBefore', filters.createdBefore);
    }
    if (filters?.page) {
      params.append('page', filters.page.toString());
    }
    if (filters?.limit) {
      params.append('limit', filters.limit.toString());
    }
    if (filters?.sortBy) {
      params.append('sortBy', filters.sortBy);
    }
    if (filters?.sortOrder) {
      params.append('sortOrder', filters.sortOrder);
    }

    return apiService.get<PaginatedResponse<Artifact>>(
      `/repositories/${repositoryId}/artifacts?${params.toString()}`
    );
  },

  // Browse directory structure
  browseDirectory: async (
    repositoryId: string,
    path: string = '/'
  ): Promise<ApiResponse<DirectoryListing>> => {
    const params = new URLSearchParams();
    params.append('path', path);

    return apiService.get<ApiResponse<DirectoryListing>>(
      `/repositories/${repositoryId}/browse?${params.toString()}`
    );
  },

  // Get artifact details
  getArtifact: async (
    repositoryId: string,
    artifactId: string
  ): Promise<ApiResponse<Artifact>> => {
    return apiService.get<ApiResponse<Artifact>>(
      `/repositories/${repositoryId}/artifacts/${artifactId}`
    );
  },

  // Upload artifact
  uploadArtifact: async (
    repositoryId: string,
    path: string,
    file: File,
    metadata?: Record<string, any>,
    onProgress?: (progress: number) => void
  ): Promise<ApiResponse<UploadArtifactResponse>> => {
    const formData = new FormData();
    formData.append('file', file);
    formData.append('path', path);
    
    if (metadata) {
      formData.append('metadata', JSON.stringify(metadata));
    }

    return apiService.upload<ApiResponse<UploadArtifactResponse>>(
      `/repositories/${repositoryId}/artifacts/upload`,
      file,
      onProgress,
      {
        data: formData,
        params: { path },
      }
    );
  },

  // Generate download URL
  generateDownloadUrl: async (
    repositoryId: string,
    artifactId: string,
    filename?: string
  ): Promise<ApiResponse<DownloadUrlResponse>> => {
    const params = new URLSearchParams();
    if (filename) {
      params.append('filename', filename);
    }

    return apiService.post<ApiResponse<DownloadUrlResponse>>(
      `/repositories/${repositoryId}/artifacts/${artifactId}/download-url`,
      null,
      { params }
    );
  },

  // Download artifact directly
  downloadArtifact: async (
    repositoryId: string,
    artifactId: string,
    filename?: string
  ): Promise<void> => {
    const params = new URLSearchParams();
    if (filename) {
      params.append('filename', filename);
    }

    return apiService.download(
      `/repositories/${repositoryId}/artifacts/${artifactId}/download?${params.toString()}`,
      filename
    );
  },

  // Delete artifact
  deleteArtifact: async (
    repositoryId: string,
    artifactId: string
  ): Promise<ApiResponse<void>> => {
    return apiService.delete<ApiResponse<void>>(
      `/repositories/${repositoryId}/artifacts/${artifactId}`
    );
  },

  // Batch operations
  batchOperations: async (
    repositoryId: string,
    operations: BatchOperationRequest
  ): Promise<ApiResponse<BatchOperationResponse>> => {
    return apiService.post<ApiResponse<BatchOperationResponse>>(
      `/repositories/${repositoryId}/artifacts/batch`,
      operations
    );
  },

  // Search artifacts across all repositories
  searchArtifacts: async (
    query: string,
    filters?: Omit<ArtifactFilters, 'search'>
  ): Promise<PaginatedResponse<Artifact>> => {
    const params = new URLSearchParams();
    params.append('q', query);

    if (filters?.repositoryId) {
      params.append('repositoryId', filters.repositoryId);
    }
    if (filters?.type) {
      params.append('type', filters.type);
    }
    if (filters?.mimeType) {
      params.append('mimeType', filters.mimeType);
    }
    if (filters?.minSize) {
      params.append('minSize', filters.minSize.toString());
    }
    if (filters?.maxSize) {
      params.append('maxSize', filters.maxSize.toString());
    }
    if (filters?.createdAfter) {
      params.append('createdAfter', filters.createdAfter);
    }
    if (filters?.createdBefore) {
      params.append('createdBefore', filters.createdBefore);
    }
    if (filters?.page) {
      params.append('page', filters.page.toString());
    }
    if (filters?.limit) {
      params.append('limit', filters.limit.toString());
    }
    if (filters?.sortBy) {
      params.append('sortBy', filters.sortBy);
    }
    if (filters?.sortOrder) {
      params.append('sortOrder', filters.sortOrder);
    }

    return apiService.get<PaginatedResponse<Artifact>>(
      `/artifacts/search?${params.toString()}`
    );
  },

  // Get artifact metadata
  getArtifactMetadata: async (
    repositoryId: string,
    artifactId: string
  ): Promise<ApiResponse<Record<string, any>>> => {
    return apiService.get<ApiResponse<Record<string, any>>>(
      `/repositories/${repositoryId}/artifacts/${artifactId}/metadata`
    );
  },

  // Update artifact metadata
  updateArtifactMetadata: async (
    repositoryId: string,
    artifactId: string,
    metadata: Record<string, any>
  ): Promise<ApiResponse<Artifact>> => {
    return apiService.patch<ApiResponse<Artifact>>(
      `/repositories/${repositoryId}/artifacts/${artifactId}/metadata`,
      metadata
    );
  },

  // Get artifact statistics
  getArtifactStats: async (
    repositoryId: string,
    artifactId: string
  ): Promise<
    ApiResponse<{
      downloadCount: number;
      lastDownloaded?: string;
      averageDownloadSize: number;
      uniqueDownloaders: number;
    }>
  > => {
    return apiService.get<
      ApiResponse<{
        downloadCount: number;
        lastDownloaded?: string;
        averageDownloadSize: number;
        uniqueDownloaders: number;
      }>
    >(`/repositories/${repositoryId}/artifacts/${artifactId}/stats`);
  },

  // Validate artifact name/path
  validateArtifactPath: async (
    repositoryId: string,
    path: string
  ): Promise<ApiResponse<{ valid: boolean; message?: string }>> => {
    return apiService.post<ApiResponse<{ valid: boolean; message?: string }>>(
      `/repositories/${repositoryId}/artifacts/validate-path`,
      { path }
    );
  },
};