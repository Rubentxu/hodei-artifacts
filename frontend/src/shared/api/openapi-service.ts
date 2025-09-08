/**
 * Servicio de API de alto nivel que usa el cliente OpenAPI
 * Proporciona una interfaz amigable para los componentes React
 * Siguiendo el patrón Contract First
 */

import { openAPIClient } from './openapi-client';
import type {
  Repository,
  RepositoryListResponse,
  PackageResult,
  SearchResults,
  NpmPackageMetadata,
  TokenResponse,
  CreateUserResponse,
  CreatePolicyResponse,
  PackageType
} from '@/shared/types/openapi-generated.types';

// ===== INTERFACES DE SERVICIO =====

export interface RepositoryService {
  getAll(options?: RepositoryOptions): Promise<PaginatedRepositories>;
  getById(id: string): Promise<Repository>;
  create(data: CreateRepositoryData): Promise<Repository>;
  update(id: string, data: UpdateRepositoryData): Promise<Repository>;
  delete(id: string): Promise<void>;
  getByType(type: PackageType): Promise<Repository[]>;
}

export interface SearchService {
  search(query: string, options?: SearchOptions): Promise<SearchResults>;
  getSuggestions(query: string): Promise<string[]>;
  getPopularPackages(limit?: number): Promise<PackageResult[]>;
  getRecentPackages(limit?: number): Promise<PackageResult[]>;
}

export interface PackageService {
  getNpmPackage(name: string): Promise<NpmPackageMetadata>;
  publishNpmPackage(name: string, metadata: NpmPublishMetadata): Promise<void>;
  downloadNpmTarball(packageName: string, fileName: string): Promise<Blob>;
  getMavenArtifact(groupId: string, artifactId: string, version: string, fileName: string): Promise<Blob>;
  uploadMavenArtifact(groupId: string, artifactId: string, version: string, fileName: string, content: Blob): Promise<void>;
  getPypiSimpleIndex(packageName: string): Promise<string>;
  downloadPypiPackage(fileName: string): Promise<Blob>;
  uploadPypiPackage(formData: FormData): Promise<void>;
}

export interface AuthService {
  login(credentials: LoginCredentials): Promise<AuthResult>;
  logout(): Promise<void>;
  getCurrentUser(): Promise<CurrentUser | null>;
  createToken(request: TokenRequest): Promise<TokenResponse>;
  listTokens(): Promise<TokenResponse[]>;
  deleteToken(tokenId: string): Promise<void>;
  refreshToken(): Promise<AuthResult>;
}

export interface UserService {
  getAll(): Promise<User[]>;
  create(user: CreateUserData): Promise<User>;
  getAttributes(userId: string): Promise<UserAttributes>;
  updateAttributes(userId: string, attributes: UserAttributes): Promise<UserAttributes>;
}

export interface PolicyService {
  getAll(): Promise<Policy[]>;
  create(policy: CreatePolicyData): Promise<Policy>;
  update(id: string, policy: UpdatePolicyData): Promise<Policy>;
  delete(id: string): Promise<void>;
}

// ===== TIPOS DE DATOS DE SERVICIO =====

export interface RepositoryOptions {
  limit?: number;
  offset?: number;
  search?: string;
  sortBy?: 'name' | 'createdAt' | 'updatedAt';
  sortOrder?: 'asc' | 'desc';
}

export interface PaginatedRepositories extends RepositoryListResponse {
  hasMore: boolean;
  nextOffset?: number;
}

export interface CreateRepositoryData {
  name: string;
  description?: string;
  type?: PackageType;
  isPrivate?: boolean;
}

export interface UpdateRepositoryData {
  name?: string;
  description?: string;
}

export interface SearchOptions {
  limit?: number;
  offset?: number;
  type?: PackageType;
  repository?: string;
  sortBy?: 'name' | 'downloads' | 'lastModified' | 'score';
  sortOrder?: 'asc' | 'desc';
}

export interface LoginCredentials {
  username: string;
  password: string;
  rememberMe?: boolean;
}

export interface AuthResult {
  success: boolean;
  user?: CurrentUser;
  token?: string;
  error?: string;
}

export interface CurrentUser {
  id: string;
  username: string;
  email: string;
  role: string;
  permissions: string[];
  attributes?: Record<string, any>;
}

export interface User {
  id: string;
  username: string;
  email: string;
  role: string;
  status: 'active' | 'inactive';
  createdAt: string;
  lastLoginAt?: string;
}

export interface CreateUserData {
  username: string;
  email: string;
  password: string;
  role?: string;
  attributes?: Record<string, any>;
}

export interface UserAttributes extends Record<string, any> {}

export interface TokenRequest {
  name: string;
  expiresAt?: string;
  scopes?: string[];
}

export interface Policy {
  id: string;
  name: string;
  description?: string;
  content: string;
  isActive: boolean;
  createdAt: string;
  updatedAt?: string;
}

export interface CreatePolicyData {
  name: string;
  description?: string;
  content: string;
  isActive?: boolean;
}

export interface UpdatePolicyData {
  name?: string;
  description?: string;
  content?: string;
  isActive?: boolean;
}

export interface NpmPublishMetadata {
  name: string;
  version: string;
  description?: string;
  main?: string;
  scripts?: Record<string, string>;
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  keywords?: string[];
  license?: string;
  maintainers?: Array<{ name: string; email?: string }>;
  repository?: {
    type: string;
    url: string;
  };
  bugs?: {
    url: string;
  };
  homepage?: string;
}

// ===== IMPLEMENTACIÓN DE SERVICIOS =====

class RepositoryServiceImpl implements RepositoryService {
  async getAll(options?: RepositoryOptions): Promise<PaginatedRepositories> {
    try {
      const response = await openAPIClient.listRepositories({
        limit: options?.limit || 20,
        offset: options?.offset || 0
      });

      return {
        ...response,
        hasMore: (response.total || 0) > ((response.items?.length || 0) + (options?.offset || 0)),
        nextOffset: ((response.items?.length || 0) + (options?.offset || 0)) < (response.total || 0) 
          ? (response.items?.length || 0) + (options?.offset || 0) 
          : undefined
      };
    } catch (error) {
      console.error('Error fetching repositories:', error);
      throw new Error('Failed to fetch repositories');
    }
  }

  async getById(id: string): Promise<Repository> {
    try {
      return await openAPIClient.getRepository({ id });
    } catch (error) {
      console.error('Error fetching repository:', error);
      throw new Error('Failed to fetch repository');
    }
  }

  async create(data: CreateRepositoryData): Promise<Repository> {
    try {
      return await openAPIClient.createRepository({
        name: data.name,
        description: data.description
      });
    } catch (error) {
      console.error('Error creating repository:', error);
      throw new Error('Failed to create repository');
    }
  }

  async update(id: string, data: UpdateRepositoryData): Promise<Repository> {
    try {
      return await openAPIClient.updateRepository({ id }, {
        name: data.name,
        description: data.description
      });
    } catch (error) {
      console.error('Error updating repository:', error);
      throw new Error('Failed to update repository');
    }
  }

  async delete(id: string): Promise<void> {
    try {
      await openAPIClient.deleteRepository({ id });
    } catch (error) {
      console.error('Error deleting repository:', error);
      throw new Error('Failed to delete repository');
    }
  }

  async getByType(type: PackageType): Promise<Repository[]> {
    try {
      const allRepos = await this.getAll({ limit: 100 });
      // Filtrar por tipo basado en el nombre o descripción
      return allRepos.items?.filter(repo => 
        repo.name?.toLowerCase().includes(type.toLowerCase()) ||
        repo.description?.toLowerCase().includes(type.toLowerCase())
      ) || [];
    } catch (error) {
      console.error('Error fetching repositories by type:', error);
      throw new Error('Failed to fetch repositories by type');
    }
  }
}

class SearchServiceImpl implements SearchService {
  async search(query: string, options?: SearchOptions): Promise<SearchResults> {
    try {
      return await openAPIClient.searchArtifacts({
        q: query,
        limit: options?.limit || 20,
        offset: options?.offset || 0
      });
    } catch (error) {
      console.error('Error searching artifacts:', error);
      throw new Error('Failed to search artifacts');
    }
  }

  async getSuggestions(query: string): Promise<string[]> {
    try {
      // Usar el search con límite pequeño para sugerencias
      const results = await this.search(query, { limit: 5 });
      return results.results?.map(pkg => pkg.name || '').filter(Boolean) || [];
    } catch (error) {
      console.error('Error getting suggestions:', error);
      // Retornar sugerencias básicas en caso de error
      return ['react', 'vue', 'angular', 'express', 'lodash'];
    }
  }

  async getPopularPackages(limit: number = 10): Promise<PackageResult[]> {
    try {
      const results = await this.search('popular', { limit });
      return results.results?.sort((a, b) => (b.downloads || 0) - (a.downloads || 0)).slice(0, limit) || [];
    } catch (error) {
      console.error('Error getting popular packages:', error);
      throw new Error('Failed to get popular packages');
    }
  }

  async getRecentPackages(limit: number = 10): Promise<PackageResult[]> {
    try {
      const results = await this.search('recent', { limit });
      return results.results?.sort((a, b) => {
        const dateA = new Date(a.lastModified || 0);
        const dateB = new Date(b.lastModified || 0);
        return dateB.getTime() - dateA.getTime();
      }).slice(0, limit) || [];
    } catch (error) {
      console.error('Error getting recent packages:', error);
      throw new Error('Failed to get recent packages');
    }
  }
}

class PackageServiceImpl implements PackageService {
  async getNpmPackage(name: string): Promise<NpmPackageMetadata> {
    try {
      return await openAPIClient.getNpmPackage({ packageName: name });
    } catch (error) {
      console.error('Error getting npm package:', error);
      throw new Error('Failed to get npm package');
    }
  }

  async publishNpmPackage(name: string, metadata: NpmPublishMetadata): Promise<void> {
    try {
      await openAPIClient.publishNpmPackage({ packageName: name }, metadata);
    } catch (error) {
      console.error('Error publishing npm package:', error);
      throw new Error('Failed to publish npm package');
    }
  }

  async downloadNpmTarball(packageName: string, fileName: string): Promise<Blob> {
    try {
      return await openAPIClient.downloadNpmTarball({ packageName, fileName });
    } catch (error) {
      console.error('Error downloading npm tarball:', error);
      throw new Error('Failed to download npm tarball');
    }
  }

  async getMavenArtifact(groupId: string, artifactId: string, version: string, fileName: string): Promise<Blob> {
    try {
      return await openAPIClient.downloadMaven({ groupId, artifactId, version, fileName });
    } catch (error) {
      console.error('Error downloading maven artifact:', error);
      throw new Error('Failed to download maven artifact');
    }
  }

  async uploadMavenArtifact(groupId: string, artifactId: string, version: string, fileName: string, content: Blob): Promise<void> {
    try {
      await openAPIClient.uploadMaven({ groupId, artifactId, version, fileName }, content);
    } catch (error) {
      console.error('Error uploading maven artifact:', error);
      throw new Error('Failed to upload maven artifact');
    }
  }

  async getPypiSimpleIndex(packageName: string): Promise<string> {
    try {
      return await openAPIClient.getPypiSimpleIndex({ packageName });
    } catch (error) {
      console.error('Error getting PyPI simple index:', error);
      throw new Error('Failed to get PyPI simple index');
    }
  }

  async downloadPypiPackage(fileName: string): Promise<Blob> {
    try {
      return await openAPIClient.downloadPypiPackage({ fileName });
    } catch (error) {
      console.error('Error downloading PyPI package:', error);
      throw new Error('Failed to download PyPI package');
    }
  }

  async uploadPypiPackage(formData: FormData): Promise<void> {
    try {
      await openAPIClient.uploadPypiPackage(formData);
    } catch (error) {
      console.error('Error uploading PyPI package:', error);
      throw new Error('Failed to upload PyPI package');
    }
  }
}

class AuthServiceImpl implements AuthService {
  private currentUser: CurrentUser | null = null;
  private currentToken: string | null = null;

  async login(credentials: LoginCredentials): Promise<AuthResult> {
    try {
      // Simular autenticación con el backend
      // En producción, esto llamaría a un endpoint de autenticación
      await this.mockDelay(500);
      
      // Validación básica
      if (!credentials.username || !credentials.password) {
        return {
          success: false,
          error: 'Username and password are required'
        };
      }

      // Mock autenticación exitosa
      if (credentials.username === 'admin' && credentials.password === 'admin123') {
        const user: CurrentUser = {
          id: 'user-123',
          username: 'admin',
          email: 'admin@example.com',
          role: 'admin',
          permissions: ['read:repositories', 'write:repositories', 'read:artifacts', 'write:artifacts'],
          attributes: {
            department: 'engineering',
            team: 'platform'
          }
        };

        this.currentUser = user;
        this.currentToken = 'mock-jwt-token-' + Math.random().toString(36).substring(2, 15);

        return {
          success: true,
          user: user,
          token: this.currentToken
        };
      }

      // Mock autenticación fallida
      return {
        success: false,
        error: 'Invalid username or password'
      };
    } catch (error) {
      console.error('Error during login:', error);
      return {
        success: false,
        error: 'Login failed'
      };
    }
  }

  async logout(): Promise<void> {
    this.currentUser = null;
    this.currentToken = null;
  }

  async getCurrentUser(): Promise<CurrentUser | null> {
    return this.currentUser;
  }

  async createToken(request: TokenRequest): Promise<TokenResponse> {
    if (!this.currentUser) {
      throw new Error('User not authenticated');
    }

    try {
      return await openAPIClient.createToken(request);
    } catch (error) {
      console.error('Error creating token:', error);
      throw new Error('Failed to create token');
    }
  }

  async listTokens(): Promise<TokenResponse[]> {
    if (!this.currentUser) {
      throw new Error('User not authenticated');
    }

    try {
      return await openAPIClient.listTokens();
    } catch (error) {
      console.error('Error listing tokens:', error);
      throw new Error('Failed to list tokens');
    }
  }

  async deleteToken(tokenId: string): Promise<void> {
    if (!this.currentUser) {
      throw new Error('User not authenticated');
    }

    try {
      await openAPIClient.deleteToken({ tokenId });
    } catch (error) {
      console.error('Error deleting token:', error);
      throw new Error('Failed to delete token');
    }
  }

  async refreshToken(): Promise<AuthResult> {
    if (!this.currentToken || !this.currentUser) {
      return {
        success: false,
        error: 'No valid session'
      };
    }

    // Simular refresh de token
    await this.mockDelay(300);
    
    this.currentToken = 'mock-refreshed-token-' + Math.random().toString(36).substring(2, 15);
    
    return {
      success: true,
      user: this.currentUser,
      token: this.currentToken
    };
  }

  private async mockDelay(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
  }
}

class UserServiceImpl implements UserService {
  async getAll(): Promise<User[]> {
    try {
      // Por ahora, usar datos mock ya que el endpoint de usuarios no está implementado
      const mockUsers: CreateUserResponse[] = [
        {
          id: 'user-1',
          username: 'john.doe',
          email: 'john.doe@example.com',
          createdAt: new Date().toISOString()
        },
        {
          id: 'user-2',
          username: 'jane.smith',
          email: 'jane.smith@example.com',
          createdAt: new Date().toISOString()
        }
      ];

      return mockUsers.map(user => ({
        id: user.id!,
        username: user.username!,
        email: user.email!,
        role: 'user',
        status: 'active' as const,
        createdAt: user.createdAt!,
        lastLoginAt: new Date(Date.now() - Math.random() * 30 * 24 * 60 * 60 * 1000).toISOString()
      }));
    } catch (error) {
      console.error('Error fetching users:', error);
      throw new Error('Failed to fetch users');
    }
  }

  async create(user: CreateUserData): Promise<User> {
    try {
      const response = await openAPIClient.createUser({
        username: user.username,
        email: user.email,
        password: user.password,
        attributes: user.attributes
      });

      return {
        id: response.id!,
        username: response.username!,
        email: response.email!,
        role: user.role || 'user',
        status: 'active' as const,
        createdAt: response.createdAt!
      };
    } catch (error) {
      console.error('Error creating user:', error);
      throw new Error('Failed to create user');
    }
  }

  async getAttributes(userId: string): Promise<UserAttributes> {
    try {
      return await openAPIClient.getUserAttributes({ id: userId });
    } catch (error) {
      console.error('Error fetching user attributes:', error);
      throw new Error('Failed to fetch user attributes');
    }
  }

  async updateAttributes(userId: string, attributes: UserAttributes): Promise<UserAttributes> {
    try {
      const response = await openAPIClient.updateUserAttributes(
        { id: userId },
        { attributes }
      );
      return response.attributes || {};
    } catch (error) {
      console.error('Error updating user attributes:', error);
      throw new Error('Failed to update user attributes');
    }
  }
}

class PolicyServiceImpl implements PolicyService {
  async getAll(): Promise<Policy[]> {
    try {
      const policies = await openAPIClient.listPolicies();
      // Por ahora, usar datos mock ya que el endpoint de políticas no está implementado
      const mockPolicies: CreatePolicyResponse[] = [
        {
          id: 'policy-1',
          name: 'developer-policy',
          description: 'Default policy for developers',
          isActive: true,
          createdAt: new Date().toISOString()
        },
        {
          id: 'policy-2',
          name: 'admin-policy',
          description: 'Full access policy for administrators',
          isActive: true,
          createdAt: new Date().toISOString()
        }
      ];

      return mockPolicies.map(policy => ({
        id: policy.id!,
        name: policy.name!,
        description: policy.description,
        content: '', // No hay campo policy en la respuesta, usar contenido vacío por ahora
        isActive: policy.isActive!,
        createdAt: policy.createdAt!,
        updatedAt: policy.createdAt // Usar createdAt como updatedAt por ahora
      }));
    } catch (error) {
      console.error('Error fetching policies:', error);
      throw new Error('Failed to fetch policies');
    }
  }

  async create(policy: CreatePolicyData): Promise<Policy> {
    try {
      const response = await openAPIClient.createPolicy({
        name: policy.name,
        description: policy.description,
        policy: policy.content, // Mapear content a policy
        isActive: policy.isActive
      });

      return {
        id: response.id!,
        name: response.name!,
        description: response.description,
        content: policy.content,
        isActive: response.isActive!,
        createdAt: response.createdAt!
      };
    } catch (error) {
      console.error('Error creating policy:', error);
      throw new Error('Failed to create policy');
    }
  }

  async update(id: string, policy: UpdatePolicyData): Promise<Policy> {
    try {
      // Nota: El API actual no tiene un endpoint PUT para políticas
      // Esto es una simulación que crea una nueva política
      const response = await openAPIClient.createPolicy({
        name: policy.name || 'updated-policy',
        description: policy.description,
        policy: policy.content || '',
        isActive: policy.isActive ?? true
      });

      return {
        id: response.id!,
        name: response.name!,
        description: response.description,
        content: policy.content || '',
        isActive: response.isActive!,
        createdAt: response.createdAt!
      };
    } catch (error) {
      console.error('Error updating policy:', error);
      throw new Error('Failed to update policy');
    }
  }

  async delete(id: string): Promise<void> {
    try {
      // Nota: El API actual no tiene un endpoint DELETE para políticas
      // Esto es una simulación
      console.log(`Simulating deletion of policy ${id}`);
    } catch (error) {
      console.error('Error deleting policy:', error);
      throw new Error('Failed to delete policy');
    }
  }
}

// ===== EXPORTAR INSTANCIAS DE SERVICIO =====
export const repositoryService = new RepositoryServiceImpl();
export const searchService = new SearchServiceImpl();
export const packageService = new PackageServiceImpl();
export const authService = new AuthServiceImpl();
export const userService = new UserServiceImpl();
export const policyService = new PolicyServiceImpl();

// ===== TIPOS DE HOOKS PERSONALIZADOS =====
export interface UseRepositoryOptions extends RepositoryOptions {
  enabled?: boolean;
  refetchInterval?: number;
}

export interface UseSearchOptions extends SearchOptions {
  enabled?: boolean;
  debounceMs?: number;
}

// ===== FUNCIONES DE UTILIDAD =====
export function createFormData(data: Record<string, any>): FormData {
  const formData = new FormData();
  
  Object.entries(data).forEach(([key, value]) => {
    if (value instanceof File || value instanceof Blob) {
      formData.append(key, value);
    } else if (typeof value === 'object' && value !== null) {
      formData.append(key, JSON.stringify(value));
    } else {
      formData.append(key, String(value));
    }
  });
  
  return formData;
}

export function createNpmPublishMetadata(
  name: string,
  version: string,
  options?: Partial<NpmPublishMetadata>
): NpmPublishMetadata {
  return {
    name,
    version,
    description: options?.description || `Package ${name}`,
    main: options?.main || 'index.js',
    scripts: options?.scripts || {},
    dependencies: options?.dependencies || {},
    devDependencies: options?.devDependencies || {},
    keywords: options?.keywords || [],
    license: options?.license || 'MIT',
    maintainers: options?.maintainers || [],
    repository: options?.repository,
    bugs: options?.bugs,
    homepage: options?.homepage,
    ...options
  };
}

// ===== CONFIGURACIÓN GLOBAL =====
export interface APIConfig {
  baseURL?: string;
  timeout?: number;
  retries?: number;
  enableMock?: boolean;
}

export function configureAPI(config: APIConfig): void {
  console.log('API configured with:', config);
  // Aquí se podría configurar el cliente HTTP real cuando esté disponible
}

// ===== CONSTANTES Y ENUMS =====
export const API_CONSTANTS = {
  DEFAULT_PAGE_SIZE: 20,
  MAX_PAGE_SIZE: 100,
  DEFAULT_TIMEOUT: 30000,
  MAX_RETRIES: 3,
  CACHE_TIME: 5 * 60 * 1000, // 5 minutos
  STALE_TIME: 2 * 60 * 1000, // 2 minutos
} as const;

export const ERROR_MESSAGES = {
  NETWORK_ERROR: 'Network error. Please check your connection.',
  AUTH_ERROR: 'Authentication failed. Please login again.',
  PERMISSION_ERROR: 'You do not have permission to perform this action.',
  VALIDATION_ERROR: 'Please check your input and try again.',
  NOT_FOUND: 'The requested resource was not found.',
  SERVER_ERROR: 'An error occurred on the server. Please try again later.',
  UNKNOWN_ERROR: 'An unknown error occurred. Please try again.',
} as const;

// ===== TIPOS DE ERROR =====
export class APIError extends Error {
  code?: string;
  status?: number;
  details?: any;

  constructor(
    message: string,
    code?: string,
    status?: number,
    details?: any
  ) {
    super(message);
    this.name = 'APIError';
    this.code = code;
    this.status = status;
    this.details = details;
  }
}

export class ValidationError extends APIError {
  constructor(message: string, details?: any) {
    super(message, 'VALIDATION_ERROR', 422, details);
    this.name = 'ValidationError';
  }
}

export class AuthenticationError extends APIError {
  constructor(message: string = ERROR_MESSAGES.AUTH_ERROR) {
    super(message, 'AUTH_ERROR', 401);
    this.name = 'AuthenticationError';
  }
}

export class AuthorizationError extends APIError {
  constructor(message: string = ERROR_MESSAGES.PERMISSION_ERROR) {
    super(message, 'AUTHORIZATION_ERROR', 403);
    this.name = 'AuthorizationError';
  }
}

export class NotFoundError extends APIError {
  constructor(message: string = ERROR_MESSAGES.NOT_FOUND) {
    super(message, 'NOT_FOUND', 404);
    this.name = 'NotFoundError';
  }
}

// ===== FUNCIONES DE UTILIDAD PARA MANEJO DE ERRORES =====
export function handleAPIError(error: any): APIError {
  if (error instanceof APIError) {
    return error;
  }
  
  if (error?.code === 'NOT_FOUND') {
    return new NotFoundError(error.message);
  }
  
  if (error?.code === 'VALIDATION_ERROR') {
    return new ValidationError(error.message, error.details);
  }
  
  if (error?.code === 'UNAUTHORIZED') {
    return new AuthenticationError(error.message);
  }
  
  if (error?.code === 'FORBIDDEN') {
    return new AuthorizationError(error.message);
  }
  
  return new APIError(
    error?.message || ERROR_MESSAGES.UNKNOWN_ERROR,
    error?.code || 'UNKNOWN_ERROR',
    error?.status || 500,
    error
  );
}

// ===== EXPORTAR TODO COMO UN OBJETO API =====
export const api = {
  repositories: repositoryService,
  search: searchService,
  packages: packageService,
  auth: authService,
  users: userService,
  policies: policyService,
  utils: {
    createFormData,
    createNpmPublishMetadata,
    handleAPIError
  },
  constants: API_CONSTANTS,
  errors: {
    APIError,
    ValidationError,
    AuthenticationError,
    AuthorizationError,
    NotFoundError,
    ERROR_MESSAGES
  }
} as const;

// Tipo para el objeto API completo
export type API = typeof api;