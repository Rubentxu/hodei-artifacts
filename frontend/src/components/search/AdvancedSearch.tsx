import React, { useState, useEffect } from 'react';
import { Search, Filter, X, Package, Calendar, Hash } from 'lucide-react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Card } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { searchServiceMock } from '@/shared/api/mock/searchService.mock';
import type { PackageResult, SearchQuery } from '@/shared/types/openapi.types';

interface AdvancedSearchProps {
  onSearchResults: (results: PackageResult[]) => void;
  onLoadingChange?: (loading: boolean) => void;
}

const AdvancedSearch: React.FC<AdvancedSearchProps> = ({ onSearchResults, onLoadingChange }) => {
  const [query, setQuery] = useState('');
  const [filters, setFilters] = useState({
    type: '',
    repositoryId: '',
    dateRange: '',
    sortBy: 'name' as const,
    sortOrder: 'asc' as const
  });
  const [showFilters, setShowFilters] = useState(false);
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [loading, setLoading] = useState(false);

  // Mock repositories for filter
  const repositories = [
    { id: '550e8400-e29b-41d4-a716-446655440001', name: 'maven-central' },
    { id: '550e8400-e29b-41d4-a716-446655440002', name: 'npm-public' },
    { id: '550e8400-e29b-41d4-a716-446655440003', name: 'pypi-internal' },
    { id: '550e8400-e29b-41d4-a716-446655440004', name: 'docker-registry' }
  ];

  useEffect(() => {
    if (query.length > 2) {
      loadSuggestions();
    } else {
      setSuggestions([]);
    }
  }, [query]);

  const loadSuggestions = async () => {
    try {
      const suggestions = await searchServiceMock.getSearchSuggestions(query);
      setSuggestions(suggestions);
      setShowSuggestions(true);
    } catch (error) {
      console.error('Error loading suggestions:', error);
    }
  };

  const handleSearch = async () => {
    if (!query.trim()) return;

    setLoading(true);
    onLoadingChange?.(true);

    try {
      const searchQuery: SearchQuery = {
        q: query,
        type: filters.type as 'maven' | 'npm' | 'pypi' | undefined,
        repositoryId: filters.repositoryId || undefined,
        sortBy: filters.sortBy,
        sortOrder: filters.sortOrder,
        page: 1,
        limit: 20
      };

      const results = await searchServiceMock.searchPackages(searchQuery);
      onSearchResults(results.items);
    } catch (error) {
      console.error('Search error:', error);
      onSearchResults([]);
    } finally {
      setLoading(false);
      onLoadingChange?.(false);
      setShowSuggestions(false);
    }
  };

  const handleSuggestionClick = (suggestion: string) => {
    setQuery(suggestion);
    setShowSuggestions(false);
    handleSearch();
  };

  const clearFilters = () => {
    setFilters({
      type: '',
      repositoryId: '',
      dateRange: '',
      sortBy: 'name',
      sortOrder: 'asc'
    });
  };

  const hasActiveFilters = Object.values(filters).some(value => value !== '');

  return (
    <div className="space-y-4">
      {/* Search Bar - Inspirado en JFrog Artifactory */}
      <div className="relative">
        <div className="flex gap-2">
          <div className="flex-1 relative">
            <div className="relative">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-5 h-5" />
              <Input
                type="text"
                placeholder="Search packages by name, description, or content..."
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onFocus={() => query.length > 2 && setShowSuggestions(true)}
                onBlur={() => setTimeout(() => setShowSuggestions(false), 200)}
                onKeyPress={(e) => e.key === 'Enter' && handleSearch()}
                className="pl-10 pr-4 py-3 w-full"
              />
            </div>

            {/* Suggestions Dropdown - Estilo GitHub */}
            {showSuggestions && suggestions.length > 0 && (
              <Card className="absolute top-full left-0 right-0 mt-1 z-50 max-h-60 overflow-y-auto">
                <div className="py-2">
                  {suggestions.map((suggestion, index) => (
                    <button
                      key={index}
                      className="w-full px-4 py-2 text-left hover:bg-gray-100 flex items-center space-x-2"
                      onClick={() => handleSuggestionClick(suggestion)}
                    >
                      <Package className="w-4 h-4 text-gray-400" />
                      <span>{suggestion}</span>
                    </button>
                  ))}
                </div>
              </Card>
            )}
          </div>

          <Button
            variant="outline"
            onClick={() => setShowFilters(!showFilters)}
            className="relative"
          >
            <Filter className="w-4 h-4 mr-2" />
            Filters
            {hasActiveFilters && (
              <Badge variant="secondary" className="ml-2">
                Active
              </Badge>
            )}
          </Button>

          <Button onClick={handleSearch} disabled={loading}>
            {loading ? (
              <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-white"></div>
            ) : (
              <Search className="w-4 h-4" />
            )}
          </Button>
        </div>
      </div>

      {/* Advanced Filters - Inspirado en Azure Artifacts */}
      {showFilters && (
        <Card className="p-6">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-lg font-semibold text-gray-900">Advanced Filters</h3>
            <Button
              variant="ghost"
              size="sm"
              onClick={clearFilters}
              className="text-gray-600"
            >
              Clear All
            </Button>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
            {/* Package Type Filter */}
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-2">
                <Package className="w-4 h-4 inline mr-1" />
                Package Type
              </label>
              <select
                value={filters.type}
                onChange={(e) => setFilters({ ...filters, type: e.target.value })}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-primary-500"
              >
                <option value="">All Types</option>
                <option value="maven">Maven</option>
                <option value="npm">npm</option>
                <option value="pypi">PyPI</option>
              </select>
            </div>

            {/* Repository Filter */}
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-2">
                <Hash className="w-4 h-4 inline mr-1" />
                Repository
              </label>
              <select
                value={filters.repositoryId}
                onChange={(e) => setFilters({ ...filters, repositoryId: e.target.value })}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-primary-500"
              >
                <option value="">All Repositories</option>
                {repositories.map((repo) => (
                  <option key={repo.id} value={repo.id}>
                    {repo.name}
                  </option>
                ))}
              </select>
            </div>

            {/* Sort By */}
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-2">
                Sort By
              </label>
              <select
                value={filters.sortBy}
                onChange={(e) => setFilters({ ...filters, sortBy: e.target.value as any })}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-primary-500"
              >
                <option value="name">Name</option>
                <option value="createdAt">Date Created</option>
                <option value="downloadCount">Download Count</option>
              </select>
            </div>

            {/* Sort Order */}
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-2">
                Order
              </label>
              <select
                value={filters.sortOrder}
                onChange={(e) => setFilters({ ...filters, sortOrder: e.target.value as any })}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-primary-500"
              >
                <option value="asc">Ascending</option>
                <option value="desc">Descending</option>
              </select>
            </div>
          </div>

          {/* Active Filters Display */}
          {hasActiveFilters && (
            <div className="mt-4 pt-4 border-t border-gray-200">
              <div className="flex items-center space-x-2 flex-wrap">
                <span className="text-sm text-gray-600">Active filters:</span>
                {filters.type && (
                  <Badge variant="secondary" className="flex items-center space-x-1">
                    <span>Type: {filters.type}</span>
                    <button
                      onClick={() => setFilters({ ...filters, type: '' })}
                      className="ml-1 hover:text-gray-700"
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </Badge>
                )}
                {filters.repositoryId && (
                  <Badge variant="secondary" className="flex items-center space-x-1">
                    <span>Repository: {repositories.find(r => r.id === filters.repositoryId)?.name}</span>
                    <button
                      onClick={() => setFilters({ ...filters, repositoryId: '' })}
                      className="ml-1 hover:text-gray-700"
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </Badge>
                )}
              </div>
            </div>
          )}
        </Card>
      )}

      {/* Search Tips - Inspirado en JFrog */}
      {!query && (
        <Card className="p-4 bg-blue-50 border-blue-200">
          <div className="flex items-start space-x-3">
            <Search className="w-5 h-5 text-blue-600 mt-0.5" />
            <div className="text-sm text-blue-800">
              <p className="font-medium mb-1">Search Tips:</p>
              <ul className="space-y-1 text-blue-700">
                <li>• Use quotes for exact matches: "spring boot"</li>
                <li>• Combine terms with AND/OR: react AND typescript</li>
                <li>• Use wildcards: spring-*</li>
                <li>• Filter by package type, repository, or date range</li>
              </ul>
            </div>
          </div>
        </Card>
      )}
    </div>
  );
};

export default AdvancedSearch;