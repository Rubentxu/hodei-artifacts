/**
 * Hook to get the user service
 * Implements dependency injection to facilitate testing
 */

import { useMemo } from 'react';
import { UserService } from '@/shared/services/users/UserService';
import { OpenAPIUserAdapter } from '@/shared/services/users/adapters/OpenAPIUserAdapter';

/**
 * Hook that provides the user service
 * Uses dependency injection to facilitate testing
 */
export function useUserService(): UserService {
  return useMemo(() => {
    // Inject OpenAPI adapter as dependency
    const userAdapter = new OpenAPIUserAdapter();
    return new UserService(userAdapter);
  }, []);
}
