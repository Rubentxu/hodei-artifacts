import { useState } from 'react';
import { notificationService } from '@/shared/stores/notification.store';
import { Link } from 'react-router-dom';
import { Card } from '@/components/ui/Card';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';
import { RepositoryCard, CreateRepositoryModal } from '@/components/repository';
import {
  useRepositories,
  useRepositoryFilters,
} from '@/shared/hooks/repositories';
import { useDebounce } from '@/shared/hooks';
import type { RepositoryType } from '@/shared/types';

export const Repositories = () => {
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

  const repositoryStatuses = [
    { value: '', label: 'All Status' },
    { value: 'active', label: 'Active' },
    { value: 'inactive', label: 'Inactive' },
    { value: 'maintenance', label: 'Maintenance' },
  ];

  const handleTypeChange = (event: React.ChangeEvent<HTMLSelectElement>) => {
    const value = event.target.value as RepositoryType | '';
    updateFilter('type', value ? [value] : undefined);
  };

  const handleStatusChange = (event: React.ChangeEvent<HTMLSelectElement>) => {
    const value = event.target.value;
    updateFilter('status', value || undefined);
  };

  const formatSize = (bytes: number): string => {
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let size = bytes;
    let unitIndex = 0;

    while (size >= 1024 && unitIndex < units.length - 1) {
      size /= 1024;
      unitIndex++;
    }

    return `${Math.round(size * 100) / 100} ${units[unitIndex]}`;
  };

  const formatLastUpdated = (dateString: string): string => {
    const date = new Date(dateString);
    const now = new Date();
    const diffInHours = Math.floor(
      (now.getTime() - date.getTime()) / (1000 * 60 * 60)
    );

    if (diffInHours < 1) {
      return 'just now';
    } else if (diffInHours < 24) {
      return `${diffInHours}h ago`;
    } else if (diffInHours < 168) {
      return `${Math.floor(diffInHours / 24)}d ago`;
    } else {
      return date.toLocaleDateString();
    }
  };

  const getRepositoryIcon = (type: RepositoryType): string => {
    switch (type) {
      case 'maven':
        return '☕';
      case 'npm':
        return '📦';
      case 'pypi':
        return '🐍';
      case 'docker':
        return '🐳';
      default:
        return '📁';
    }
  };

  const getRepositoryTypeColor = (type: RepositoryType): string => {
    switch (type) {
      case 'maven':
        return 'bg-orange-100 text-orange-800';
      case 'npm':
        return 'bg-red-100 text-red-800';
      case 'pypi':
        return 'bg-blue-100 text-blue-800';
      case 'docker':
        return 'bg-cyan-100 text-cyan-800';
      default:
        return 'bg-gray-100 text-gray-800';
    }
  };

  const getVisibilityBadge = (isPublic: boolean): string => {
    return isPublic
      ? 'bg-green-100 text-green-800'
      : 'bg-yellow-100 text-yellow-800';
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
            <select
              value={filters.status || ''}
              onChange={handleStatusChange}
              className="w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
            >
              {repositoryStatuses.map(status => (
                <option key={status.value} value={status.value}>
                  {status.label}
                </option>
              ))}
            </select>
          </div>
        </div>

        {(filters.search || filters.type?.length || filters.status) && (
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
            {filters.status && (
              <span className="inline-flex items-center px-2 py-1 rounded-full text-xs font-medium bg-gray-100 text-gray-800">
                Status: {filters.status}
              </span>
            )}
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
        <div className="flex items-center justify-center py-12">
          <Spinner size="lg" />
        </div>
      )}

      {/* Repositories List */}
      {!isLoading && data && (
        <>
          <div className="mb-4 flex items-center justify-between">
            <p className="text-sm text-gray-600">
              Showing {data.data.length} of {data.pagination.total} repositories
            </p>
            <div className="flex items-center gap-2">
              <span className="text-sm text-gray-600">Sort by:</span>
              <select
                value={`${filters.sortBy}-${filters.sortOrder}`}
                onChange={e => {
                  const [sortBy, sortOrder] = e.target.value.split('-');
                  updateFilter('sortBy', sortBy);
                  updateFilter('sortOrder', sortOrder);
                }}
                className="px-2 py-1 border border-gray-300 rounded text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              >
                <option value="name-asc">Name (A-Z)</option>
                <option value="name-desc">Name (Z-A)</option>
                <option value="lastUpdated-desc">Recently Updated</option>
                <option value="lastUpdated-asc">Oldest Updated</option>
                <option value="packageCount-desc">Most Packages</option>
                <option value="size-desc">Largest Size</option>
              </select>
            </div>
          </div>

          <div className="space-y-4">
            {data.data.map(repository => (
              <RepositoryCard
                key={repository.id}
                repository={repository}
                onEdit={repo => {
                  // TODO: Implement edit functionality
                  console.log('Edit repository:', repo);
                  notificationService.info('Edit Feature', 'Edit functionality coming soon!');
                }}
                onDelete={repo => {
                  // TODO: Implement delete functionality
                  console.log('Delete repository:', repo);
                  notificationService.info('Delete Feature', 'Delete functionality coming soon!');
                }}
                onToggleVisibility={repo => {
                  // TODO: Implement toggle visibility
                  console.log('Toggle visibility:', repo);
                  notificationService.info('Visibility Feature', 'Toggle visibility coming soon!');
                }}
              />
            ))}
          </div>

          {/* Pagination */}
          {data.pagination.totalPages > 1 && (
            <div className="mt-8 flex items-center justify-center">
              <nav className="flex items-center gap-2">
                <button
                  onClick={() =>
                    updateFilter('page', Math.max(1, (filters.page || 1) - 1))
                  }
                  disabled={!data.pagination.hasPrevious}
                  className="px-3 py-2 text-sm font-medium text-gray-500 hover:text-gray-700 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  Previous
                </button>

                {Array.from(
                  { length: Math.min(5, data.pagination.totalPages) },
                  (_, i) => {
                    const currentPage = filters.page || 1;
                    const startPage = Math.max(1, currentPage - 2);
                    const pageNumber = startPage + i;

                    if (pageNumber > data.pagination.totalPages) return null;

                    return (
                      <button
                        key={pageNumber}
                        onClick={() => updateFilter('page', pageNumber)}
                        className={`px-3 py-2 text-sm font-medium rounded-md ${
                          pageNumber === currentPage
                            ? 'bg-blue-600 text-white'
                            : 'text-gray-700 hover:bg-gray-100'
                        }`}
                      >
                        {pageNumber}
                      </button>
                    );
                  }
                )}

                <button
                  onClick={() =>
                    updateFilter(
                      'page',
                      Math.min(
                        data.pagination.totalPages,
                        (filters.page || 1) + 1
                      )
                    )
                  }
                  disabled={!data.pagination.hasNext}
                  className="px-3 py-2 text-sm font-medium text-gray-500 hover:text-gray-700 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  Next
                </button>
              </nav>
            </div>
          )}
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
            {filters.search || filters.type?.length || filters.status
              ? 'Try adjusting your search criteria or filters.'
              : 'Get started by creating your first repository.'}
          </p>
          <Button onClick={() => setShowCreateModal(true)}>
            + Create Repository
          </Button>
        </Card>
      )}

      {/* Create Repository Modal */}
      <CreateRepositoryModal
        isOpen={showCreateModal}
        onClose={() => setShowCreateModal(false)}
        onSuccess={() => {
          // Refresh the repositories list
          // React Query will automatically refetch when the mutation succeeds
        }}
      />
    </div>
  );
};
