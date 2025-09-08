import { useState } from 'react';
import { useNotifications } from '@/shared/stores/ui.store';
import { Card } from '@/components/ui/Card';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';
import { useRepositories, useRepositoryFilters } from '@/shared/hooks';
import { useDebounce } from '@/shared/hooks';
import type { RepositoryType, RepositoryFilters } from '@/shared/types';

const Repositories = () => {
  const { showInfo } = useNotifications();
  const [searchTerm, setSearchTerm] = useState('');
  const [showCreateModal, setShowCreateModal] = useState(false);
  const debouncedSearchTerm = useDebounce(searchTerm, 300);

  const { filters, updateFilter, clearFilters } = useRepositoryFilters({
    search: debouncedSearchTerm,
    limit: 10,
    sortBy: 'name',
    sortOrder: 'asc',
  });

  const { data, isLoading, error } = useRepositories(filters);

  const repositoryTypes: { value: RepositoryType | ''; label: string }[] = [
    { value: '', label: 'All Types' },
    { value: 'maven', label: 'Maven' },
    { value: 'npm', label: 'npm' },
    { value: 'pypi', label: 'PyPI' },
    { value: 'docker', label: 'Docker' },
  ];

  const handleTypeChange = (event: React.ChangeEvent<HTMLSelectElement>) => {
    const value = event.target.value as RepositoryType | '';
    updateFilter('type', value ? [value] : undefined);
  };

  if (error) {
    return (
      <div className="p-6">
        <div className="bg-red-50 border border-red-200 rounded-lg p-4">
          <h3 className="text-lg font-semibold text-red-800 mb-2">
            Error loading repositories
          </h3>
          <p className="text-red-600">
            Please try refreshing the page or contact support.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="p-6">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-3xl font-bold text-gray-900">Repositories</h1>
          <p className="text-gray-600 mt-1">
            Manage your artifact repositories and packages
          </p>
        </div>
        <Button onClick={() => setShowCreateModal(true)}>
          + New Repository
        </Button>
      </div>

      {/* Search and Filters */}
      <Card className="p-4 mb-6">
        <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
          <div className="md:col-span-2">
            <Input
              placeholder="🔍 Search repositories..."
              value={searchTerm}
              onChange={e => setSearchTerm(e.target.value)}
              className="w-full"
            />
          </div>
          <div>
            <select
              value={filters.type?.[0] || ''}
              onChange={handleTypeChange}
              className="w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
            >
              {repositoryTypes.map(type => (
                <option key={type.value} value={type.value}>
                  {type.label}
                </option>
              ))}
            </select>
          </div>
          <div>
            <Button onClick={clearFilters} variant="outline" className="w-full">
              Clear Filters
            </Button>
          </div>
        </div>

        {(filters.search || filters.type?.length) && (
          <div className="mt-4 flex items-center gap-2">
            <span className="text-sm text-gray-600">Active filters:</span>
            {filters.search && (
              <span className="inline-flex items-center px-2 py-1 rounded-full text-xs font-medium bg-blue-100 text-blue-800">
                Search: "{filters.search}"
              </span>
            )}
            {filters.type?.map(type => (
              <span
                key={type}
                className="inline-flex items-center px-2 py-1 rounded-full text-xs font-medium bg-gray-100 text-gray-800"
              >
                Type: {type}
              </span>
            ))}
            <button
              onClick={clearFilters}
              className="text-sm text-blue-600 hover:text-blue-800 ml-2"
            >
              Clear all
            </button>
          </div>
        )}
      </Card>

      {/* Loading State */}
      {isLoading && (
        <div className="space-y-4">
          <Card className="p-4">
            <div className="animate-pulse">
              <div className="h-4 bg-gray-200 rounded w-3/4 mb-2"></div>
              <div className="h-4 bg-gray-200 rounded w-1/2"></div>
            </div>
          </Card>
          <Card className="p-4">
            <div className="animate-pulse">
              <div className="h-4 bg-gray-200 rounded w-3/4 mb-2"></div>
              <div className="h-4 bg-gray-200 rounded w-1/2"></div>
            </div>
          </Card>
        </div>
      )}

      {/* Repositories List */}
      {!isLoading && data && (
        <>
          <div className="mb-4 flex items-center justify-between">
            <p className="text-sm text-gray-600">
              Showing {data.data.length} of {data.total} repositories
            </p>
          </div>

          <div className="space-y-4">
            {data.data.map(repository => (
              <Card key={repository.id} className="p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <h3 className="text-lg font-semibold text-gray-900">
                      {repository.name}
                    </h3>
                    <p className="text-gray-600 text-sm mt-1">
                      {repository.description}
                    </p>
                    <div className="flex items-center gap-4 mt-2 text-sm text-gray-500">
                      <span>Type: {repository.type}</span>
                      <span>Visibility: {repository.visibility}</span>
                      <span>Packages: {repository.packageCount}</span>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() =>
                        showInfo('Info', 'View functionality coming soon!')
                      }
                    >
                      View
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() =>
                        showInfo('Info', 'Edit functionality coming soon!')
                      }
                    >
                      Edit
                    </Button>
                  </div>
                </div>
              </Card>
            ))}
          </div>
        </>
      )}

      {/* Empty State */}
      {!isLoading && data && data.data.length === 0 && (
        <Card className="p-12 text-center">
          <div className="text-6xl mb-4">📦</div>
          <h3 className="text-xl font-semibold text-gray-900 mb-2">
            No repositories found
          </h3>
          <p className="text-gray-600 mb-6">
            {filters.search || filters.type?.length
              ? 'Try adjusting your search criteria or filters.'
              : 'Get started by creating your first repository.'}
          </p>
          <Button onClick={() => setShowCreateModal(true)}>
            + Create Repository
          </Button>
        </Card>
      )}

      {/* Create Repository Modal Placeholder */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <Card className="p-6 max-w-md w-full">
            <h3 className="text-lg font-semibold mb-4">Create Repository</h3>
            <p className="text-gray-600 mb-4">
              Repository creation coming soon...
            </p>
            <div className="flex justify-end gap-2">
              <Button
                variant="outline"
                onClick={() => setShowCreateModal(false)}
              >
                Cancel
              </Button>
              <Button
                onClick={() => {
                  setShowCreateModal(false);
                  showInfo('Info', 'Repository creation coming soon!');
                }}
              >
                Create
              </Button>
            </div>
          </Card>
        </div>
      )}
    </div>
  );
};

export default Repositories;
