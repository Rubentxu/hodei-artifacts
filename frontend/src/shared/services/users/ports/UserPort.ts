/**
 * Puerto (interfaz) para el adaptador de usuarios
 * Define el contrato que deben implementar los adaptadores de datos
 * Siguiendo el principio de Inversión de Dependencias (DIP) de SOLID
 */

import type { 
  CreateUserCommand,
  CreateUserResponse,
  UpdateUserAttributesCommand,
  UpdateUserAttributesResponse,
  ListUsersParams,
  UserAttributesParams,
  UpdateUserAttributesBody
} from '@/shared/types/openapi-generated.types';

/**
 * Puerto que define las operaciones de acceso a datos para usuarios
 * Los adaptadores (HTTP, Mock, etc.) deben implementar esta interfaz
 */
export interface UserPort {
  /**
   * List all users
   */
  listUsers(params: ListUsersParams): Promise<CreateUserResponse[]>;

  /**
   * Create a new user
   */
  createUser(body: CreateUserCommand): Promise<CreateUserResponse>;

  /**
   * Get user attributes
   */
  getUserAttributes(params: UserAttributesParams): Promise<UpdateUserAttributesResponse>;

  /**
   * Update user attributes
   */
  updateUserAttributes(
    params: UserAttributesParams,
    body: UpdateUserAttributesBody
  ): Promise<UpdateUserAttributesResponse>;
}