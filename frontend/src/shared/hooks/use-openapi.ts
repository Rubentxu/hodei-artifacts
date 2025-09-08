/**
 * Hooks personalizados para usar los servicios OpenAPI en componentes React
 * Siguiendo el patrón Contract First
 */

import { useState, useEffect, useCallback, useRef } from 'react';
import {
  useQuery,
  useMutation,
  useQueryClient,
  QueryClient,
} from '@tanstack/react-query';
import type {
  Repository,
  PackageResult,
  SearchResults,
  TokenResponse,
  CreateUserResponse,
  CreatePolicyResponse,
  PackageType,
  NpmPackageMetadata,
} from '@/shared/types/openapi-generated.types';
import {
  repositoryService,
  searchService,
  packageService,
  authService,
  userService,
  policyService,
  type RepositoryOptions,
  type SearchOptions,
  type CreateRepositoryData,
  type UpdateRepositoryData,
  type LoginCredentials,
  type AuthResult,
  type CurrentUser,
  type CreateUserData,
  type UserAttributes,
  type CreatePolicyData,
  type UpdatePolicyData,
  type NpmPublishMetadata,
  type TokenRequest,
  type User,
  type Policy,
} from '@/shared/api/openapi-service';
import { API_CONSTANTS, ERROR_MESSAGES } from '@/shared/api/openapi-service';

// ===== TIPOS DE HOOKS =====

export interface UseRepositoriesResult {
  repositories: Repository[];
  total: number;
  hasMore: boolean;
  nextOffset?: number;
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  refetch: () => void;
}

export interface UseRepositoryResult {
  repository: Repository | null;
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  refetch: () => void;
}

export interface UseSearchResult {
  results: PackageResult[];
  total: number;
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  refetch: () => void;
}

export interface UseAuthResult {
  user: CurrentUser | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  login: (credentials: LoginCredentials) => Promise<AuthResult>;
  logout: () => Promise<void>;
  refreshToken: () => Promise<AuthResult>;
}

export interface UseTokensResult {
  tokens: TokenResponse[];
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  createToken: (request: TokenRequest) => Promise<TokenResponse>;
  deleteToken: (tokenId: string) => Promise<void>;
  refetch: () => void;
}

export interface UseUsersResult {
  users: User[];
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  createUser: (user: CreateUserData) => Promise<User>;
  refetch: () => void;
}

export interface UsePoliciesResult {
  policies: Policy[];
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  createPolicy: (policy: CreatePolicyData) => Promise<Policy>;
  updatePolicy: (id: string, policy: UpdatePolicyData) => Promise<Policy>;
  deletePolicy: (id: string) => Promise<void>;
  refetch: () => void;
}

export interface UsePackageResult {
  package: NpmPackageMetadata | null;
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  refetch: () => void;
}

// ===== CONFIGURACIÓN DE REACT QUERY =====

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: API_CONSTANTS.STALE_TIME,
      gcTime: API_CONSTANTS.CACHE_TIME,
      retry: API_CONSTANTS.MAX_RETRIES,
      refetchOnWindowFocus: false,
    },
  },
});

// ===== HOOKS DE REPOSITORIOS =====

export function useRepositories(
  options?: RepositoryOptions & { enabled?: boolean }
) {
  return useQuery({
    queryKey: ['repositories', options],
    queryFn: () => repositoryService.getAll(options),
    enabled: options?.enabled !== false,
    staleTime: options?.enabled === false ? Infinity : API_CONSTANTS.STALE_TIME,
  });
}

export function useRepository(id: string, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['repository', id],
    queryFn: () => repositoryService.getById(id),
    enabled: options?.enabled !== false && !!id,
  });
}

export function useCreateRepository() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: CreateRepositoryData) => repositoryService.create(data),
    onSuccess: () => {
      // Invalidar caché de repositorios
      queryClient.invalidateQueries({ queryKey: ['repositories'] });
    },
  });
}

export function useUpdateRepository() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateRepositoryData }) =>
      repositoryService.update(id, data),
    onSuccess: (_, variables) => {
      // Invalidar caché del repositorio específico
      queryClient.invalidateQueries({ queryKey: ['repository', variables.id] });
      // Invalidar lista de repositorios
      queryClient.invalidateQueries({ queryKey: ['repositories'] });
    },
  });
}

export function useDeleteRepository() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => repositoryService.delete(id),
    onSuccess: () => {
      // Invalidar toda la caché de repositorios
      queryClient.invalidateQueries({ queryKey: ['repositories'] });
      queryClient.invalidateQueries({ queryKey: ['repository'] });
    },
  });
}

export function useRepositoriesByType(
  type: PackageType,
  options?: RepositoryOptions
) {
  return useQuery({
    queryKey: ['repositories', 'by-type', type, options],
    queryFn: () => repositoryService.getByType(type),
    enabled: !!type,
  });
}

// ===== HOOKS DE BÚSQUEDA =====

export function useSearch(
  query: string,
  options?: SearchOptions & { debounceMs?: number }
) {
  const debouncedQuery = useDebounce(query, options?.debounceMs || 300);

  return useQuery({
    queryKey: ['search', debouncedQuery, options],
    queryFn: () => searchService.search(debouncedQuery, options),
    enabled: debouncedQuery.length > 0,
  });
}

export function useSearchSuggestions(
  query: string,
  options?: { debounceMs?: number }
) {
  const debouncedQuery = useDebounce(query, options?.debounceMs || 300);

  return useQuery({
    queryKey: ['search-suggestions', debouncedQuery],
    queryFn: () => searchService.getSuggestions(debouncedQuery),
    enabled: debouncedQuery.length > 0,
  });
}

export function usePopularPackages(limit: number = 10) {
  return useQuery({
    queryKey: ['popular-packages', limit],
    queryFn: () => searchService.getPopularPackages(limit),
    staleTime: 5 * 60 * 1000, // 5 minutos
  });
}

export function useRecentPackages(limit: number = 10) {
  return useQuery({
    queryKey: ['recent-packages', limit],
    queryFn: () => searchService.getRecentPackages(limit),
    staleTime: 5 * 60 * 1000, // 5 minutos
  });
}

// ===== HOOKS DE AUTENTICACIÓN =====

export function useAuth() {
  const [user, setUser] = useState<CurrentUser | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    // Verificar si hay un usuario autenticado al cargar
    const checkAuth = async () => {
      try {
        const currentUser = await authService.getCurrentUser();
        setUser(currentUser);
      } catch (error) {
        console.error('Error checking authentication:', error);
      } finally {
        setIsLoading(false);
      }
    };

    checkAuth();
  }, []);

  const login = useCallback(
    async (credentials: LoginCredentials): Promise<AuthResult> => {
      try {
        const result = await authService.login(credentials);
        if (result.success && result.user) {
          setUser(result.user);
        }
        return result;
      } catch (error) {
        console.error('Error during login:', error);
        return {
          success: false,
          error: 'Login failed',
        };
      }
    },
    []
  );

  const logout = useCallback(async (): Promise<void> => {
    try {
      await authService.logout();
      setUser(null);
    } catch (error) {
      console.error('Error during logout:', error);
    }
  }, []);

  const refreshToken = useCallback(async (): Promise<AuthResult> => {
    try {
      const result = await authService.refreshToken();
      if (result.success && result.user) {
        setUser(result.user);
      }
      return result;
    } catch (error) {
      console.error('Error refreshing token:', error);
      return {
        success: false,
        error: 'Token refresh failed',
      };
    }
  }, []);

  return {
    user,
    isAuthenticated: !!user,
    isLoading,
    login,
    logout,
    refreshToken,
  };
}

export function useTokens() {
  return useQuery({
    queryKey: ['tokens'],
    queryFn: () => authService.listTokens(),
    enabled: !!authService.getCurrentUser(), // Solo si está autenticado
  });
}

export function useCreateToken() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (request: TokenRequest) => authService.createToken(request),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tokens'] });
    },
  });
}

export function useDeleteToken() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (tokenId: string) => authService.deleteToken(tokenId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['tokens'] });
    },
  });
}

// ===== HOOKS DE USUARIOS =====

export function useUsers() {
  return useQuery({
    queryKey: ['users'],
    queryFn: () => userService.getAll(),
    enabled: !!authService.getCurrentUser(), // Solo si está autenticado
  });
}

export function useCreateUser() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (user: CreateUserData) => userService.create(user),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['users'] });
    },
  });
}

export function useUserAttributes(userId: string) {
  return useQuery({
    queryKey: ['user-attributes', userId],
    queryFn: () => userService.getAttributes(userId),
    enabled: !!userId && !!authService.getCurrentUser(),
  });
}

export function useUpdateUserAttributes() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      userId,
      attributes,
    }: {
      userId: string;
      attributes: UserAttributes;
    }) => userService.updateAttributes(userId, attributes),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ['user-attributes', variables.userId],
      });
    },
  });
}

// ===== HOOKS DE POLÍTICAS =====

export function usePolicies() {
  return useQuery({
    queryKey: ['policies'],
    queryFn: () => policyService.getAll(),
    enabled: !!authService.getCurrentUser(), // Solo si está autenticado
  });
}

export function useCreatePolicy() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (policy: CreatePolicyData) => policyService.create(policy),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['policies'] });
    },
  });
}

export function useUpdatePolicy() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, policy }: { id: string; policy: UpdatePolicyData }) =>
      policyService.update(id, policy),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: ['policies'] });
    },
  });
}

export function useDeletePolicy() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => policyService.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['policies'] });
    },
  });
}

// ===== HOOKS DE PAQUETES =====

export function useNpmPackage(name: string, options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['npm-package', name],
    queryFn: () => packageService.getNpmPackage(name),
    enabled: options?.enabled !== false && !!name,
  });
}

export function usePublishNpmPackage() {
  return useMutation({
    mutationFn: ({
      name,
      metadata,
    }: {
      name: string;
      metadata: NpmPublishMetadata;
    }) => packageService.publishNpmPackage(name, metadata),
  });
}

export function useDownloadNpmTarball() {
  return useMutation({
    mutationFn: ({
      packageName,
      fileName,
    }: {
      packageName: string;
      fileName: string;
    }) => packageService.downloadNpmTarball(packageName, fileName),
  });
}

// ===== UTILIDADES =====

/**
 * Hook para debounce de valores
 */
export function useDebounce<T>(value: T, delay: number): T {
  const [debouncedValue, setDebouncedValue] = useState<T>(value);
  const timeoutRef = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
    }

    timeoutRef.current = setTimeout(() => {
      setDebouncedValue(value);
    }, delay);

    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, [value, delay]);

  return debouncedValue;
}

/**
 * Hook para manejar estados de carga de forma declarativa
 */
export function useLoadingState<T>(
  asyncFunction: () => Promise<T>,
  dependencies: any[] = []
): {
  data: T | null;
  isLoading: boolean;
  error: Error | null;
  execute: () => Promise<void>;
} {
  const [data, setData] = useState<T | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  const execute = useCallback(async () => {
    setIsLoading(true);
    setError(null);

    try {
      const result = await asyncFunction();
      setData(result);
    } catch (err) {
      setError(err instanceof Error ? err : new Error('Unknown error'));
    } finally {
      setIsLoading(false);
    }
  }, dependencies);

  return { data, isLoading, error, execute };
}

/**
 * Hook para paginación infinita
 */
export function useInfiniteQuery<T>(
  queryKey: string[],
  fetchFunction: (page: number) => Promise<{ items: T[]; hasMore: boolean }>,
  options?: { enabled?: boolean }
) {
  const [data, setData] = useState<T[]>([]);
  const [page, setPage] = useState(1);
  const [hasMore, setHasMore] = useState(true);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  const loadMore = useCallback(async () => {
    if (isLoading || !hasMore || options?.enabled === false) return;

    setIsLoading(true);
    setError(null);

    try {
      const result = await fetchFunction(page);
      setData(prev => [...prev, ...result.items]);
      setHasMore(result.hasMore);
      setPage(prev => prev + 1);
    } catch (err) {
      setError(
        err instanceof Error ? err : new Error('Failed to load more data')
      );
    } finally {
      setIsLoading(false);
    }
  }, [page, isLoading, hasMore, options?.enabled, fetchFunction]);

  const reset = useCallback(() => {
    setData([]);
    setPage(1);
    setHasMore(true);
    setError(null);
  }, []);

  return {
    data,
    isLoading,
    error,
    hasMore,
    loadMore,
    reset,
  };
}

// ===== EXPORTAR CONFIGURACIÓN DE REACT QUERY =====
export { queryClient };
export type { QueryClient };
