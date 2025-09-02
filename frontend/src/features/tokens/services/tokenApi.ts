import type {
  ApiToken,
  NewApiToken,
  CreatedApiToken,
} from '../types/token.types';

class TokenService {
  private readonly basePath = '/tokens';

  async getTokens(): Promise<ApiToken[]> {
    // const response = await apiClient.get<ApiToken[]>(this.basePath);
    // return response.data;
    console.log('[Mock] Fetching API tokens');
    return Promise.resolve([
      {
        id: 'token-1',
        name: 'CI/CD Pipeline',
        lastUsed: '2 hours ago',
        created: '2024-08-20',
        scopes: ['repo:read', 'repo:write'],
      },
      {
        id: 'token-2',
        name: 'Developer Access',
        lastUsed: '5 hours ago',
        created: '2024-08-15',
        scopes: ['repo:read'],
      },
    ]);
  }

  async createToken(data: NewApiToken): Promise<CreatedApiToken> {
    // const response = await apiClient.post<CreatedApiToken>(this.basePath, data);
    // return response.data;
    console.log('[Mock] Creating API token with:', data);
    const newId = `token-${Date.now()}`;
    return Promise.resolve({
      id: newId,
      ...data,
      lastUsed: 'Never',
      created: new Date().toISOString().split('T')[0],
      token: `hodei_pat_${newId}_${Math.random().toString(36).substring(2)}`,
    });
  }

  async revokeToken(id: string): Promise<void> {
    // await apiClient.delete(`${this.basePath}/${id}`);
    console.log(`[Mock] Revoking token ${id}`);
    return Promise.resolve();
  }
}

export const tokenService = new TokenService();
