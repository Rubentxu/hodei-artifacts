import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { STORAGE_KEYS } from '@/shared/constants';
import type { User } from '@/shared/types';

export interface AuthState {
  // Authentication state
  user: User | null;
  token: string | null;
  isAuthenticated: boolean;
  
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

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      // Initial state
      user: null,
      token: null,
      isAuthenticated: false,
      isLoading: false,
      error: null,

      // Actions
      login: (user, token) => {
        set({ 
          user, 
          token, 
          isAuthenticated: true, 
          error: null,
          isLoading: false 
        });
      },

      logout: () => {
        set({ 
          user: null, 
          token: null, 
          isAuthenticated: false, 
          error: null,
          isLoading: false 
        });
      },

      setUser: (user) => {
        set({ user });
        if (user) {
          set({ isAuthenticated: true });
        }
      },

      setToken: (token) => {
        set({ token });
        if (token) {
          set({ isAuthenticated: true });
        }
      },

      setIsLoading: (isLoading) => set({ isLoading }),

      setError: (error) => set({ error }),

      clearError: () => set({ error: null }),
    }),
    {
      name: STORAGE_KEYS.AUTH,
      partialize: (state) => ({
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
    return roles.includes(user?.role || '');
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