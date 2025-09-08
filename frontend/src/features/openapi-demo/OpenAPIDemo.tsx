/**
 * Componente de demostración que muestra el uso del API OpenAPI Contract First
 * Este componente demuestra cómo usar los hooks personalizados con el contrato API
 */

import React, { useState } from 'react';
import {
  useRepositories,
  useSearch,
  usePopularPackages,
  useRecentPackages,
  useAuth,
  useTokens,
  useCreateToken,
  useUsers,
  usePolicies,
} from '@/shared/hooks/use-openapi';
import { Card } from '@/components/ui/Card';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { toast } from '@/shared/utils/notifications-simple';
import type { PackageType } from '@/shared/types/openapi-generated.types';
import type { User, Policy } from '@/shared/api/openapi-service';

/**
 * Componente de demostración del API OpenAPI Contract First
 * Muestra cómo usar los servicios mock con el patrón Contract First
 */
export function OpenAPIDemo(): React.ReactElement {
  // Usar el sistema de notificaciones simple
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedType, setSelectedType] = useState<PackageType | 'all'>('all');
  const [newTokenName, setNewTokenName] = useState('');

  // ===== HOOKS DE REPOSITORIOS =====
  const {
    data: repositoriesData,
    isLoading: reposLoading,
    isError: reposError,
    error: reposErrorData,
  } = useRepositories({ limit: 10 });

  // ===== HOOKS DE BÚSQUEDA =====
  const {
    data: searchResults,
    isLoading: searchLoading,
    isError: searchError,
    error: searchErrorData,
  } = useSearch(searchQuery, {
    limit: 10,
    type: selectedType === 'all' ? undefined : (selectedType as PackageType),
  });

  const { data: popularPackages, isLoading: popularLoading } =
    usePopularPackages(5);

  const { data: recentPackages, isLoading: recentLoading } =
    useRecentPackages(5);

  // ===== HOOKS DE AUTENTICACIÓN =====
  const {
    user,
    isAuthenticated,
    isLoading: authLoading,
    login,
    logout,
  } = useAuth();

  // ===== HOOKS DE TOKENS =====
  const { data: tokens, isLoading: tokensLoading } = useTokens();

  const { mutate: createToken, isPending: creatingToken } = useCreateToken();

  // ===== HOOKS DE USUARIOS Y POLÍTICAS =====
  const { data: users, isLoading: usersLoading } = useUsers();

  const { data: policies, isLoading: policiesLoading } = usePolicies();

  // ===== HANDLERS =====

  const handleLogin = async () => {
    const result = await login({
      username: 'admin',
      password: 'admin123',
    });

    if (result.success) {
      toast.success('Login successful!');
    } else {
      toast.error(result.error || 'Login failed');
    }
  };

  const handleLogout = async () => {
    await logout();
    toast.success('Logged out successfully');
  };

  const handleCreateToken = async () => {
    if (!newTokenName.trim()) {
      toast.error('Please enter a token name');
      return;
    }

    createToken(
      {
        name: newTokenName,
        scopes: ['read:repositories', 'write:artifacts'],
        expiresAt: new Date(
          Date.now() + 30 * 24 * 60 * 60 * 1000
        ).toISOString(), // 30 días
      },
      {
        onSuccess: () => {
          toast.success('Token created successfully!');
          setNewTokenName('');
        },
        onError: error => {
          toast.error(error.message || 'Failed to create token');
        },
      }
    );
  };

  // ===== RENDER =====

  return (
    <div className="space-y-8 p-6">
      <div className="text-center mb-8">
        <h1 className="text-3xl font-bold text-gray-900 mb-2">
          OpenAPI Contract First Demo
        </h1>
        <p className="text-gray-600">
          Demonstrating the Contract First pattern with mock API responses
        </p>
        <Badge variant="success" className="mt-2">
          Contract First Pattern
        </Badge>
      </div>

      {/* ===== SECCIÓN DE AUTENTICACIÓN ===== */}
      <Card className="p-6">
        <h2 className="text-xl font-semibold mb-4">Authentication</h2>

        {authLoading ? (
          <Spinner size="sm" />
        ) : isAuthenticated ? (
          <div className="space-y-4">
            <div className="bg-green-50 border border-green-200 rounded-lg p-4">
              <p className="text-green-800 font-medium">
                ✅ Authenticated as {user?.username}
              </p>
              <p className="text-green-600 text-sm">Role: {user?.role}</p>
              <p className="text-green-600 text-sm">
                Permissions: {user?.permissions.join(', ')}
              </p>
            </div>
            <Button onClick={handleLogout} variant="outline">
              Logout
            </Button>
          </div>
        ) : (
          <div className="space-y-4">
            <div className="bg-yellow-50 border border-yellow-200 rounded-lg p-4">
              <p className="text-yellow-800">🔒 Not authenticated</p>
              <p className="text-yellow-600 text-sm">
                Use admin/admin123 to login
              </p>
            </div>
            <Button onClick={handleLogin} variant="primary">
              Login as Admin
            </Button>
          </div>
        )}
      </Card>

      {/* ===== SECCIÓN DE REPOSITORIOS ===== */}
      <Card className="p-6">
        <h2 className="text-xl font-semibold mb-4">Repositories</h2>

        {reposLoading ? (
          <Spinner size="sm" />
        ) : reposError ? (
          <div className="text-red-600">
            Error loading repositories: {reposErrorData?.message}
          </div>
        ) : (
          <div className="space-y-3">
            <p className="text-gray-600 mb-4">
              Total repositories: {repositoriesData?.total || 0}
            </p>
            {repositoriesData?.items?.map(repo => (
              <div
                key={repo.id}
                className="border border-gray-200 rounded-lg p-4"
              >
                <div className="flex justify-between items-start">
                  <div>
                    <h3 className="font-medium text-gray-900">{repo.name}</h3>
                    <p className="text-gray-600 text-sm">{repo.description}</p>
                    <p className="text-gray-500 text-xs mt-1">
                      Created: {new Date(repo.createdAt).toLocaleDateString()}
                    </p>
                  </div>
                  <Badge variant="secondary">
                    {repo.id.substring(0, 8)}...
                  </Badge>
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>

      {/* ===== SECCIÓN DE BÚSQUEDA ===== */}
      <Card className="p-6">
        <h2 className="text-xl font-semibold mb-4">Package Search</h2>

        <div className="space-y-4 mb-6">
          <Input
            placeholder="Search packages (e.g., react, junit, requests)..."
            value={searchQuery}
            onChange={e => setSearchQuery(e.target.value)}
            className="w-full"
          />

          <Select
            value={selectedType}
            onValueChange={value =>
              setSelectedType(value as PackageType | 'all')
            }
          >
            <option value="all">All Types</option>
            <option value="npm">npm</option>
            <option value="maven">Maven</option>
            <option value="pypi">PyPI</option>
          </Select>
        </div>

        {searchLoading ? (
          <Spinner size="sm" />
        ) : searchError ? (
          <div className="text-red-600">
            Error searching: {searchErrorData?.message}
          </div>
        ) : searchResults?.results && searchResults.results.length > 0 ? (
          <div className="space-y-3">
            <p className="text-gray-600 mb-4">
              Found {searchResults.total} packages
            </p>
            {searchResults.results.map(pkg => (
              <div
                key={`${pkg.type}-${pkg.name}`}
                className="border border-gray-200 rounded-lg p-4"
              >
                <div className="flex justify-between items-start">
                  <div className="flex-1">
                    <div className="flex items-center gap-2 mb-1">
                      <h3 className="font-medium text-gray-900">{pkg.name}</h3>
                      <Badge variant="secondary">{pkg.type}</Badge>
                      <Badge variant="secondary">v{pkg.latestVersion}</Badge>
                    </div>
                    <p className="text-gray-600 text-sm">{pkg.description}</p>
                    <div className="flex items-center gap-4 mt-2 text-xs text-gray-500">
                      <span>
                        📥 {pkg.downloads?.toLocaleString()} downloads
                      </span>
                      <span>⭐ Score: {(pkg.score || 0).toFixed(2)}</span>
                      <span>
                        📅{' '}
                        {new Date(pkg.lastModified || '').toLocaleDateString()}
                      </span>
                    </div>
                    {pkg.keywords && pkg.keywords.length > 0 && (
                      <div className="flex flex-wrap gap-1 mt-2">
                        {pkg.keywords.slice(0, 3).map(keyword => (
                          <Badge key={keyword} variant="secondary" size="sm">
                            {keyword}
                          </Badge>
                        ))}
                      </div>
                    )}
                  </div>
                  <div className="text-right">
                    <Badge variant="secondary">
                      {pkg.license || 'Unknown'}
                    </Badge>
                  </div>
                </div>
              </div>
            ))}
          </div>
        ) : searchQuery ? (
          <div className="text-gray-500 text-center py-8">
            No packages found for "{searchQuery}"
          </div>
        ) : (
          <div className="text-gray-500 text-center py-8">
            Enter a search query to find packages
          </div>
        )}
      </Card>

      {/* ===== SECCIÓN DE PAQUETES POPULARES Y RECIENTES ===== */}
      <div className="grid md:grid-cols-2 gap-6">
        <Card className="p-6">
          <h2 className="text-xl font-semibold mb-4">Popular Packages</h2>
          {popularLoading ? (
            <Spinner size="sm" />
          ) : (
            <div className="space-y-3">
              {popularPackages?.map(pkg => (
                <div
                  key={`${pkg.type}-${pkg.name}`}
                  className="flex justify-between items-center p-3 bg-gray-50 rounded-lg"
                >
                  <div>
                    <p className="font-medium text-gray-900">{pkg.name}</p>
                    <p className="text-xs text-gray-600">
                      {pkg.type} • {pkg.downloads?.toLocaleString()} downloads
                    </p>
                  </div>
                  <Badge variant="secondary">v{pkg.latestVersion}</Badge>
                </div>
              ))}
            </div>
          )}
        </Card>

        <Card className="p-6">
          <h2 className="text-xl font-semibold mb-4">Recent Packages</h2>
          {recentLoading ? (
            <Spinner size="sm" />
          ) : (
            <div className="space-y-3">
              {recentPackages?.map(pkg => (
                <div
                  key={`${pkg.type}-${pkg.name}`}
                  className="flex justify-between items-center p-3 bg-gray-50 rounded-lg"
                >
                  <div>
                    <p className="font-medium text-gray-900">{pkg.name}</p>
                    <p className="text-xs text-gray-600">
                      {pkg.type} •{' '}
                      {new Date(pkg.lastModified || '').toLocaleDateString()}
                    </p>
                  </div>
                  <Badge variant="secondary">v{pkg.latestVersion}</Badge>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>

      {/* ===== SECCIÓN DE TOKENS (solo si está autenticado) ===== */}
      {isAuthenticated && (
        <Card className="p-6">
          <h2 className="text-xl font-semibold mb-4">API Tokens</h2>

          <div className="mb-6">
            <div className="flex gap-2">
              <Input
                placeholder="Token name"
                value={newTokenName}
                onChange={e => setNewTokenName(e.target.value)}
                className="flex-1"
              />
              <Button
                onClick={handleCreateToken}
                disabled={creatingToken || !newTokenName.trim()}
                variant="primary"
              >
                {creatingToken ? <Spinner size="sm" /> : 'Create Token'}
              </Button>
            </div>
          </div>

          {tokensLoading ? (
            <Spinner size="sm" />
          ) : (
            <div className="space-y-3">
              {tokens && tokens.length > 0 ? (
                tokens.map(token => (
                  <div
                    key={token.id}
                    className="flex justify-between items-center p-3 bg-gray-50 rounded-lg"
                  >
                    <div>
                      <p className="font-medium text-gray-900">{token.name}</p>
                      <p className="text-xs text-gray-600">
                        Created:{' '}
                        {new Date(token.createdAt || '').toLocaleDateString()}
                      </p>
                      <p className="text-xs text-gray-600">
                        Expires:{' '}
                        {token.expiresAt
                          ? new Date(token.expiresAt).toLocaleDateString()
                          : 'Never'}
                      </p>
                      {token.scopes && token.scopes.length > 0 && (
                        <div className="flex flex-wrap gap-1 mt-1">
                          {token.scopes.map(scope => (
                            <Badge key={scope} variant="secondary" size="sm">
                              {scope}
                            </Badge>
                          ))}
                        </div>
                      )}
                    </div>
                    <Badge variant="secondary">
                      {token.token?.substring(0, 8)}...
                    </Badge>
                  </div>
                ))
              ) : (
                <div className="text-gray-500 text-center py-4">
                  No tokens created yet
                </div>
              )}
            </div>
          )}
        </Card>
      )}

      {/* ===== SECCIÓN DE USUARIOS Y POLÍTICAS (solo si está autenticado) ===== */}
      {isAuthenticated && user?.role === 'admin' && (
        <div className="grid md:grid-cols-2 gap-6">
          <Card className="p-6">
            <h2 className="text-xl font-semibold mb-4">Users</h2>
            {usersLoading ? (
              <Spinner size="sm" />
            ) : (
              <div className="space-y-3">
                {users && users.length > 0 ? (
                  users.map(user => (
                    <div
                      key={user.id}
                      className="flex justify-between items-center p-3 bg-gray-50 rounded-lg"
                    >
                      <div>
                        <p className="font-medium text-gray-900">
                          {user.username}
                        </p>
                        <p className="text-xs text-gray-600">{user.email}</p>
                        <p className="text-xs text-gray-600">
                          Role: {user.role}
                        </p>
                      </div>
                      <Badge
                        variant={
                          user.status === 'active' ? 'success' : 'secondary'
                        }
                      >
                        {user.status}
                      </Badge>
                    </div>
                  ))
                ) : (
                  <div className="text-gray-500 text-center py-4">
                    No users found
                  </div>
                )}
              </div>
            )}
          </Card>

          <Card className="p-6">
            <h2 className="text-xl font-semibold mb-4">Policies</h2>
            {policiesLoading ? (
              <Spinner size="sm" />
            ) : (
              <div className="space-y-3">
                {policies && policies.length > 0 ? (
                  policies.map(policy => (
                    <div key={policy.id} className="p-3 bg-gray-50 rounded-lg">
                      <div className="flex justify-between items-start mb-2">
                        <h3 className="font-medium text-gray-900">
                          {policy.name}
                        </h3>
                        <Badge
                          variant={policy.isActive ? 'success' : 'secondary'}
                        >
                          {policy.isActive ? 'Active' : 'Inactive'}
                        </Badge>
                      </div>
                      <p className="text-sm text-gray-600">
                        {policy.description}
                      </p>
                      <p className="text-xs text-gray-500 mt-1">
                        Created:{' '}
                        {new Date(policy.createdAt).toLocaleDateString()}
                      </p>
                    </div>
                  ))
                ) : (
                  <div className="text-gray-500 text-center py-4">
                    No policies found
                  </div>
                )}
              </div>
            )}
          </Card>
        </div>
      )}

      {/* ===== INFORMACIÓN DE CONTRACT FIRST ===== */}
      <Card className="p-6 bg-blue-50 border-blue-200">
        <h2 className="text-xl font-semibold mb-4 text-blue-900">
          Contract First Pattern
        </h2>
        <div className="space-y-3 text-blue-800">
          <p>✅ All API calls are based on the OpenAPI 3.0.3 contract</p>
          <p>
            ✅ Mock responses follow the exact schema defined in the contract
          </p>
          <p>✅ TypeScript types are generated from the OpenAPI schemas</p>
          <p>
            ✅ When the real backend is ready, just change the client
            implementation
          </p>
          <p>✅ Zero breaking changes - the contract is the source of truth</p>
        </div>
        <div className="mt-4 p-3 bg-blue-100 rounded-lg">
          <p className="text-sm text-blue-700">
            <strong>Next Steps:</strong> When the Rust backend is ready, simply
            replace the mock client with the real HTTP client. All components
            will continue to work without changes!
          </p>
        </div>
      </Card>
    </div>
  );
}

/**
 * Componente de demostración simplificado para mostrar solo los datos principales
 */
export function OpenAPIDemoCompact(): React.ReactElement {
  const { data: repositories } = useRepositories({ limit: 5 });
  const { data: popularPackages } = usePopularPackages(3);
  const { data: recentPackages } = useRecentPackages(3);

  return (
    <div className="space-y-4 p-4">
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Card className="p-4">
          <h3 className="font-semibold text-gray-900 mb-2">Repositories</h3>
          <p className="text-2xl font-bold text-blue-600">
            {repositories?.total || 0}
          </p>
          <p className="text-sm text-gray-600">Total repositories</p>
        </Card>

        <Card className="p-4">
          <h3 className="font-semibold text-gray-900 mb-2">Popular Packages</h3>
          <div className="space-y-1">
            {popularPackages?.slice(0, 3).map(pkg => (
              <div key={pkg.name} className="flex justify-between text-sm">
                <span>{pkg.name}</span>
                <Badge variant="secondary">{pkg.type}</Badge>
              </div>
            ))}
          </div>
        </Card>

        <Card className="p-4">
          <h3 className="font-semibold text-gray-900 mb-2">Recent Packages</h3>
          <div className="space-y-1">
            {recentPackages?.slice(0, 3).map(pkg => (
              <div key={pkg.name} className="flex justify-between text-sm">
                <span>{pkg.name}</span>
                <Badge variant="secondary">{pkg.type}</Badge>
              </div>
            ))}
          </div>
        </Card>
      </div>

      <div className="text-center text-sm text-gray-500">
        Powered by OpenAPI Contract First Pattern
      </div>
    </div>
  );
}
