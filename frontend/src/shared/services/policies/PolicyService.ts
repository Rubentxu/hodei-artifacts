/**
 * Servicio de dominio para gestión de políticas de seguridad
 * Sigue principios SOLID y Clean Code
 */

import type { 
  CreatePolicyCommand,
  CreatePolicyResponse,
  ListPoliciesParams
} from '@/shared/types/openapi-generated.types';
import type { PolicyPort } from './ports/PolicyPort.js';

/**
 * Servicio de aplicación para operaciones de políticas
 * Implementa la lógica de negocio específica del dominio
 */
export class PolicyService {
  constructor(policyPort: PolicyPort) {
    this.policyPort = policyPort;
  }

  private readonly policyPort: PolicyPort;

  /**
   * Lista todas las políticas
   */
  async listarPoliticas(): Promise<CreatePolicyResponse[]> {
    try {
      const params: ListPoliciesParams = {};
      return await this.policyPort.listarPoliticas(params);
    } catch (error) {
      console.error('Error al listar políticas:', error);
      throw new Error('No se pudieron listar las políticas');
    }
  }

  /**
   * Crea una nueva política
   */
  async crearPolitica(comando: CreatePolicyCommand): Promise<CreatePolicyResponse> {
    // Validaciones de negocio
    if (!comando.name || comando.name.trim().length === 0) {
      throw new Error('El nombre de la política es requerido');
    }

    if (comando.name.length < 3) {
      throw new Error('El nombre de la política debe tener al menos 3 caracteres');
    }

    if (comando.name.length > 100) {
      throw new Error('El nombre de la política no puede exceder 100 caracteres');
    }

    if (!comando.policy || comando.policy.trim().length === 0) {
      throw new Error('El contenido de la política es requerido');
    }

    // Validar sintaxis básica de Cedar (ejemplo simple)
    const erroresSintaxis = this.validarSintaxisCedar(comando.policy);
    if (erroresSintaxis.length > 0) {
      throw new Error(`Errores de sintaxis en la política: ${erroresSintaxis.join(', ')}`);
    }

    try {
      return await this.policyPort.crearPolitica(comando);
    } catch (error) {
      console.error('Error al crear política:', error);
      throw new Error('No se pudo crear la política');
    }
  }

  /**
   * Activa una política
   */
  async activarPolitica(id: string): Promise<CreatePolicyResponse> {
    if (!id || id.trim().length === 0) {
      throw new Error('El ID de la política es requerido');
    }

    try {
      // Obtener la política actual (esto requeriría un método getPolicy en el puerto)
      // Por ahora, asumimos que la política existe y la actualizamos
      const politicaActualizada = await this.policyPort.actualizarPolitica(id, {
        isActive: true
      });

      return politicaActualizada;
    } catch (error) {
      console.error(`Error al activar política ${id}:`, error);
      throw new Error('No se pudo activar la política');
    }
  }

  /**
   * Desactiva una política
   */
  async desactivarPolitica(id: string): Promise<CreatePolicyResponse> {
    if (!id || id.trim().length === 0) {
      throw new Error('El ID de la política es requerido');
    }

    try {
      const politicaActualizada = await this.policyPort.actualizarPolitica(id, {
        isActive: false
      });

      return politicaActualizada;
    } catch (error) {
      console.error(`Error al desactivar política ${id}:`, error);
      throw new Error('No se pudo desactivar la política');
    }
  }

  /**
   * Elimina una política
   */
  async eliminarPolitica(id: string): Promise<void> {
    if (!id || id.trim().length === 0) {
      throw new Error('El ID de la política es requerido');
    }

    try {
      await this.policyPort.eliminarPolitica(id);
    } catch (error) {
      console.error(`Error al eliminar política ${id}:`, error);
      throw new Error('No se pudo eliminar la política');
    }
  }

  /**
   * Valida la sintaxis básica de una política Cedar
   */
  private validarSintaxisCedar(policy: string): string[] {
    const errores: string[] = [];
    
    // Validaciones básicas de sintaxis Cedar
    const lineas = policy.split('\n').map(line => line.trim()).filter(line => line.length > 0);
    
    for (let i = 0; i < lineas.length; i++) {
      const linea = lineas[i];
      
      // Verificar que las líneas terminen con punto y coma
      if (!linea.endsWith(';') && !linea.startsWith('//') && !linea.startsWith('/*')) {
        errores.push(`Línea ${i + 1}: Falta punto y coma`);
      }
      
      // Verificar estructura básica de permitir/denegar
      if (linea.includes('permit') || linea.includes('forbid')) {
        if (!linea.includes('when')) {
          errores.push(`Línea ${i + 1}: Falta condición 'when'`);
        }
      }
    }
    
    // Validar que haya al menos una regla permitir o forbid
    const tienePermitir = lineas.some(linea => linea.includes('permit'));
    const tieneForbid = lineas.some(linea => linea.includes('forbid'));
    
    if (!tienePermitir && !tieneForbid) {
      errores.push('La política debe contener al menos una regla permitir o forbid');
    }
    
    return errores;
  }

  /**
   * Genera una plantilla de política Cedar básica
   */
  generarPlantillaPolitica(tipo: 'usuario' | 'repositorio' | 'admin' = 'usuario'): string {
    switch (tipo) {
      case 'usuario':
        return `// Política de acceso para usuarios estándar
permit(
  principal == User::"user-id",
  action in [Action::"read:repositories", Action::"read:artifacts"],
  resource
)
when {
  resource.type == "repository" && 
  resource.visibility == "public"
};`;
      
      case 'repositorio':
        return `// Política de acceso para gestión de repositorios
permit(
  principal,
  action in [Action::"write:repositories", Action::"read:repositories"],
  resource
)
when {
  resource.type == "repository" && 
  principal.role == "developer"
};`;
      
      case 'admin':
        return `// Política de acceso completo para administradores
permit(
  principal,
  action,
  resource
)
when {
  principal.role == "admin"
};`;
      
      default:
        return `// Plantilla básica de política
permit(
  principal,
  action,
  resource
)
when {
  // Agregar condiciones aquí
};`;
    }
  }

  /**
   * Analiza una política y extrae información útil
   */
  analizarPolitica(policy: string): {
    acciones: string[];
    recursos: string[];
    condiciones: string[];
    tipo: 'permit' | 'forbid';
  } {
    const acciones: string[] = [];
    const recursos: string[] = [];
    const condiciones: string[] = [];
    let tipo: 'permit' | 'forbid' = 'permit';
    
    const lineas = policy.split('\n').map(line => line.trim());
    
    for (const linea of lineas) {
      if (linea.includes('permit')) tipo = 'permit';
      if (linea.includes('forbid')) tipo = 'forbid';
      
      // Extraer acciones
      if (linea.includes('action')) {
        const match = linea.match(/Action::"([^"]+)"/g);
        if (match) {
          acciones.push(...match.map(m => m.replace(/Action::"/g, '').replace(/"/g, '')));
        }
      }
      
      // Extraer condiciones básicas
      if (linea.includes('when')) {
        const condicionStart = lineas.indexOf(linea);
        const condicionEnd = lineas.findIndex((l, i) => i > condicionStart && l.includes('};'));
        if (condicionEnd > -1) {
          const condicionLines = lineas.slice(condicionStart + 1, condicionEnd);
          condiciones.push(...condicionLines.map(l => l.trim()).filter(l => l.length > 0));
        }
      }
    }
    
    return {
      acciones: [...new Set(acciones)], // Eliminar duplicados
      recursos: [...new Set(recursos)],
      condiciones,
      tipo
    };
  }
}