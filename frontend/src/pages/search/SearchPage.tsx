import React, { useMemo, useEffect } from 'react';
import { Star, Trash2 } from 'lucide-react';
import {
  useInfiniteSearch,
  useSearch,
  useSearchStore,
  FavoriteSearch,
} from '../../features/search';
import { useDebounce } from '../../shared/hooks/useDebounce';
import { useInView } from 'react-intersection-observer';
import { Input } from '../../components/ui/input';
import { DataTable } from '../../components/layout/data-table';
import { Badge } from '../../components/ui/badge';
import { Spinner } from '../../components/ui/spinner';
import { Checkbox } from '../../components/ui/checkbox';
import { Button } from '../../components/ui/button';
import { PageHeader } from '../../components/layout/page-header';
import { SearchResultHighlighter } from '../../features/search/components/SearchResultHighlighter';
import type { Column } from '../../components/layout/data-table';
import type {
  SearchResultItem,
  SearchFilters as SearchFiltersType,
} from '../../features/search';

import { PageHeader } from '../../components/layout/page-header';

const FavoritesList = React.memo(
  ({
    favorites,
    onLoad,
    onRemove,
  }: {
    favorites: FavoriteSearch[];
    onLoad: (fav: FavoriteSearch) => void;
    onRemove: (id: string) => void;
  }) => {
    if (favorites.length === 0) return null;

    return (
      <div className="mt-6">
        <h4 className="font-medium text-sm mb-2">Favorite Searches</h4>
        <div className="space-y-2">
          {favorites.map(fav => (
            <div
              key={fav.id}
              className="flex items-center justify-between text-sm text-blue-600 hover:underline"
            >
              <button onClick={() => onLoad(fav)} className="truncate pr-2">
                {fav.name}
              </button>
              <button
                onClick={() => onRemove(fav.id)}
                className="text-red-500 hover:text-red-700"
              >
                <Trash2 size={14} />
              </button>
            </div>
          ))}
        </div>
      </div>
    );
  }
);

const SearchFilters = React.memo(
  ({
    facets,
    onFilterChange,
    appliedFilters,
    favorites,
    loadFavorite,
    removeFavorite,
  }) => {
    if (!facets)
      return (
        <aside className="w-1/4 p-4 border-r">
          <Spinner size="sm" />
        </aside>
      );

    return (
      <aside className="w-1/4 p-4 border-r">
        <h3 className="font-semibold mb-4">Filters</h3>
        <div className="space-y-6">
          {Object.entries(facets).map(([facetKey, values]) => (
            <div key={facetKey}>
              <h4 className="font-medium text-sm capitalize mb-2">
                {facetKey.replace('_', ' ')}
              </h4>
              <div className="space-y-2">
                {values.map(({ value, count }) => (
                  <div key={value} className="flex items-center">
                    <Checkbox
                      id={`${facetKey}-${value}`}
                      checked={appliedFilters[facetKey]?.includes(value)}
                      onCheckedChange={checked =>
                        onFilterChange(facetKey, value, checked)
                      }
                    />
                    <label
                      htmlFor={`${facetKey}-${value}`}
                      className="ml-2 text-sm text-gray-600 cursor-pointer"
                    >
                      {value}{' '}
                      <span className="text-xs text-gray-400">({count})</span>
                    </label>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
        <FavoritesList
          favorites={favorites}
          onLoad={loadFavorite}
          onRemove={removeFavorite}
        />
      </aside>
    );
  }
);

const SearchResults = React.memo(
  ({ filters }: { filters: Omit<SearchFiltersType, 'page'> }) => {
    const {
      data,
      isLoading,
      error,
      fetchNextPage,
      hasNextPage,
      isFetchingNextPage,
    } = useInfiniteSearch(filters);
    const { ref, inView } = useInView();

    useEffect(() => {
      if (inView && hasNextPage) {
        fetchNextPage();
      }
    }, [inView, hasNextPage, fetchNextPage]);

    const allResults = useMemo(
      () => data?.pages.flatMap(page => page.results) ?? [],
      [data]
    );

    const columns: Column<SearchResultItem>[] = [
      {
        key: 'name',
        title: 'Name',
        sortable: true,
        render: (_, row) => (
          <div>
            <SearchResultHighlighter text={row.name} query={filters.query} />
            <p className="text-sm text-muted-foreground">
              <SearchResultHighlighter
                text={row.description}
                query={filters.query}
              />
            </p>
          </div>
        ),
      },
      {
        key: 'type',
        title: 'Type',
        render: (_, row) => <Badge variant="primary">{row.type}</Badge>,
      },
      { key: 'repository', title: 'Repository', sortable: true },
      { key: 'version', title: 'Version' },
      {
        key: 'size',
        title: 'Size',
        render: size => `${(size / 1024).toFixed(2)} KB`,
      },
    ];

    if (isLoading && filters.query) {
      return (
        <div className="flex-1 p-4 flex justify-center items-center">
          <Spinner />
        </div>
      );
    }

    if (error) {
      return (
        <div className="flex-1 p-4 text-red-500">Error: {error.message}</div>
      );
    }

    if (!filters.query) {
      return (
        <div className="flex-1 p-4 text-gray-500">
          Enter a search term to begin.
        </div>
      );
    }

    if (!allResults.length) {
      return (
        <div className="flex-1 p-4 text-gray-500">
          No results found for "{filters.query}".
        </div>
      );
    }

    return (
      <div className="flex-1 p-4">
        <DataTable columns={columns} data={allResults} />
        <div className="mt-4 flex justify-center">
          <Button
            ref={ref}
            onClick={() => fetchNextPage()}
            disabled={!hasNextPage || isFetchingNextPage}
          >
            {isFetchingNextPage
              ? 'Loading more...'
              : hasNextPage
                ? 'Load More'
                : 'Nothing more to load'}
          </Button>
        </div>
      </div>
    );
  }
);

const SearchPage = () => {
  const {
    query,
    filters,
    favorites,
    setQuery,
    setFilters,
    loadFavorite,
    addSearchToHistory,
    addFavorite,
    removeFavorite,
  } = useSearchStore();

  const debouncedQuery = useDebounce(query, 500);

  const searchFilters = useMemo(
    () => ({ ...filters, query: debouncedQuery }),
    [filters, debouncedQuery]
  );

  useEffect(() => {
    if (debouncedQuery) {
      addSearchToHistory(debouncedQuery);
    }
  }, [debouncedQuery, addSearchToHistory]);

  const handleFilterChange = (
    key: string,
    value: string,
    isEnabled: boolean
  ) => {
    const currentValues = filters[key] ? [...filters[key]] : [];
    let newValues;
    if (isEnabled) {
      if (!currentValues.includes(value)) newValues = [...currentValues, value];
    } else {
      newValues = currentValues.filter(v => v !== value);
    }
    setFilters({ ...filters, [key]: newValues });
  };

  const handleSaveFavorite = () => {
    const name = prompt('Enter a name for this search:');
    if (name) {
      addFavorite({ name, query, filters });
    }
  };

  const { data: facetData } = useSearch(searchFilters);

  return (
    <div className="p-8">
      <PageHeader
        title="Advanced Search"
        subtitle="Find packages, artifacts, and resources across all repositories."
      />
      <div className="flex items-center gap-2 mb-4">
        <Input
          placeholder="Search packages, keywords, descriptions..."
          value={query}
          onChange={e => setQuery(e.target.value)}
          className="w-full text-lg p-4"
        />
        <Button
          onClick={handleSaveFavorite}
          variant="ghost"
          disabled={!query.trim()}
        >
          <Star size={18} className="mr-2" /> Save Search
        </Button>
      </div>
      <div className="flex">
        <SearchFilters
          facets={facetData?.facets}
          onFilterChange={handleFilterChange}
          appliedFilters={filters}
          favorites={favorites}
          loadFavorite={loadFavorite}
          removeFavorite={removeFavorite}
        />
        <SearchResults filters={searchFilters} />
      </div>
    </div>
  );
};

export default SearchPage;
