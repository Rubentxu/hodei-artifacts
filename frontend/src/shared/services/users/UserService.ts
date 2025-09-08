/**
 * Servicio de dominio para gestión de usuarios
 * Sigue principios SOLID y Clean Code
 */

import type {
  CreateUserCommand,
  CreateUserResponse,
  UpdateUserAttributesCommand,
  UpdateUserAttributesResponse,
  ListUsersParams,
  UserAttributesParams,
  UpdateUserAttributesBody,
} from '@/shared/types/openapi-generated.types';
import type { UserPort } from './ports/UserPort';

/**
 * Servicio de aplicación para operaciones de usuarios
 * Implementa la lógica de negocio específica del dominio
 */
export class UserService {
  constructor(userPort: UserPort) {
    this.userPort = userPort;
  }

  private readonly userPort: UserPort;

  /**
   * List all users
   */
  async listUsers(): Promise<CreateUserResponse[]> {
    try {
      const params: ListUsersParams = {};
      return await this.userPort.listUsers(params);
    } catch (error) {
      console.error('Error listing users:', error);
      throw new Error('Could not list users');
    }
  }

  /**
   * Create a new user
   */
  async createUser(command: CreateUserCommand): Promise<CreateUserResponse> {
    // Business validations
    if (!command.username || command.username.trim().length === 0) {
      throw new Error('Username is required');
    }

    if (command.username.length < 3) {
      throw new Error('Username must be at least 3 characters long');
    }

    if (command.username.length > 50) {
      throw new Error('Username cannot exceed 50 characters');
    }

    // Validate username format
    const usernameRegex = /^[a-zA-Z0-9_-]+$/;
    if (!usernameRegex.test(command.username)) {
      throw new Error(
        'Username can only contain letters, numbers, hyphens and underscores'
      );
    }

    if (!command.email || command.email.trim().length === 0) {
      throw new Error('Email is required');
    }

    // Validate email format
    const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
    if (!emailRegex.test(command.email)) {
      throw new Error('Email format is not valid');
    }

    if (!command.password || command.password.length === 0) {
      throw new Error('Password is required');
    }

    // Validate password strength
    const passwordErrors = this.validatePasswordStrength(command.password);
    if (passwordErrors.length > 0) {
      throw new Error(
        `Password does not meet requirements: ${passwordErrors.join(', ')}`
      );
    }

    try {
      return await this.userPort.createUser(command);
    } catch (error) {
      console.error('Error creating user:', error);
      throw new Error('Could not create user');
    }
  }

  /**
   * Get user attributes
   */
  async getUserAttributes(
    userId: string
  ): Promise<UpdateUserAttributesResponse> {
    if (!userId || userId.trim().length === 0) {
      throw new Error('User ID is required');
    }

    try {
      const params: UserAttributesParams = { id: userId };
      return await this.userPort.getUserAttributes(params);
    } catch (error) {
      console.error(`Error getting user attributes ${userId}:`, error);
      throw new Error('Could not get user attributes');
    }
  }

  /**
   * Update user attributes
   */
  async updateUserAttributes(
    userId: string,
    command: UpdateUserAttributesCommand
  ): Promise<UpdateUserAttributesResponse> {
    if (!userId || userId.trim().length === 0) {
      throw new Error('User ID is required');
    }

    if (!command.attributes || Object.keys(command.attributes).length === 0) {
      throw new Error('Attributes to update are required');
    }

    try {
      const params: UserAttributesParams = { id: userId };
      const body: UpdateUserAttributesBody = command;

      return await this.userPort.updateUserAttributes(params, body);
    } catch (error) {
      console.error(`Error updating user attributes ${userId}:`, error);
      throw new Error('Could not update user attributes');
    }
  }

  /**
   * Validate password strength
   */
  private validatePasswordStrength(password: string): string[] {
    const errors: string[] = [];

    if (password.length < 8) {
      errors.push('minimum 8 characters');
    }

    if (password.length > 128) {
      errors.push('maximum 128 characters');
    }

    if (!/[a-z]/.test(password)) {
      errors.push('at least one lowercase letter');
    }

    if (!/[A-Z]/.test(password)) {
      errors.push('at least one uppercase letter');
    }

    if (!/[0-9]/.test(password)) {
      errors.push('at least one number');
    }

    if (!/[!@#$%^&*()_+\-=\[\]{};':"\\|,.<>\/?]/.test(password)) {
      errors.push('at least one special character');
    }

    return errors;
  }

  /**
   * Generate suggested username based on email
   */
  generateSuggestedUsername(email: string): string {
    const parts = email.split('@');
    const usernameBase = parts[0];

    // Clean username from disallowed characters
    const cleanUsername = usernameBase
      .toLowerCase()
      .replace(/[^a-z0-9_-]/g, '')
      .substring(0, 50);

    return cleanUsername || 'user';
  }

  /**
   * Check if email is in use (simulation)
   */
  async isEmailInUse(email: string): Promise<boolean> {
    // In a real implementation, this would query the database
    // For now, we simulate it's not in use
    return false;
  }

  /**
   * Check if username is in use (simulation)
   */
  async isUsernameInUse(username: string): Promise<boolean> {
    // In a real implementation, this would query the database
    // For now, we simulate it's not in use
    return false;
  }

  /**
   * Generate default attributes for a new user
   */
  generateDefaultAttributes(): Record<string, any> {
    return {
      role: 'user',
      createdAt: new Date().toISOString(),
      preferences: {
        theme: 'light',
        language: 'en',
        notifications: true,
      },
    };
  }

  /**
   * Analyze user and extract useful information
   */
  analyzeUser(user: CreateUserResponse): {
    id: string;
    username: string;
    email: string;
    isActive: boolean;
    creationDate: Date;
    daysSinceCreation: number;
  } {
    const creationDate = new Date(user.createdAt || new Date());
    const now = new Date();
    const daysSinceCreation = Math.floor(
      (now.getTime() - creationDate.getTime()) / (1000 * 60 * 60 * 24)
    );

    return {
      id: user.id || '',
      username: user.username || '',
      email: user.email || '',
      isActive: true, // By default, new users are active
      creationDate,
      daysSinceCreation,
    };
  }
}
