import { mockAdapter } from '@/shared/api/mockAdapter';
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
    try {
      // Usar el adaptador para obtener usuarios de servicios mock mejorados
      const legacyUsers = await mockAdapter.getUsers();

      // Convertir formato legacy al formato esperado
      return legacyUsers.map(user => ({
        id: user.id,
        name: user.name,
        email: user.email,
        role: user.role,
        status: user.status as 'Active' | 'Inactive',
        organization: user.organization,
      }));
    } catch (error) {
      console.error('Error in enhanced getUsers service:', error);
      // Retornar datos de respaldo si hay error
      return [
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
      ];
    }
  }

  async createUser(data: NewUser): Promise<User> {
    try {
      // Usar el adaptador para crear usuario
      const newUser = await mockAdapter.createUser(data);

      return {
        id: newUser.id,
        name: newUser.name,
        email: newUser.email,
        role: newUser.role,
        status: newUser.status as 'Active' | 'Inactive',
        organization: newUser.organization,
      };
    } catch (error) {
      console.error('Error in enhanced createUser service:', error);
      // Retornar usuario mock si hay error
      return {
        id: `user-${Date.now()}`,
        ...data,
        organization: 'Hodei Inc.',
      };
    }
  }

  async updateUser(id: string, data: UpdateUser): Promise<User> {
    try {
      // Por ahora, simular actualización con datos mock
      console.log(`[Enhanced Mock] Updating user ${id} with:`, data);
      return {
        id,
        name: data.name || '',
        email: data.email || '',
        role: data.role || '',
        status: data.status || 'Inactive',
        organization: 'Hodei Inc.',
      };
    } catch (error) {
      console.error('Error in enhanced updateUser service:', error);
      throw new Error('Failed to update user');
    }
  }

  async getMyProfile(): Promise<UserProfile> {
    try {
      // Usar el adaptador para obtener perfil
      const profile = await mockAdapter.getMyProfile();

      return {
        id: profile.id,
        name: profile.name,
        email: profile.email,
        organization: profile.organization,
      };
    } catch (error) {
      console.error('Error in enhanced getMyProfile service:', error);
      // Retornar perfil mock si hay error
      return {
        id: 'user-123',
        name: 'John Doe',
        email: 'john.doe@example.com',
        organization: 'Hodei Inc.',
      };
    }
  }

  async updateMyProfile(data: UpdateUserProfile): Promise<UserProfile> {
    try {
      // Por ahora, simular actualización con datos mock
      console.log('[Enhanced Mock] Updating user profile with:', data);
      return {
        id: 'user-123',
        ...data,
        organization: 'Hodei Inc.',
      };
    } catch (error) {
      console.error('Error in enhanced updateMyProfile service:', error);
      throw new Error('Failed to update profile');
    }
  }
}

export const userService = new UserService();
