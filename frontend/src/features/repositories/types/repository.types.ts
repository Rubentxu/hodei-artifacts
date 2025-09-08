export type RepositoryType = 'maven' | 'npm' | 'pypi' | 'docker';

export interface Repository {
  id: string;
  name: string;
  description: string;
  type: RepositoryType;
  isPublic: boolean;
  url: string;
  packageCount: number;
  size: number;
  lastUpdated: string;
  createdAt: string;
  updatedAt: string;
  configuration: Record<string, any>;
}

export interface NewRepository {
  name: string;
  description: string;
  type: RepositoryType;
  isPublic: boolean;
  configuration: Record<string, any>;
}

export interface UpdateRepository {
  name?: string;
  description?: string;
  isPublic?: boolean;
  configuration?: Record<string, any>;
}

export interface RepositoryFilters {
  type?: RepositoryType;
  isPublic?: boolean;
  search?: string;
}
