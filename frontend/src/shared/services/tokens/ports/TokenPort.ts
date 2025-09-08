/**
 * Puerto (interfaz) para el adaptador de tokens
 * Define el contrato que deben implementar los adaptadores de datos
 * Siguiendo el principio de Inversión de Dependencias (DIP) de SOLID
 */

import type { 
  TokenRequest,
  TokenResponse,
  TokenInfo,
  ListTokensParams,
  TokenParams
} from '@/shared/types/openapi-generated.types';

/**
 * Puerto que define las operaciones de acceso a datos para tokens
 * Los adaptadores (HTTP, Mock, etc.) deben implementar esta interfaz
 */
export interface TokenPort {
  /**
   * Lista todos los tokens del usuario
   */
  listarTokens(params: ListTokensParams): Promise<TokenResponse[]>;

  /**
   * Crea un nuevo token de acceso
   */
  crearToken(body: TokenRequest): Promise<TokenResponse>;

  /**
   * Obtiene información detallada de un token
   */
  obtenerToken(params: TokenParams): Promise<TokenInfo>;

  /**
   * Elimina un token
   */
  eliminarToken(params: TokenParams): Promise<void>;
}