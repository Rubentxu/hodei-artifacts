/**
 * Puerto (interfaz) para el adaptador de políticas
 * Define el contrato que deben implementar los adaptadores de datos
 * Siguiendo el principio de Inversión de Dependencias (DIP) de SOLID
 */

import type {
  CreatePolicyCommand,
  CreatePolicyResponse,
  ListPoliciesParams,
} from '@/shared/types/openapi-generated.types';

/**
 * Puerto que define las operaciones de acceso a datos para políticas
 * Los adaptadores (HTTP, Mock, etc.) deben implementar esta interfaz
 */
export interface PolicyPort {
  /**
   * Lista todas las políticas
   */
  listarPoliticas(params: ListPoliciesParams): Promise<CreatePolicyResponse[]>;

  /**
   * Crea una nueva política
   */
  crearPolitica(body: CreatePolicyCommand): Promise<CreatePolicyResponse>;

  /**
   * Actualiza una política existente
   */
  actualizarPolitica(
    id: string,
    body: Partial<CreatePolicyCommand>
  ): Promise<CreatePolicyResponse>;

  /**
   * Elimina una política
   */
  eliminarPolitica(id: string): Promise<void>;
}
