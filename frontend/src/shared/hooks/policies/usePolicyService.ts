/**
 * Hook para obtener el servicio de políticas
 * Implementa inyección de dependencias para facilitar testing
 */

import { useMemo } from 'react';
import { PolicyService } from '@/shared/services/policies/PolicyService';
import { OpenAPIPolicyAdapter } from '@/shared/services/policies/adapters/OpenAPIPolicyAdapter';

/**
 * Hook que proporciona el servicio de políticas
 * Usa inyección de dependencias para facilitar el testing
 */
export function usePolicyService(): PolicyService {
  return useMemo(() => {
    // Inyectar el adaptador OpenAPI como dependencia
    const policyAdapter = new OpenAPIPolicyAdapter();
    return new PolicyService(policyAdapter);
  }, []);
}