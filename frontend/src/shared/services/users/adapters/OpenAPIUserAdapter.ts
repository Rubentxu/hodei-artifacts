/**
 * Adaptador de usuarios que implementa el puerto UserPort
 * Utiliza el cliente OpenAPI para comunicación con el backend
 * Sigue el patrón Adapter (Puerto y Adaptador)
 */

import type { UserPort } from '../ports/UserPort';
import type {
  CreateUserCommand,
  CreateUserResponse,
  UpdateUserAttributesCommand,
  UpdateUserAttributesResponse,
  ListUsersParams,
  UserAttributesParams,
  UpdateUserAttributesBody,
} from '@/shared/types/openapi-generated.types';
import { openAPIClient } from '@/shared/api/openapi-client';

/**
 * Adaptador que implementa UserPort usando el cliente OpenAPI
 */
export class OpenAPIUserAdapter implements UserPort {
  /**
   * List all users
   */
  async listUsers(params: ListUsersParams): Promise<CreateUserResponse[]> {
    try {
      // Note: Current OpenAPI doesn't have a GET endpoint for listing users
      // This is a placeholder implementation
      // In a real implementation, this would call GET /v1/users

      console.log('Simulating user listing');
      return [];
    } catch (error) {
      console.error('Error listing users:', error);
      throw new Error('Error listing users');
    }
  }

  /**
   * Create a new user
   */
  async createUser(body: CreateUserCommand): Promise<CreateUserResponse> {
    try {
      // Note: Current OpenAPI doesn't have a POST endpoint for creating users
      // This is a placeholder implementation that simulates creation
      // In a real implementation, this would call POST /v1/users

      const simulatedUser: CreateUserResponse = {
        id: `user_${Date.now()}`,
        username: body.username || 'user',
        email: body.email || 'user@example.com',
        createdAt: new Date().toISOString(),
      };

      return simulatedUser;
    } catch (error) {
      console.error('Error creating user:', error);
      throw new Error('Error creating user');
    }
  }

  /**
   * Get user attributes
   */
  async getUserAttributes(
    params: UserAttributesParams
  ): Promise<UpdateUserAttributesResponse> {
    try {
      // Note: Current OpenAPI doesn't have a GET endpoint for user attributes
      // This is a placeholder implementation
      // In a real implementation, this would call GET /v1/users/{id}/attributes

      const simulatedAttributes: UpdateUserAttributesResponse = {
        id: params.id,
        username: 'user',
        email: 'user@example.com',
        attributes: {
          role: 'user',
          preferences: {
            theme: 'light',
            language: 'en',
          },
        },
        updatedAt: new Date().toISOString(),
      };

      return simulatedAttributes;
    } catch (error) {
      console.error(`Error getting user attributes ${params.id}:`, error);
      throw new Error('Error getting user attributes');
    }
  }

  /**
   * Update user attributes
   */
  async updateUserAttributes(
    params: UserAttributesParams,
    body: UpdateUserAttributesBody
  ): Promise<UpdateUserAttributesResponse> {
    try {
      // Note: Current OpenAPI doesn't have a PUT endpoint for updating user attributes
      // This is a placeholder implementation
      // In a real implementation, this would call PUT /v1/users/{id}/attributes

      const updatedAttributes: UpdateUserAttributesResponse = {
        id: params.id,
        username: 'user',
        email: 'user@example.com',
        attributes: body.attributes || {},
        updatedAt: new Date().toISOString(),
      };

      return updatedAttributes;
    } catch (error) {
      console.error(`Error updating user attributes ${params.id}:`, error);
      throw new Error('Error updating user attributes');
    }
  }
}
