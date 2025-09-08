/**
 * Servicio de dominio para gestión de tokens de autenticación
 * Sigue principios SOLID y Clean Code
 */

import type {
  TokenRequest,
  TokenResponse,
  TokenInfo,
  ListTokensParams,
  TokenParams,
} from '@/shared/types/openapi-generated.types';
import type { TokenPort } from './ports/TokenPort.js';

/**
 * Servicio de aplicación para operaciones de tokens
 * Implementa la lógica de negocio específica del dominio
 */
export class TokenService {
  constructor(tokenPort: TokenPort) {
    this.tokenPort = tokenPort;
  }

  private readonly tokenPort: TokenPort;

  /**
   * Lista todos los tokens del usuario actual
   */
  async listarTokens(): Promise<TokenResponse[]> {
    try {
      const params: ListTokensParams = {};
      return await this.tokenPort.listarTokens(params);
    } catch (error) {
      console.error('Error al listar tokens:', error);
      throw new Error('No se pudieron listar los tokens');
    }
  }

  /**
   * Crea un nuevo token de acceso
   */
  async crearToken(solicitud: TokenRequest): Promise<TokenResponse> {
    // Validaciones de negocio
    if (!solicitud.name || solicitud.name.trim().length === 0) {
      throw new Error('El nombre del token es requerido');
    }

    if (solicitud.name.length < 3) {
      throw new Error('El nombre del token debe tener al menos 3 caracteres');
    }

    if (solicitud.name.length > 50) {
      throw new Error('El nombre del token no puede exceder 50 caracteres');
    }

    // Validar scopes si se proporcionan
    if (solicitud.scopes && solicitud.scopes.length > 0) {
      const scopesValidos = this.validarScopes(solicitud.scopes);
      if (scopesValidos.length !== solicitud.scopes.length) {
        throw new Error('Algunos scopes proporcionados no son válidos');
      }
    }

    // Si no se proporciona fecha de expiración, establecer una por defecto (30 días)
    if (!solicitud.expiresAt) {
      const fechaExpiracion = new Date();
      fechaExpiracion.setDate(fechaExpiracion.getDate() + 30);
      solicitud.expiresAt = fechaExpiracion.toISOString();
    }

    // Validar que la fecha de expiración sea futura
    const fechaExpiracion = new Date(solicitud.expiresAt);
    const ahora = new Date();
    if (fechaExpiracion <= ahora) {
      throw new Error('La fecha de expiración debe ser futura');
    }

    // Validar que la fecha de expiración no sea muy lejana (máximo 1 año)
    const unAnioDespues = new Date();
    unAnioDespues.setFullYear(unAnioDespues.getFullYear() + 1);
    if (fechaExpiracion > unAnioDespues) {
      throw new Error('La fecha de expiración no puede ser mayor a 1 año');
    }

    try {
      return await this.tokenPort.crearToken(solicitud);
    } catch (error) {
      console.error('Error al crear token:', error);
      throw new Error('No se pudo crear el token');
    }
  }

  /**
   * Obtiene información detallada de un token
   */
  async obtenerTokenInfo(tokenId: string): Promise<TokenInfo> {
    if (!tokenId || tokenId.trim().length === 0) {
      throw new Error('El ID del token es requerido');
    }

    try {
      const params: TokenParams = { tokenId };
      return await this.tokenPort.obtenerToken(params);
    } catch (error) {
      console.error(
        `Error al obtener información del token ${tokenId}:`,
        error
      );
      throw new Error('No se pudo obtener la información del token');
    }
  }

  /**
   * Elimina un token
   */
  async eliminarToken(tokenId: string): Promise<void> {
    if (!tokenId || tokenId.trim().length === 0) {
      throw new Error('El ID del token es requerido');
    }

    try {
      const params: TokenParams = { tokenId };
      await this.tokenPort.eliminarToken(params);
    } catch (error) {
      console.error(`Error al eliminar token ${tokenId}:`, error);
      throw new Error('No se pudo eliminar el token');
    }
  }

  /**
   * Verifica si un token está expirado
   */
  estaTokenExpirado(token: TokenResponse | TokenInfo): boolean {
    if (!token.expiresAt) {
      return false; // Sin fecha de expiración, asumimos que no expira
    }

    const fechaExpiracion = new Date(token.expiresAt);
    const ahora = new Date();

    return fechaExpiracion <= ahora;
  }

  /**
   * Calcula los días restantes hasta la expiración de un token
   */
  calcularDiasHastaExpiracion(token: TokenResponse | TokenInfo): number {
    if (!token.expiresAt) {
      return -1; // Sin fecha de expiración
    }

    const fechaExpiracion = new Date(token.expiresAt);
    const ahora = new Date();

    const diferenciaMs = fechaExpiracion.getTime() - ahora.getTime();
    const diferenciaDias = Math.ceil(diferenciaMs / (1000 * 60 * 60 * 24));

    return diferenciaDias;
  }

  /**
   * Genera un token seguro y único
   */
  generarTokenSeguro(): string {
    const caracteres =
      'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    const longitud = 32;
    let token = '';

    for (let i = 0; i < longitud; i++) {
      token += caracteres.charAt(Math.floor(Math.random() * caracteres.length));
    }

    return `hodei_${token}`;
  }

  /**
   * Valida los scopes proporcionados
   */
  private validarScopes(scopes: string[]): string[] {
    const scopesValidos = [
      'read:repositories',
      'write:repositories',
      'read:artifacts',
      'write:artifacts',
      'read:maven',
      'write:maven',
      'read:npm',
      'write:npm',
      'admin',
    ];

    return scopes.filter(scope => scopesValidos.includes(scope));
  }

  /**
   * Genera scopes recomendados basados en el uso
   */
  generarScopesRecomendados(
    tipoUso: 'lectura' | 'escritura' | 'admin' = 'lectura'
  ): string[] {
    switch (tipoUso) {
      case 'lectura':
        return ['read:repositories', 'read:artifacts'];

      case 'escritura':
        return [
          'read:repositories',
          'write:repositories',
          'read:artifacts',
          'write:artifacts',
        ];

      case 'admin':
        return ['admin'];

      default:
        return ['read:repositories'];
    }
  }

  /**
   * Analiza un token y extrae información útil
   */
  analizarToken(token: TokenResponse | TokenInfo): {
    id: string;
    nombre: string;
    estaActivo: boolean;
    estaExpirado: boolean;
    diasHastaExpiracion: number;
    scopes: string[];
    esTokenAdmin: boolean;
  } {
    const estaExpirado = this.estaTokenExpirado(token);
    const diasHastaExpiracion = this.calcularDiasHastaExpiracion(token);
    const esTokenAdmin = token.scopes?.includes('admin') ?? false;

    return {
      id: token.id || '',
      nombre: token.name || '',
      estaActivo: !estaExpirado,
      estaExpirado,
      diasHastaExpiracion,
      scopes: token.scopes || [],
      esTokenAdmin,
    };
  }

  /**
   * Genera una descripción útil para un token basado en sus scopes
   */
  generarDescripcionToken(scopes: string[]): string {
    if (scopes.includes('admin')) {
      return 'Token de administrador con acceso completo al sistema';
    }

    const descripciones: string[] = [];

    if (scopes.some(s => s.includes('repositories'))) {
      if (scopes.some(s => s.includes('write:repositories'))) {
        descripciones.push('gestión de repositorios');
      } else {
        descripciones.push('lectura de repositorios');
      }
    }

    if (scopes.some(s => s.includes('artifacts'))) {
      if (scopes.some(s => s.includes('write:artifacts'))) {
        descripciones.push('gestión de artefactos');
      } else {
        descripciones.push('lectura de artefactos');
      }
    }

    if (scopes.some(s => s.includes('maven'))) {
      descripciones.push('acceso a Maven');
    }

    if (scopes.some(s => s.includes('npm'))) {
      descripciones.push('acceso a NPM');
    }

    if (descripciones.length === 0) {
      return 'Token con acceso básico';
    }

    return `Token con acceso a ${descripciones.join(', ')}`;
  }
}
