/**
 * Cliente API generado a partir del contrato OpenAPI 3.0.3
 * Implementa todos los endpoints del API con respuestas mockeadas
 * Siguiendo el patrón Contract First
 */

import type {
  Repository,
  RepositoryListResponse,
  CreateRepositoryRequest,
  UpdateRepositoryRequest,
  ArtifactUploadResponse,
  PresignedUrlResponse,
  SearchResults,
  PackageResult,
  NpmPackageMetadata,
  NpmPublishRequest,
  NpmPublishResponse,
  PythonPackageUpload,
  TokenRequest,
  TokenResponse,
  TokenInfo,
  CreateUserCommand,
  CreateUserResponse,
  UpdateUserAttributesCommand,
  UpdateUserAttributesResponse,
  CreatePolicyCommand,
  CreatePolicyResponse,
  UploadResponse,
  Error as ApiError,
  ListRepositoriesParams,
  RepositoryParams,
  SearchArtifactsParams,
  DownloadMavenParams,
  GetNpmPackageParams,
  PublishNpmBody,
  DownloadNpmTarballParams,
  GetPypiSimpleParams,
  DownloadPypiPackageParams,
  UploadPypiBody,
  ListTokensParams,
  TokenParams,
  ListUsersParams,
  UserAttributesParams,
  UpdateUserAttributesBody,
  ListPoliciesParams,
  CreatePolicyBody,
  PackageType,
  ArtifactStatus,
} from '@/shared/types/openapi-generated.types';

// ===== CONFIGURACIÓN DEL CLIENTE =====
const API_BASE_URL = 'http://localhost:8080/v2';
const API_VERSION = 'v2.1.0';

// Interfaz para el cliente API
export interface OpenAPIClient {
  // REPOSITORIES
  listRepositories(
    params?: ListRepositoriesParams
  ): Promise<RepositoryListResponse>;
  createRepository(body: CreateRepositoryRequest): Promise<Repository>;
  getRepository(params: RepositoryParams): Promise<Repository>;
  updateRepository(
    params: RepositoryParams,
    body: UpdateRepositoryRequest
  ): Promise<Repository>;
  deleteRepository(params: RepositoryParams): Promise<void>;

  // ARTIFACTS
  uploadArtifact(body: FormData): Promise<ArtifactUploadResponse>;
  getArtifact(params: {
    id: string;
    presigned?: boolean;
  }): Promise<Blob | PresignedUrlResponse>;

  // SEARCH
  searchArtifacts(params: SearchArtifactsParams): Promise<SearchResults>;

  // MAVEN REPOSITORY
  downloadMaven(params: DownloadMavenParams): Promise<Blob>;
  uploadMaven(params: DownloadMavenParams, body: Blob): Promise<UploadResponse>;

  // NPM REGISTRY
  getNpmPackage(params: GetNpmPackageParams): Promise<NpmPackageMetadata>;
  publishNpmPackage(
    params: GetNpmPackageParams,
    body: NpmPublishRequest
  ): Promise<NpmPublishResponse>;
  downloadNpmTarball(params: DownloadNpmTarballParams): Promise<Blob>;

  // PYPI REPOSITORY
  getPypiSimpleIndex(params: GetPypiSimpleParams): Promise<string>;
  downloadPypiPackage(params: DownloadPypiPackageParams): Promise<Blob>;
  uploadPypiPackage(body: FormData): Promise<UploadResponse>;

  // AUTHENTICATION
  listTokens(params?: ListTokensParams): Promise<TokenResponse[]>;
  createToken(body: TokenRequest): Promise<TokenResponse>;
  getToken(params: TokenParams): Promise<TokenInfo>;
  deleteToken(params: TokenParams): Promise<void>;

  // USERS
  listUsers(params?: ListUsersParams): Promise<CreateUserResponse[]>;
  createUser(body: CreateUserCommand): Promise<CreateUserResponse>;
  getUserAttributes(params: UserAttributesParams): Promise<Record<string, any>>;
  updateUserAttributes(
    params: UserAttributesParams,
    body: UpdateUserAttributesBody
  ): Promise<UpdateUserAttributesResponse>;

  // POLICIES
  listPolicies(params?: ListPoliciesParams): Promise<CreatePolicyResponse[]>;
  createPolicy(body: CreatePolicyBody): Promise<CreatePolicyResponse>;
}

// ===== IMPLEMENTACIÓN DEL CLIENTE CON MOCK CONTRACT FIRST =====

class OpenAPIClientImpl implements OpenAPIClient {
  private mockDelay = (ms: number) =>
    new Promise(resolve => setTimeout(resolve, ms));

  // ===== MÉTODOS AUXILIARES PARA MOCK DATA =====

  private generateId(): string {
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(
      /[xy]/g,
      function (c) {
        const r = (Math.random() * 16) | 0;
        const v = c === 'x' ? r : (r & 0x3) | 0x8;
        return v.toString(16);
      }
    );
  }

  private getCurrentTimestamp(): string {
    return new Date().toISOString();
  }

  // ===== REPOSITORIES =====

  async listRepositories(
    params?: ListRepositoriesParams
  ): Promise<RepositoryListResponse> {
    await this.mockDelay(200); // Simular latencia de red

    const limit = params?.limit || 20;
    const offset = params?.offset || 0;

    // Mock data basada en el contrato OpenAPI
    const mockRepositories: Repository[] = [
      {
        id: this.generateId(),
        name: 'npm-public',
        description: 'Public npm registry mirror',
        createdAt: this.getCurrentTimestamp(),
      },
      {
        id: this.generateId(),
        name: 'maven-central',
        description: 'Maven Central repository proxy',
        createdAt: this.getCurrentTimestamp(),
      },
      {
        id: this.generateId(),
        name: 'pypi-official',
        description: 'Official PyPI repository',
        createdAt: this.getCurrentTimestamp(),
      },
      {
        id: this.generateId(),
        name: 'private-npm',
        description: 'Private npm packages',
        createdAt: this.getCurrentTimestamp(),
      },
    ];

    return {
      total: mockRepositories.length,
      items: mockRepositories.slice(offset, offset + limit),
    };
  }

  async createRepository(body: CreateRepositoryRequest): Promise<Repository> {
    await this.mockDelay(300);

    // Validación básica según el contrato
    if (!body.name || body.name.trim().length === 0) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Repository name is required'
      );
    }

    // Simular conflicto si el nombre ya existe
    if (body.name === 'existing-repo') {
      throw this.createApiError('CONFLICT', 'Repository name already exists');
    }

    return {
      id: this.generateId(),
      name: body.name,
      description: body.description,
      createdAt: this.getCurrentTimestamp(),
    };
  }

  async getRepository(params: RepositoryParams): Promise<Repository> {
    await this.mockDelay(150);

    // Mock data para un repositorio específico
    const mockRepository: Repository = {
      id: params.id,
      name: 'example-repo',
      description: 'Example repository for testing',
      createdAt: '2024-01-15T10:30:00Z',
    };

    // Simular not found para ciertos IDs
    if (params.id === '00000000-0000-0000-0000-000000000000') {
      throw this.createApiError('NOT_FOUND', 'Repository not found');
    }

    return mockRepository;
  }

  async updateRepository(
    params: RepositoryParams,
    body: UpdateRepositoryRequest
  ): Promise<Repository> {
    await this.mockDelay(250);

    // Validación según contrato
    if (body.name && body.name.trim().length === 0) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Repository name cannot be empty'
      );
    }

    return {
      id: params.id,
      name: body.name || 'updated-repo',
      description: body.description || 'Updated description',
      createdAt: '2024-01-15T10:30:00Z',
    };
  }

  async deleteRepository(params: RepositoryParams): Promise<void> {
    await this.mockDelay(200);

    // Simular not found para ciertos IDs
    if (params.id === '00000000-0000-0000-0000-000000000000') {
      throw this.createApiError('NOT_FOUND', 'Repository not found');
    }

    // Éxito - no retorna contenido (204)
    return;
  }

  // ===== ARTIFACTS =====

  async uploadArtifact(body: FormData): Promise<ArtifactUploadResponse> {
    await this.mockDelay(500); // Upload lleva más tiempo

    // Simular validación de form data
    const file = body.get('file');
    const metadata = body.get('metadata');

    if (!file || !metadata) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'File and metadata are required'
      );
    }

    // Simular duplicado si el archivo ya existe
    const fileName = (file as File).name;
    if (fileName.includes('duplicate')) {
      return {
        id: this.generateId(),
        status: 'duplicate',
        repositoryId: this.generateId(),
      };
    }

    return {
      id: this.generateId(),
      status: 'accepted',
      repositoryId: this.generateId(),
    };
  }

  async getArtifact(params: {
    id: string;
    presigned?: boolean;
  }): Promise<Blob | PresignedUrlResponse> {
    await this.mockDelay(200);

    if (params.presigned) {
      // Retornar URL presignada
      return {
        url: `${API_BASE_URL}/artifacts/${params.id}/download?token=mock-presigned-token`,
        expiresAt: new Date(Date.now() + 3600000).toISOString(), // 1 hora
      };
    } else {
      // Retornar blob binario
      const mockContent = 'Mock artifact content for ID: ' + params.id;
      return new Blob([mockContent], { type: 'application/octet-stream' });
    }
  }

  // ===== SEARCH =====

  async searchArtifacts(params: SearchArtifactsParams): Promise<SearchResults> {
    await this.mockDelay(300);

    if (!params.q || params.q.trim().length === 0) {
      throw this.createApiError('VALIDATION_ERROR', 'Search query is required');
    }

    // Mock data de paquetes basada en la búsqueda
    const mockPackages: PackageResult[] = [
      {
        type: 'npm',
        name: 'react',
        latestVersion: '18.2.0',
        description: 'A JavaScript library for building user interfaces',
        downloadUrl: `${API_BASE_URL}/npm/react/-/react-18.2.0.tgz`,
        lastModified: this.getCurrentTimestamp(),
        downloads: 45234567,
        maintainers: ['facebook', 'react-team'],
        keywords: ['react', 'javascript', 'ui', 'frontend'],
        license: 'MIT',
        score: 0.95,
      },
      {
        type: 'maven',
        name: 'junit',
        latestVersion: '5.9.2',
        description: 'JUnit is a unit testing framework for Java',
        downloadUrl: `${API_BASE_URL}/maven2/junit/junit/5.9.2/junit-5.9.2.jar`,
        lastModified: this.getCurrentTimestamp(),
        downloads: 23456789,
        maintainers: ['junit-team'],
        keywords: ['testing', 'java', 'unit-test'],
        license: 'EPL-2.0',
        score: 0.88,
      },
      {
        type: 'pypi',
        name: 'requests',
        latestVersion: '2.28.2',
        description: 'Python HTTP for Humans.',
        downloadUrl: `${API_BASE_URL}/packages/requests-2.28.2-py3-none-any.whl`,
        lastModified: this.getCurrentTimestamp(),
        downloads: 32345678,
        maintainers: ['psf', 'kennethreitz'],
        keywords: ['http', 'python', 'api', 'rest'],
        license: 'Apache-2.0',
        score: 0.92,
      },
    ];

    // Filtrar por query
    const filteredPackages = mockPackages.filter(
      pkg =>
        pkg.name?.toLowerCase().includes(params.q.toLowerCase()) ||
        pkg.description?.toLowerCase().includes(params.q.toLowerCase())
    );

    const limit = params.limit || 20;
    const offset = params.offset || 0;

    return {
      total: filteredPackages.length,
      limit,
      offset,
      results: filteredPackages.slice(offset, offset + limit),
    };
  }

  // ===== MAVEN REPOSITORY =====

  async downloadMaven(params: DownloadMavenParams): Promise<Blob> {
    await this.mockDelay(400);

    // Validación de parámetros según el contrato
    const groupIdPattern = /^[a-zA-Z0-9._-]+$/;
    const artifactIdPattern = /^[a-zA-Z0-9._-]+$/;
    const versionPattern = /^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$/;

    if (!groupIdPattern.test(params.groupId)) {
      throw this.createApiError('VALIDATION_ERROR', 'Invalid groupId format');
    }
    if (!artifactIdPattern.test(params.artifactId)) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Invalid artifactId format'
      );
    }
    if (!versionPattern.test(params.version)) {
      throw this.createApiError('VALIDATION_ERROR', 'Invalid version format');
    }

    // Simular artefacto Maven
    const mockMavenContent = `// Mock Maven artifact
// GroupId: ${params.groupId}
// ArtifactId: ${params.artifactId}
// Version: ${params.version}
// File: ${params.fileName}
`;

    return new Blob([mockMavenContent], { type: 'application/java-archive' });
  }

  async uploadMaven(
    params: DownloadMavenParams,
    body: Blob
  ): Promise<UploadResponse> {
    await this.mockDelay(600);

    // Validaciones
    if (body.size === 0) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Artifact content is required'
      );
    }

    // Simular duplicado
    if (params.fileName.includes('duplicate')) {
      return {
        id: this.generateId(),
        status: 'duplicate',
        message: 'Artifact already exists',
        url: `${API_BASE_URL}/maven2/${params.groupId}/${params.artifactId}/${params.version}/${params.fileName}`,
      };
    }

    return {
      id: this.generateId(),
      status: 'accepted',
      message: 'Artifact uploaded successfully',
      url: `${API_BASE_URL}/maven2/${params.groupId}/${params.artifactId}/${params.version}/${params.fileName}`,
    };
  }

  // ===== NPM REGISTRY =====

  async getNpmPackage(
    params: GetNpmPackageParams
  ): Promise<NpmPackageMetadata> {
    await this.mockDelay(250);

    // Validación de nombre de paquete npm
    const npmPackagePattern =
      /^(@[a-z0-9-~][a-z0-9-._~]*)?[a-z0-9-~][a-z0-9-._~]*$/;
    if (!npmPackagePattern.test(params.packageName)) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Invalid npm package name format'
      );
    }

    // Mock metadata de paquete npm
    return {
      name: params.packageName,
      'dist-tags': {
        latest: '1.2.3',
        beta: '2.0.0-beta.1',
      },
      versions: {
        '1.2.3': {
          name: params.packageName,
          version: '1.2.3',
          description: `Mock package ${params.packageName}`,
          main: 'index.js',
          scripts: {
            test: 'jest',
            build: 'tsc',
          },
          dependencies: {
            lodash: '^4.17.21',
          },
          devDependencies: {
            jest: '^29.0.0',
            typescript: '^5.0.0',
          },
          keywords: ['mock', 'test'],
          license: 'MIT',
          maintainers: [
            {
              name: 'mock-user',
              email: 'mock@example.com',
            },
          ],
          dist: {
            tarball: `${API_BASE_URL}/${params.packageName}/-/package-1.2.3.tgz`,
            shasum: 'mock-shasum-12345',
            integrity: 'sha512-mock-integrity-hash',
          },
        },
      },
      time: {
        '1.2.3': '2024-01-15T10:30:00Z',
        '1.2.2': '2024-01-10T08:20:00Z',
      },
      maintainers: [
        {
          name: 'mock-maintainer',
          email: 'maintainer@example.com',
        },
      ],
      description: `Mock npm package ${params.packageName} for testing`,
      keywords: ['mock', 'test', 'npm'],
      license: 'MIT',
      repository: {
        type: 'git',
        url: `https://github.com/mock/${params.packageName}.git`,
      },
      bugs: {
        url: `https://github.com/mock/${params.packageName}/issues`,
      },
      homepage: `https://github.com/mock/${params.packageName}#readme`,
    };
  }

  async publishNpmPackage(
    params: GetNpmPackageParams,
    body: NpmPublishRequest
  ): Promise<NpmPublishResponse> {
    await this.mockDelay(400);

    // Validaciones según el contrato
    if (!body.name || !body.version) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Package name and version are required'
      );
    }

    // Simular conflicto si el paquete ya existe
    if (body.name.includes('existing')) {
      throw this.createApiError('CONFLICT', 'Package already exists');
    }

    return {
      success: true,
      id: this.generateId(),
      message: `Package ${body.name}@${body.version} published successfully`,
    };
  }

  async downloadNpmTarball(params: DownloadNpmTarballParams): Promise<Blob> {
    await this.mockDelay(300);

    // Validación de nombre de archivo
    const tarballPattern = /^.+\.tgz$/;
    if (!tarballPattern.test(params.fileName)) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Invalid tarball filename format'
      );
    }

    // Mock tarball content
    const mockTarballContent = `Mock npm tarball content for ${params.packageName}/${params.fileName}`;

    return new Blob([mockTarballContent], {
      type: 'application/gzip',
    });
  }

  // ===== PYPI REPOSITORY =====

  async getPypiSimpleIndex(params: GetPypiSimpleParams): Promise<string> {
    await this.mockDelay(200);

    // Mock PyPI simple index (HTML)
    return `<!DOCTYPE html>
<html>
  <head>
    <title>Links for ${params.packageName}</title>
  </head>
  <body>
    <h1>Links for ${params.packageName}</h1>
    <a href="${API_BASE_URL}/packages/${params.packageName}-1.2.3-py3-none-any.whl#sha256=mock-sha256-hash">${params.packageName}-1.2.3-py3-none-any.whl</a><br/>
    <a href="${API_BASE_URL}/packages/${params.packageName}-1.2.3.tar.gz#sha256=mock-sha256-hash">${params.packageName}-1.2.3.tar.gz</a><br/>
  </body>
</html>`;
  }

  async downloadPypiPackage(params: DownloadPypiPackageParams): Promise<Blob> {
    await this.mockDelay(350);

    // Mock wheel or sdist content
    const mockPackageContent = `Mock PyPI package content for ${params.fileName}`;

    return new Blob([mockPackageContent], {
      type: params.fileName.endsWith('.whl')
        ? 'application/octet-stream'
        : 'application/gzip',
    });
  }

  async uploadPypiPackage(body: FormData): Promise<UploadResponse> {
    await this.mockDelay(500);

    // Validación básica del form data
    const name = body.get('name');
    const version = body.get('version');
    const content = body.get('content');

    if (!name || !version || !content) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Name, version and content are required'
      );
    }

    return {
      id: this.generateId(),
      status: 'accepted',
      message: `Package ${name}@${version} uploaded successfully`,
      url: `${API_BASE_URL}/packages/${name}-${version}-py3-none-any.whl`,
    };
  }

  // ===== AUTHENTICATION =====

  async listTokens(params?: ListTokensParams): Promise<TokenResponse[]> {
    await this.mockDelay(200);

    return [
      {
        id: this.generateId(),
        name: 'development-token',
        token: 'mock-api-token-12345',
        expiresAt: new Date(
          Date.now() + 30 * 24 * 60 * 60 * 1000
        ).toISOString(), // 30 días
        scopes: ['read:repositories', 'write:artifacts'],
        createdAt: this.getCurrentTimestamp(),
      },
      {
        id: this.generateId(),
        name: 'ci-token',
        token: 'mock-api-token-67890',
        expiresAt: new Date(
          Date.now() + 90 * 24 * 60 * 60 * 1000
        ).toISOString(), // 90 días
        scopes: ['read:repositories', 'read:artifacts'],
        createdAt: this.getCurrentTimestamp(),
      },
    ];
  }

  async createToken(body: TokenRequest): Promise<TokenResponse> {
    await this.mockDelay(300);

    if (!body.name || body.name.trim().length === 0) {
      throw this.createApiError('VALIDATION_ERROR', 'Token name is required');
    }

    return {
      id: this.generateId(),
      name: body.name,
      token:
        'mock-generated-token-' + Math.random().toString(36).substring(2, 15),
      expiresAt:
        body.expiresAt ||
        new Date(Date.now() + 30 * 24 * 60 * 60 * 1000).toISOString(),
      scopes: body.scopes || ['read:repositories'],
      createdAt: this.getCurrentTimestamp(),
    };
  }

  async getToken(params: TokenParams): Promise<TokenInfo> {
    await this.mockDelay(150);

    return {
      id: params.tokenId,
      name: 'mock-token-name',
      expiresAt: new Date(Date.now() + 15 * 24 * 60 * 60 * 1000).toISOString(),
      scopes: ['read:repositories', 'write:artifacts'],
      createdAt: this.getCurrentTimestamp(),
      lastUsedAt: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(), // Hace 2 horas
    };
  }

  async deleteToken(params: TokenParams): Promise<void> {
    await this.mockDelay(200);

    // Simular not found para ciertos IDs
    if (params.tokenId === '00000000-0000-0000-0000-000000000000') {
      throw this.createApiError('NOT_FOUND', 'Token not found');
    }

    return;
  }

  // ===== USERS =====

  async listUsers(params?: ListUsersParams): Promise<CreateUserResponse[]> {
    await this.mockDelay(250);

    return [
      {
        id: this.generateId(),
        username: 'john.doe',
        email: 'john.doe@example.com',
        createdAt: this.getCurrentTimestamp(),
      },
      {
        id: this.generateId(),
        username: 'jane.smith',
        email: 'jane.smith@example.com',
        createdAt: this.getCurrentTimestamp(),
      },
    ];
  }

  async createUser(body: CreateUserCommand): Promise<CreateUserResponse> {
    await this.mockDelay(350);

    if (!body.username || !body.email || !body.password) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Username, email and password are required'
      );
    }

    // Validación básica de email
    const emailPattern = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
    if (!emailPattern.test(body.email)) {
      throw this.createApiError('VALIDATION_ERROR', 'Invalid email format');
    }

    return {
      id: this.generateId(),
      username: body.username,
      email: body.email,
      createdAt: this.getCurrentTimestamp(),
    };
  }

  async getUserAttributes(
    params: UserAttributesParams
  ): Promise<Record<string, any>> {
    await this.mockDelay(200);

    return {
      role: 'developer',
      department: 'engineering',
      team: 'platform',
      permissions: ['read:repositories', 'write:artifacts'],
    };
  }

  async updateUserAttributes(
    params: UserAttributesParams,
    body: UpdateUserAttributesBody
  ): Promise<UpdateUserAttributesResponse> {
    await this.mockDelay(300);

    return {
      id: params.id,
      username: 'john.doe',
      email: 'john.doe@example.com',
      attributes: body.attributes || {},
      updatedAt: this.getCurrentTimestamp(),
    };
  }

  // ===== POLICIES =====

  async listPolicies(
    params?: ListPoliciesParams
  ): Promise<CreatePolicyResponse[]> {
    await this.mockDelay(200);

    return [
      {
        id: this.generateId(),
        name: 'developer-policy',
        description: 'Default policy for developers',
        isActive: true,
        createdAt: this.getCurrentTimestamp(),
      },
      {
        id: this.generateId(),
        name: 'admin-policy',
        description: 'Full access policy for administrators',
        isActive: true,
        createdAt: this.getCurrentTimestamp(),
      },
    ];
  }

  async createPolicy(body: CreatePolicyBody): Promise<CreatePolicyResponse> {
    await this.mockDelay(400);

    if (!body.name || !body.policy) {
      throw this.createApiError(
        'VALIDATION_ERROR',
        'Policy name and content are required'
      );
    }

    return {
      id: this.generateId(),
      name: body.name,
      description: body.description,
      isActive: body.isActive ?? true,
      createdAt: this.getCurrentTimestamp(),
    };
  }

  // ===== MÉTODOS AUXILIARES =====

  private createApiError(code: string, message: string): ApiError {
    return {
      code,
      message,
      timestamp: this.getCurrentTimestamp(),
      success: false,
    } as ApiError;
  }
}

// ===== EXPORTAR INSTANCIA ÚNICA =====
export const openAPIClient = new OpenAPIClientImpl();

// ===== TIPOS DE UTILIDAD PARA EL CLIENTE =====
export interface OpenAPIResponse<T> {
  data: T;
  status: number;
  headers: Record<string, string>;
}

export interface OpenAPIError {
  error: ApiError;
  status: number;
  headers: Record<string, string>;
}

// ===== FUNCIONES DE UTILIDAD =====
export function isOpenAPIError(response: any): response is OpenAPIError {
  return response?.error && !response.data;
}

export function getErrorMessage(error: ApiError): string {
  return error.message || 'An unknown error occurred';
}

// ===== CONSTANTES DEL API =====
export const API_ENDPOINTS = {
  REPOSITORIES: '/v1/repositories',
  REPOSITORY_BY_ID: '/v1/repositories/:id',
  ARTIFACTS: '/v1/artifacts',
  ARTIFACT_BY_ID: '/v1/artifacts/:id',
  SEARCH: '/v1/search',
  MAVEN_DOWNLOAD: '/maven2/:groupId/:artifactId/:version/:fileName',
  MAVEN_UPLOAD: '/maven2/:groupId/:artifactId/:version/:fileName',
  NPM_PACKAGE: '/:packageName',
  NPM_TARBALL: '/:packageName/-/:fileName',
  PYPI_SIMPLE: '/simple/:packageName/',
  PYPI_PACKAGE: '/packages/:fileName',
  PYPI_UPLOAD: '/packages',
  TOKENS: '/auth/tokens',
  TOKEN_BY_ID: '/auth/tokens/:tokenId',
  USERS: '/v1/users',
  USER_ATTRIBUTES: '/v1/users/:id/attributes',
  POLICIES: '/v1/policies',
} as const;

export const API_SCOPES = {
  READ_REPOSITORIES: 'read:repositories',
  WRITE_REPOSITORIES: 'write:repositories',
  READ_ARTIFACTS: 'read:artifacts',
  WRITE_ARTIFACTS: 'write:artifacts',
  READ_MAVEN: 'read:maven',
  WRITE_MAVEN: 'write:maven',
  READ_NPM: 'read:npm',
  WRITE_NPM: 'write:npm',
  ADMIN: 'admin',
} as const;

// ===== CONFIGURACIÓN DE MOCK =====
export interface MockConfig {
  delay?: number;
  shouldFail?: boolean;
  failureRate?: number;
  customResponses?: Record<string, any>;
}

export function configureMockClient(config: MockConfig): void {
  // Esta función permite configurar el comportamiento del mock
  // Por ejemplo, añadir delays, simular fallos, etc.
  console.log('Mock client configured with:', config);
}

// ===== GENERADOR DE DATOS MOCK REALISTAS =====
export class MockDataGenerator {
  static generateRepository(name?: string): Repository {
    return {
      id: this.generateId(),
      name: name || `repo-${Math.random().toString(36).substring(2, 8)}`,
      description: `Repository for ${name || 'testing'}`,
      createdAt: new Date().toISOString(),
    };
  }

  static generatePackageResult(
    name?: string,
    type?: PackageType
  ): PackageResult {
    const packageName =
      name || `package-${Math.random().toString(36).substring(2, 8)}`;
    const packageType = type || 'npm';

    return {
      type: packageType,
      name: packageName,
      latestVersion: `${Math.floor(Math.random() * 10)}.${Math.floor(Math.random() * 10)}.${Math.floor(Math.random() * 10)}`,
      description: `Mock ${packageType} package ${packageName}`,
      downloadUrl: `https://api.repo-manager.com/v2/${packageType}/${packageName}/download`,
      lastModified: new Date().toISOString(),
      downloads: Math.floor(Math.random() * 1000000),
      maintainers: ['mock-user', 'test-user'],
      keywords: ['mock', 'test', packageType],
      license: 'MIT',
      score: Math.random(),
    };
  }

  static generateId(): string {
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(
      /[xy]/g,
      function (c) {
        const r = (Math.random() * 16) | 0;
        const v = c === 'x' ? r : (r & 0x3) | 0x8;
        return v.toString(16);
      }
    );
  }
}

// Exportar para uso en tests y desarrollo
export { MockDataGenerator as MockGenerator };
