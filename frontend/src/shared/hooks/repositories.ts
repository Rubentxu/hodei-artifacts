import { useQuery } from '@tanstack/react-query';
import { useState, useCallback } from 'react';
import type {
  RepositoryFilters,
  PaginatedResponse,
  Repository,
} from '@/shared/types';
import { QUERY_KEYS } from '@/shared/constants';

// Mock API functions - replace with actual API calls
const fetchRepositories = async (
  filters: RepositoryFilters
): Promise<PaginatedResponse<Repository>> => {
  // Simulate API delay
  await new Promise(resolve => setTimeout(resolve, 500));

  // Mock data
  const mockRepositories: Repository[] = [
    {
      id: '1',
      name: 'maven-central',
      description: 'Maven Central Repository',
      type: 'maven',
      visibility: 'public',
      isPublic: true,
      packageCount: 150,
      size: 1024 * 1024 * 100, // 100MB
      lastUpdated: new Date().toISOString(),
      url: 'https://repo1.maven.org/maven2',
    },
    {
      id: '2',
      name: 'npm-registry',
      description: 'npm Registry',
      type: 'npm',
      visibility: 'public',
      isPublic: true,
      packageCount: 250,
      size: 1024 * 1024 * 200, // 200MB
      lastUpdated: new Date().toISOString(),
      url: 'https://registry.npmjs.org',
    },
  ];

  return {
    data: mockRepositories,
    total: mockRepositories.length,
    page: filters.page || 1,
    limit: filters.limit || 10,
    hasNext: false,
    hasPrev: false,
  };
};

const fetchRepository = async (id: string): Promise<{ data: Repository }> => {
  // Simulate API delay
  await new Promise(resolve => setTimeout(resolve, 300));

  // Mock data
  const mockRepository: Repository = {
    id,
    name: 'maven-central',
    description: 'Maven Central Repository',
    type: 'maven',
    visibility: 'public',
    isPublic: true,
    packageCount: 150,
    size: 1024 * 1024 * 100, // 100MB
    lastUpdated: new Date().toISOString(),
    url: 'https://repo1.maven.org/maven2',
  };

  return { data: mockRepository };
};

export const useRepositories = (filters: RepositoryFilters) => {
  return useQuery({
    queryKey: QUERY_KEYS.REPOSITORIES,
    queryFn: () => fetchRepositories(filters),
  });
};

export const useRepository = (id: string) => {
  return useQuery({
    queryKey: QUERY_KEYS.REPOSITORY(id),
    queryFn: () => fetchRepository(id),
    enabled: !!id,
  });
};

export const useRepositoryFilters = (initialFilters: RepositoryFilters) => {
  const [filters, setFilters] = useState<RepositoryFilters>(initialFilters);

  const updateFilter = useCallback(
    <K extends keyof RepositoryFilters>(
      key: K,
      value: RepositoryFilters[K]
    ) => {
      setFilters(prev => ({ ...prev, [key]: value }));
    },
    []
  );

  const clearFilters = useCallback(() => {
    setFilters(initialFilters);
  }, [initialFilters]);

  return {
    filters,
    updateFilter,
    clearFilters,
  };
};
