import { apiClient } from '@/shared/api/client';
import type {
  User,
  UserProfile,
  UpdateUserProfile,
  NewUser,
  UpdateUser,
} from '../types/user.types';

class UserService {
  private readonly basePath = '/users';

  async getUsers(): Promise<User[]> {
    // const response = await apiClient.get<User[]>(this.basePath);
    // return response.data;
    console.log('[Mock] Fetching all users');
    return Promise.resolve([
      {
        id: 'user-1',
        name: 'John Doe',
        email: 'john.doe@example.com',
        role: 'Admin',
        status: 'Active',
        organization: 'Hodei Inc.',
      },
      {
        id: 'user-2',
        name: 'Jane Smith',
        email: 'jane.smith@example.com',
        role: 'User',
        status: 'Active',
        organization: 'Hodei Inc.',
      },
      {
        id: 'user-3',
        name: 'Bob Johnson',
        email: 'bob.johnson@example.com',
        role: 'User',
        status: 'Inactive',
        organization: 'Hodei Inc.',
      },
    ]);
  }

  async createUser(data: NewUser): Promise<User> {
    // const response = await apiClient.post<User>(this.basePath, data);
    // return response.data;
    console.log('[Mock] Creating user with:', data);
    const newUser: User = {
      id: `user-${Date.now()}`,
      ...data,
      organization: 'Hodei Inc.',
    };
    return Promise.resolve(newUser);
  }

  async updateUser(id: string, data: UpdateUser): Promise<User> {
    // const response = await apiClient.put<User>(`${this.basePath}/${id}`, data);
    // return response.data;
    console.log(`[Mock] Updating user ${id} with:`, data);
    const updatedUser: User = {
      id,
      name: data.name || '',
      email: data.email || '',
      role: data.role || '',
      status: data.status || 'Inactive',
      organization: 'Hodei Inc.',
    };
    return Promise.resolve(updatedUser);
  }

  async getMyProfile(): Promise<UserProfile> {
    // const response = await apiClient.get<UserProfile>(`${this.basePath}/me`);
    // return response.data;
    console.log('[Mock] Fetching user profile');
    return Promise.resolve({
      id: 'user-123',
      name: 'John Doe',
      email: 'john.doe@example.com',
      organization: 'Hodei Inc.',
    });
  }

  async updateMyProfile(data: UpdateUserProfile): Promise<UserProfile> {
    // const response = await apiClient.put<UserProfile>(`${this.basePath}/me`, data);
    // return response.data;
    console.log('[Mock] Updating user profile with:', data);
    return Promise.resolve({
      id: 'user-123',
      ...data,
      organization: 'Hodei Inc.',
    });
  }
}

export const userService = new UserService();
