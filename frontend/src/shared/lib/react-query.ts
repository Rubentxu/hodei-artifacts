import { QueryClient } from '@tanstack/react-query';

// Create a client
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Time in milliseconds after data is considered stale
      staleTime: 5 * 60 * 1000, // 5 minutes
      // Time in milliseconds that unused/inactive cache data remains in memory
      gcTime: 10 * 60 * 1000, // 10 minutes (formerly cacheTime)
      // Retry failed requests
      retry: (failureCount, error: any) => {
        // Don't retry on 4xx errors (client errors)
        if (error?.status >= 400 && error?.status < 500) {
          return false;
        }
        // Retry up to 3 times for other errors
        return failureCount < 3;
      },
      // Retry delay (exponential backoff)
      retryDelay: attemptIndex => Math.min(1000 * 2 ** attemptIndex, 30000),
      // Refetch on window focus
      refetchOnWindowFocus: true,
      // Refetch on reconnect
      refetchOnReconnect: true,
      // Refetch on mount if data is stale
      refetchOnMount: true,
    },
    mutations: {
      // Retry failed mutations
      retry: 1,
      // Retry delay for mutations
      retryDelay: 1000,
    },
  },
});

// Query invalidation helpers
export const invalidateQueries = {
  // Invalidate all queries
  all: () => queryClient.invalidateQueries(),

  // Invalidate specific query patterns
  users: () => queryClient.invalidateQueries({ queryKey: ['users'] }),
  user: (id: string) =>
    queryClient.invalidateQueries({ queryKey: ['users', id] }),

  repositories: () =>
    queryClient.invalidateQueries({ queryKey: ['repositories'] }),
  repository: (id: string) =>
    queryClient.invalidateQueries({ queryKey: ['repositories', id] }),

  artifacts: () => queryClient.invalidateQueries({ queryKey: ['artifacts'] }),
  artifact: (id: string) =>
    queryClient.invalidateQueries({ queryKey: ['artifacts', id] }),

  search: () => queryClient.invalidateQueries({ queryKey: ['search'] }),
};

// Cache management helpers
export const cacheHelpers = {
  // Clear all cache
  clear: () => queryClient.clear(),

  // Remove specific queries from cache
  remove: (queryKey: any[]) => queryClient.removeQueries({ queryKey }),

  // Get cached data
  getData: <T>(queryKey: any[]): T | undefined => {
    return queryClient.getQueryData<T>(queryKey);
  },

  // Set cached data
  setData: <T>(queryKey: any[], data: T) => {
    queryClient.setQueryData<T>(queryKey, data);
  },

  // Prefetch data
  prefetch: async (queryKey: any[], queryFn: () => Promise<any>) => {
    await queryClient.prefetchQuery({
      queryKey,
      queryFn,
    });
  },
};
