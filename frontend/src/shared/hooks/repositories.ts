// Forcing a reload to clear Vite cache
import { useEffect, useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { repositoriesApi } from '@/shared/api/repositories';
import type {
  Repository,
  RepositoryFilters,
  UpdateRepositoryRequest,
} from '@/shared/types';

// Query keys for React Query cache
const REPOSITORIES_KEYS = {
  all: ['repositories'] as const,
  lists: () => [...REPOSITORIES_KEYS.all, 'list'] as const,
  list: (filters: RepositoryFilters) =>
    [...REPOSITORIES_KEYS.lists(), filters] as const,
  details: () => [...REPOSITORIES_KEYS.all, 'detail'] as const,
  detail: (id: string) => [...REPOSITORIES_KEYS.details(), id] as const,
  dashboard: ['dashboard'] as const,
};

export const useRepositories = (filters?: RepositoryFilters) => {
  return useQuery({
    queryKey: REPOSITORIES_KEYS.list(filters || {}),
    queryFn: () => repositoriesApi.getRepositories(filters),
    staleTime: 5 * 60 * 1000, // 5 minutes
    gcTime: 10 * 60 * 1000, // 10 minutes
  });
};

export const useRepository = (id: string) => {
  return useQuery({
    queryKey: REPOSITORIES_KEYS.detail(id),
    queryFn: () => repositoriesApi.getRepository(id),
    enabled: !!id,
    staleTime: 2 * 60 * 1000, // 2 minutes
  });
};

export const useDashboardData = (limit: number = 5) => {
  const [isLoading, setIsLoading] = useState(true);

  const mockData = {
    data: {
      metrics: {
        totalPackages: 1250,
        activeRepositories: 42,
        onlineUsers: 7,
        storageUsed: { value: 128, unit: 'GB' },
      },
      recentRepositories: [
        {
          id: '1',
          name: 'hodei-main',
          type: 'maven',
          packageCount: 120,
          size: 54321098,
          lastUpdated: new Date().toISOString(),
          visibility: 'public',
          isPublic: true,
          url: '',
        },
        {
          id: '2',
          name: 'project-hermes',
          type: 'npm',
          packageCount: 45,
          size: 2210987,
          lastUpdated: new Date().toISOString(),
          visibility: 'private',
          isPublic: false,
          url: '',
        },
        {
          id: '3',
          name: 'data-pipelines',
          type: 'pypi',
          packageCount: 88,
          size: 12345678,
          lastUpdated: new Date().toISOString(),
          visibility: 'public',
          isPublic: true,
          url: '',
        },
      ].slice(0, limit),
      recentActivity: [
        {
          id: '1',
          type: 'upload',
          userName: 'Ruben',
          targetName: 'hodei-main/artifact-1.0.jar',
          timestamp: new Date().toISOString(),
          targetType: 'artifact',
          targetId: '1',
        },
        {
          id: '2',
          type: 'create',
          userName: 'Alice',
          targetName: 'new-frontend-repo',
          timestamp: new Date().toISOString(),
          targetType: 'repository',
          targetId: '4',
        },
        {
          id: '3',
          type: 'download',
          userName: 'Bob',
          targetName: 'project-hermes/react-18.tgz',
          timestamp: new Date().toISOString(),
          targetType: 'artifact',
          targetId: '2',
        },
      ].slice(0, limit),
    },
    success: true,
  };

  useEffect(() => {
    const timer = setTimeout(() => {
      setIsLoading(false);
    }, 1500); // Simulate a 1.5 second network delay
    return () => clearTimeout(timer);
  }, []);

  return {
    data: isLoading ? undefined : mockData,
    isLoading,
    error: null,
  };
};

export const useCreateRepository = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: repositoriesApi.createRepository,
    onSuccess: () => {
      // Invalidate all repositories lists
      queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEYS.lists() });
      // Invalidate dashboard data
      queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEYS.dashboard });
    },
  });
};

export const useUpdateRepository = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateRepositoryRequest }) =>
      repositoriesApi.updateRepository(id, data),
    onSuccess: (_, variables) => {
      // Invalidate the specific repository
      queryClient.invalidateQueries({
        queryKey: REPOSITORIES_KEYS.detail(variables.id),
      });
      // Invalidate all repositories lists
      queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEYS.lists() });
    },
  });
};

export const useDeleteRepository = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: repositoriesApi.deleteRepository,
    onSuccess: (_, id) => {
      // Remove the specific repository from cache
      queryClient.removeQueries({ queryKey: REPOSITORIES_KEYS.detail(id) });
      // Invalidate all repositories lists
      queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEYS.lists() });
      // Invalidate dashboard data
      queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEYS.dashboard });
    },
  });
};

export const useRepositoryActivity = (repositoryId: string, limit?: number) => {
  return useQuery({
    queryKey: ['repository', repositoryId, 'activity', limit],
    queryFn: () => repositoriesApi.getRepositoryActivity(repositoryId, limit),
    enabled: !!repositoryId,
    staleTime: 60 * 1000, // 1 minute
  });
};

export const useRepositoryStats = (id: string) => {
  return useQuery({
    queryKey: ['repository', id, 'stats'],
    queryFn: () => repositoriesApi.getRepositoryStats(id),
    enabled: !!id,
    staleTime: 2 * 60 * 1000, // 2 minutes
  });
};

export const useValidateRepositoryName = () => {
  return useMutation({
    mutationFn: repositoriesApi.validateRepositoryName,
  });
};

// Optimistic updates for better UX
export const useOptimisticRepositories = () => {
  const queryClient = useQueryClient();

  const addRepository = (newRepository: Repository) => {
    queryClient.setQueryData<Repository[]>(
      REPOSITORIES_KEYS.lists(),
      (old = []) => [newRepository, ...old]
    );
  };

  const updateRepository = (updatedRepository: Repository) => {
    queryClient.setQueryData<Repository[]>(
      REPOSITORIES_KEYS.lists(),
      (old = []) =>
        old.map(repo =>
          repo.id === updatedRepository.id ? updatedRepository : repo
        )
    );

    queryClient.setQueryData(
      REPOSITORIES_KEYS.detail(updatedRepository.id),
      updatedRepository
    );
  };

  const removeRepository = (id: string) => {
    queryClient.setQueryData<Repository[]>(
      REPOSITORIES_KEYS.lists(),
      (old = []) => old.filter(repo => repo.id !== id)
    );

    queryClient.removeQueries({ queryKey: REPOSITORIES_KEYS.detail(id) });
  };

  return { addRepository, updateRepository, removeRepository };
};

// Hook for repository filters state
export const useRepositoryFilters = (
  initialFilters: RepositoryFilters = {}
) => {
  const [filters, setFilters] = useState<RepositoryFilters>({
    search: undefined,
    type: undefined,
    visibility: undefined,
    status: undefined,
    page: undefined,
    limit: undefined,
    sortBy: undefined,
    sortOrder: undefined,
    ...initialFilters,
  });

  const updateFilter = (key: keyof RepositoryFilters, value: any) => {
    setFilters((prev: RepositoryFilters) => ({
      ...prev,
      [key]: value,
      page: 1, // Reset to first page when filters change
    }));
  };

  const clearFilters = () => {
    setFilters(initialFilters);
  };

  return {
    filters,
    setFilters,
    updateFilter,
    clearFilters,
  };
};
