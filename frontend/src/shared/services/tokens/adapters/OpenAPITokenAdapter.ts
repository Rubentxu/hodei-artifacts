/**
 * Adaptador de tokens que implementa el puerto TokenPort
 * Utiliza el cliente OpenAPI para comunicación con el backend
 * Sigue el patrón Adapter (Puerto y Adaptador)
 */

import type { TokenPort } from '../ports/TokenPort';
import type { 
  TokenRequest,
  TokenResponse,
  TokenInfo,
  ListTokensParams,
  TokenParams
} from '@/shared/types/openapi-generated.types';
import { openAPIClient } from '@/shared/api/openapi-client';

/**
 * Adaptador que implementa TokenPort usando el cliente OpenAPI
 */
export class OpenAPITokenAdapter implements TokenPort {
  /**
   * Lista todos los tokens del usuario
   */
  async listarTokens(params: ListTokensParams): Promise<TokenResponse[]> {
    try {
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.listTokens(params);
      
      // El resultado ya está en el formato correcto según el contrato OpenAPI
      return resultado;
    } catch (error) {
      console.error('Error al listar tokens:', error);
      throw new Error('Error al listar tokens');
    }
  }

  /**
   * Crea un nuevo token de acceso
   */
  async crearToken(body: TokenRequest): Promise<TokenResponse> {
    try {
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.createToken(body);
      
      // El resultado ya está en el formato correcto según el contrato OpenAPI
      return resultado;
    } catch (error) {
      console.error('Error al crear token:', error);
      throw new Error('Error al crear token');
    }
  }

  /**
   * Obtiene información detallada de un token
   */
  async obtenerToken(params: TokenParams): Promise<TokenInfo> {
    try {
      // Llamar al cliente OpenAPI
      const resultado = await openAPIClient.getToken(params);
      
      // El resultado ya está en el formato correcto según el contrato OpenAPI
      return resultado;
    } catch (error) {
      console.error(`Error al obtener información del token ${params.tokenId}:`, error);
      throw new Error('Error al obtener información del token');
    }
  }

  /**
   * Elimina un token
   */
  async eliminarToken(params: TokenParams): Promise<void> {
    try {
      // Llamar al cliente OpenAPI
      await openAPIClient.deleteToken(params);
      
      // No hay resultado que retornar para DELETE
    } catch (error) {
      console.error(`Error al eliminar token ${params.tokenId}:`, error);
      throw new Error('Error al eliminar token');
    }
  }
}