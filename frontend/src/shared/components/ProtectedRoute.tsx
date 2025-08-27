import type { ReactNode } from 'react';
import { Navigate, useLocation } from 'react-router-dom';
import { useAuth } from '@/shared/stores/auth.store';
import { Spinner } from '@/components/ui/Spinner';

interface ProtectedRouteProps {
  children: ReactNode;
  requireAuth?: boolean;
  requiredRole?: string | string[];
  fallback?: ReactNode;
}

export const ProtectedRoute = ({
  children,
  requireAuth = true,
  requiredRole,
  fallback,
}: ProtectedRouteProps) => {
  const { isAuthenticated, isLoading, user } = useAuth();
  const location = useLocation();

  // Show loading spinner while checking authentication
  if (isLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <Spinner size="lg" />
      </div>
    );
  }

  // Redirect to login if authentication is required but user is not authenticated
  if (requireAuth && !isAuthenticated) {
    return <Navigate to="/login" state={{ from: location }} replace />;
  }

  // Redirect away from login if user is already authenticated
  if (!requireAuth && isAuthenticated) {
    return <Navigate to="/" replace />;
  }

  // Check role-based access
  if (requiredRole && isAuthenticated) {
    const hasRequiredRole = Array.isArray(requiredRole)
      ? requiredRole.includes(user?.role || '')
      : user?.role === requiredRole;

    if (!hasRequiredRole) {
      // Show fallback or redirect to unauthorized page
      if (fallback) {
        return <>{fallback}</>;
      }

      return <Navigate to="/unauthorized" state={{ from: location }} replace />;
    }
  }

  return <>{children}</>;
};

// Convenience components for common protection patterns
export const AuthRequired = ({ children }: { children: ReactNode }) => (
  <ProtectedRoute requireAuth={true}>{children}</ProtectedRoute>
);

export const AdminRequired = ({ children }: { children: ReactNode }) => (
  <ProtectedRoute requireAuth={true} requiredRole="admin">
    {children}
  </ProtectedRoute>
);

export const PublicOnly = ({ children }: { children: ReactNode }) => (
  <ProtectedRoute requireAuth={false}>{children}</ProtectedRoute>
);

// Hook version for programmatic access control
export const useRouteGuard = () => {
  const { isAuthenticated, isLoading, user } = useAuth();

  const hasRole = (role: string | string[]): boolean => {
    if (isLoading || !isAuthenticated) return false;

    return Array.isArray(role)
      ? role.includes(user?.role || '')
      : user?.role === role;
  };

  const hasAnyRole = (roles: string[]): boolean => {
    return roles.some(role => hasRole(role));
  };

  return {
    isAuthenticated,
    isLoading,
    user,
    hasRole,
    hasAnyRole,
    isAdmin: hasRole('admin'),
  };
};
