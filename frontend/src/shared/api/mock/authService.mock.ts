import type {
  TokenRequest,
  TokenResponse,
  TokenInfo,
} from '@/shared/types/openapi.types';

// Mock data para tokens
const mockTokens: TokenInfo[] = [
  {
    id: 'token-001',
    name: 'Development Token',
    createdAt: '2024-01-15T10:30:00Z',
    expiresAt: '2024-12-31T23:59:59Z',
    lastUsedAt: '2024-01-20T14:20:00Z',
    permissions: ['read', 'write'],
  },
  {
    id: 'token-002',
    name: 'CI/CD Token',
    createdAt: '2024-01-16T09:15:00Z',
    expiresAt: '2024-06-30T23:59:59Z',
    lastUsedAt: '2024-01-18T16:45:00Z',
    permissions: ['read', 'write', 'delete'],
  },
  {
    id: 'token-003',
    name: 'Read-only Token',
    createdAt: '2024-01-17T11:30:00Z',
    permissions: ['read'],
  },
];

// Mock user data
const mockUser = {
  id: 'user-001',
  username: 'admin',
  email: 'admin@hodei-artifacts.com',
  attributes: {
    department: 'engineering',
    role: 'administrator',
    fullName: 'Administrator User',
  },
};

export const authServiceMock = {
  async login(
    username: string,
    password: string
  ): Promise<{ token: string; user: typeof mockUser }> {
    await new Promise(resolve => setTimeout(resolve, 1000)); // Simular autenticación

    if (username === 'admin' && password === 'admin123') {
      return {
        token: 'mock-jwt-token-' + Date.now(),
        user: mockUser,
      };
    }

    throw new Error('Invalid credentials');
  },

  async createToken(request: TokenRequest): Promise<TokenResponse> {
    await new Promise(resolve => setTimeout(resolve, 800));

    const newToken: TokenResponse = {
      token: 'mock-token-' + Date.now(),
      id: `token-${mockTokens.length + 1}`,
      name: request.name,
      createdAt: new Date().toISOString(),
      expiresAt: request.expiresIn
        ? new Date(Date.now() + request.expiresIn * 1000).toISOString()
        : undefined,
    };

    mockTokens.push({
      id: newToken.id,
      name: newToken.name,
      createdAt: newToken.createdAt,
      expiresAt: newToken.expiresAt,
      permissions: request.permissions || ['read'],
    });

    return newToken;
  },

  async getTokens(): Promise<TokenInfo[]> {
    await new Promise(resolve => setTimeout(resolve, 500));
    return mockTokens;
  },

  async getToken(id: string): Promise<TokenInfo | null> {
    await new Promise(resolve => setTimeout(resolve, 300));
    return mockTokens.find(token => token.id === id) || null;
  },

  async deleteToken(id: string): Promise<void> {
    await new Promise(resolve => setTimeout(resolve, 400));

    const index = mockTokens.findIndex(token => token.id === id);
    if (index === -1) throw new Error('Token not found');

    mockTokens.splice(index, 1);
  },

  async refreshToken(token: string): Promise<TokenResponse> {
    await new Promise(resolve => setTimeout(resolve, 600));

    // Simular refresh de token
    return {
      token: 'refreshed-' + token,
      id: 'refreshed-' + Date.now(),
      name: 'Refreshed Token',
      createdAt: new Date().toISOString(),
      expiresAt: new Date(Date.now() + 86400000).toISOString(), // 24 horas
    };
  },

  async logout(): Promise<void> {
    await new Promise(resolve => setTimeout(resolve, 300));
    // Limpiar datos de sesión
    console.log('User logged out');
  },

  async getCurrentUser(): Promise<typeof mockUser> {
    await new Promise(resolve => setTimeout(resolve, 200));
    return mockUser;
  },
};
