import { apiService } from './client';
import type {
  Repository,
  ApiResponse,
  PaginatedResponse,
  CreateRepositoryRequest,
  UpdateRepositoryRequest,
  RepositoryFilters,
  DashboardData,
  ActivityEvent,
} from '@/shared/types';

export const repositoriesApi = {
  // Get all repositories with optional filters
  getRepositories: async (
    filters?: RepositoryFilters
  ): Promise<PaginatedResponse<Repository>> => {
    const params = new URLSearchParams();

    if (filters?.type?.length) {
      filters.type.forEach(type => params.append('type', type));
    }
    if (filters?.visibility?.length) {
      filters.visibility.forEach(visibility =>
        params.append('visibility', visibility)
      );
    }
    if (filters?.search) {
      params.append('search', filters.search);
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

    return apiService.get<PaginatedResponse<Repository>>(
      `/repositories?${params.toString()}`
    );
  },

  // Get a specific repository by ID
  getRepository: async (id: string): Promise<ApiResponse<Repository>> => {
    return apiService.get<ApiResponse<Repository>>(`/repositories/${id}`);
  },

  // Create a new repository
  createRepository: async (
    data: CreateRepositoryRequest
  ): Promise<ApiResponse<Repository>> => {
    return apiService.post<ApiResponse<Repository>>('/repositories', data);
  },

  // Update an existing repository
  updateRepository: async (
    id: string,
    data: UpdateRepositoryRequest
  ): Promise<ApiResponse<Repository>> => {
    return apiService.patch<ApiResponse<Repository>>(
      `/repositories/${id}`,
      data
    );
  },

  // Delete a repository
  deleteRepository: async (id: string): Promise<ApiResponse<void>> => {
    return apiService.delete<ApiResponse<void>>(`/repositories/${id}`);
  },

  // Get dashboard data
  getDashboardData: async (): Promise<ApiResponse<DashboardData>> => {
    return apiService.get<ApiResponse<DashboardData>>('/dashboard');
  },

  // Get repository activity
  getRepositoryActivity: async (
    repositoryId: string,
    limit?: number
  ): Promise<ApiResponse<ActivityEvent[]>> => {
    const params = new URLSearchParams();
    if (limit) {
      params.append('limit', limit.toString());
    }
    return apiService.get<ApiResponse<ActivityEvent[]>>(
      `/repositories/${repositoryId}/activity?${params.toString()}`
    );
  },

  // Validate repository name
  validateRepositoryName: async (
    name: string
  ): Promise<ApiResponse<{ valid: boolean; message?: string }>> => {
    return apiService.post<ApiResponse<{ valid: boolean; message?: string }>>(
      '/repositories/validate-name',
      { name }
    );
  },

  // Get repository statistics
  getRepositoryStats: async (
    id: string
  ): Promise<
    ApiResponse<{
      packageCount: number;
      totalSize: number;
      lastUpdated: string;
      downloadCount: number;
      uploadCount: number;
    }>
  > => {
    return apiService.get<
      ApiResponse<{
        packageCount: number;
        totalSize: number;
        lastUpdated: string;
        downloadCount: number;
        uploadCount: number;
      }>
    >(`/repositories/${id}/stats`);
  },
};
