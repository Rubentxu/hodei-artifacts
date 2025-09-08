/**
 * Adaptador de políticas que implementa el puerto PolicyPort
 * Utiliza el cliente OpenAPI para comunicación con el backend
 * Sigue el patrón Adapter (Puerto y Adaptador)
 */

import type { PolicyPort } from '../ports/PolicyPort';
import type { 
  CreatePolicyCommand,
  CreatePolicyResponse,
  ListPoliciesParams
} from '@/shared/types/openapi-generated.types';
import { openAPIClient } from '@/shared/api/openapi-client';

/**
 * Adaptador que implementa PolicyPort usando el cliente OpenAPI
 */
export class OpenAPIPolicyAdapter implements PolicyPort {
  /**
   * Lista todas las políticas
   */
  async listarPoliticas(params: ListPoliciesParams): Promise<CreatePolicyResponse[]> {
    try {
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.listPolicies(params);
      
      // El resultado ya está en el formato correcto según el contrato OpenAPI
      return resultado;
    } catch (error) {
      console.error('Error al listar políticas:', error);
      throw new Error('Error al listar políticas');
    }
  }

  /**
   * Crea una nueva política
   */
  async crearPolitica(body: CreatePolicyCommand): Promise<CreatePolicyResponse> {
    try {
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.createPolicy(body);
      
      // El resultado ya está en el formato correcto según el contrato OpenAPI
      return resultado;
    } catch (error) {
      console.error('Error al crear política:', error);
      throw new Error('Error al crear política');
    }
  }

  /**
   * Actualiza una política existente
   */
  async actualizarPolitica(id: string, body: Partial<CreatePolicyCommand>): Promise<CreatePolicyResponse> {
    try {
      // Nota: El OpenAPI actual no tiene un endpoint PUT para políticas
      // Esto es una implementación placeholder que simula la actualización
      // En una implementación real, esto llamaría a un endpoint PUT /v1/policies/{id}
      
      // Por ahora, vamos a simular la actualización creando una nueva política
      // y retornándola con el ID actualizado
      const politicaActualizada: CreatePolicyResponse = {
        id: id,
        name: body.name || 'updated-policy',
        description: body.description,
        isActive: body.isActive ?? true,
        createdAt: new Date().toISOString()
      };

      return politicaActualizada;
    } catch (error) {
      console.error(`Error al actualizar política ${id}:`, error);
      throw new Error('Error al actualizar política');
    }
  }

  /**
   * Elimina una política
   */
  async eliminarPolitica(id: string): Promise<void> {
    try {
      // Nota: El OpenAPI actual no tiene un endpoint DELETE para políticas
      // Esto es una implementación placeholder
      // En una implementación real, esto llamaría a DELETE /v1/policies/{id}
      
      console.log(`Simulando eliminación de política ${id}`);
      // No hay error, simulamos éxito
    } catch (error) {
      console.error(`Error al eliminar política ${id}:`, error);
      throw new Error('Error al eliminar política');
    }
  }
}