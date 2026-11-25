import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { STORAGE_KEYS } from '@/shared/constants';
import type { User } from '@/shared/types';

export interface AuthState {
  // Authentication state
  user: User | null;
  token: string | null;
  isAuthenticated: boolean;
  /** Monotonically increasing counter bumped on login, logout, or principal/token change. */
  sessionEpoch: number;

  // Actions
  login: (user: User, token: string) => void;
  logout: () => void;
  setUser: (user: User | null) => void;
  setToken: (token: string | null) => void;

  // Loading states
  isLoading: boolean;
  setIsLoading: (loading: boolean) => void;

  // Error state
  error: string | null;
  setError: (error: string | null) => void;
  clearError: () => void;
}

/**
 * Derives a cache-safe session identifier without exposing raw credentials.
 */
export const selectPrincipalSessionKey = (
  state: Pick<AuthState, 'user' | 'token' | 'sessionEpoch'>
): string => {
  const principal =
    state.user?.id ?? (state.token ? 'authenticated' : 'anonymous');
  return `${principal}:${state.sessionEpoch}`;
};

export const useAuthStore = create<AuthState>()(
  persist(
    set => ({
      // Initial state
      user: null,
      token: null,
      isAuthenticated: false,
      sessionEpoch: 0,
      isLoading: false,
      error: null,

      // Actions
      login: (user, token) => {
        set(state => ({
          user,
          token,
          isAuthenticated: true,
          sessionEpoch: state.sessionEpoch + 1,
          error: null,
          isLoading: false,
        }));
      },

      logout: () => {
        set(state => ({
          user: null,
          token: null,
          isAuthenticated: false,
          sessionEpoch: state.sessionEpoch + 1,
          error: null,
          isLoading: false,
        }));
      },

      setUser: user => {
        set(state => {
          const principalChanged =
            user?.id !== state.user?.id ||
            Boolean(user) !== Boolean(state.user);
          return {
            user,
            isAuthenticated: user ? true : state.isAuthenticated,
            sessionEpoch: principalChanged
              ? state.sessionEpoch + 1
              : state.sessionEpoch,
          };
        });
      },

      setToken: token => {
        set(state => ({
          token,
          isAuthenticated: token ? true : state.isAuthenticated,
          sessionEpoch:
            token !== state.token ? state.sessionEpoch + 1 : state.sessionEpoch,
        }));
      },

      setIsLoading: isLoading => set({ isLoading }),

      setError: error => set({ error }),

      clearError: () => set({ error: null }),
    }),
    {
      name: STORAGE_KEYS.AUTH_TOKEN,
      partialize: state => ({
        user: state.user,
        token: state.token,
        isAuthenticated: state.isAuthenticated,
      }),
    }
  )
);

// Convenience hooks for specific auth actions
export const useAuth = () => {
  const user = useAuthStore(state => state.user);
  const token = useAuthStore(state => state.token);
  const isAuthenticated = useAuthStore(state => state.isAuthenticated);
  const sessionEpoch = useAuthStore(state => state.sessionEpoch);
  const isLoading = useAuthStore(state => state.isLoading);
  const error = useAuthStore(state => state.error);

  const login = useAuthStore(state => state.login);
  const logout = useAuthStore(state => state.logout);
  const setUser = useAuthStore(state => state.setUser);
  const setToken = useAuthStore(state => state.setToken);
  const setIsLoading = useAuthStore(state => state.setIsLoading);
  const setError = useAuthStore(state => state.setError);
  const clearError = useAuthStore(state => state.clearError);

  return {
    user,
    token,
    isAuthenticated,
    sessionEpoch,
    isLoading,
    error,
    login,
    logout,
    setUser,
    setToken,
    setIsLoading,
    setError,
    clearError,
  };
};

// Hook to check user permissions
export const usePermissions = () => {
  const user = useAuthStore(state => state.user);

  const hasRole = (role: string): boolean => {
    return user?.role === role;
  };

  const hasAnyRole = (roles: string[]): boolean => {
    return roles.includes(user?.role ?? '');
  };

  const isAdmin = (): boolean => {
    return user?.role === 'admin';
  };

  return {
    hasRole,
    hasAnyRole,
    isAdmin,
    userRole: user?.role,
  };
};
