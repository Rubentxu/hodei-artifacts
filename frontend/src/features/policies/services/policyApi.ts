import type { Policy, NewPolicy, UpdatePolicy } from '../types/policy.types';

class PolicyService {
  private readonly basePath = '/policies';

  async getPolicies(): Promise<Policy[]> {
    // const response = await apiClient.get<Policy[]>(this.basePath);
    // return response.data;
    console.log('[Mock] Fetching policies');
    return Promise.resolve([
      {
        id: 'policy-1',
        name: 'Admin Full Access',
        description: 'Allows full access to all resources',
        effect: 'Permit',
        body: 'permit(principal, action, resource);',
      },
      {
        id: 'policy-2',
        name: 'Repository Read Access',
        description: 'Read access to specific repositories',
        effect: 'Permit',
        body: 'permit(principal, action == "repo:read", resource in Group::"public-repos");',
      },
      {
        id: 'policy-3',
        name: 'Block Malicious IPs',
        description: 'Forbids access from known malicious IPs',
        effect: 'Forbid',
        body: 'forbid(principal, action, resource) when { context.ip in Group::"malicious-ips" };',
      },
    ]);
  }

  async createPolicy(data: NewPolicy): Promise<Policy> {
    // const response = await apiClient.post<Policy>(this.basePath, data);
    // return response.data;
    console.log('[Mock] Creating policy with:', data);
    const newPolicy: Policy = {
      id: `policy-${Date.now()}`,
      ...data,
    };
    return Promise.resolve(newPolicy);
  }

  async updatePolicy(id: string, data: UpdatePolicy): Promise<Policy> {
    // const response = await apiClient.put<Policy>(`${this.basePath}/${id}`, data);
    // return response.data;
    console.log(`[Mock] Updating policy ${id} with:`, data);
    const updatedPolicy: Policy = {
      id,
      name: data.name || '',
      description: data.description || '',
      effect: data.effect || 'Permit',
      body: data.body || '',
    };
    return Promise.resolve(updatedPolicy);
  }

  async deletePolicy(id: string): Promise<void> {
    // await apiClient.delete(`${this.basePath}/${id}`);
    console.log(`[Mock] Deleting policy ${id}`);
    return Promise.resolve();
  }
}

export const policyService = new PolicyService();
