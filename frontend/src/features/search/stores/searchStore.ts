import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { SearchFilters } from '../types/search.types';

export type FavoriteSearch = {
  id: string;
  name: string;
  query: string;
  filters: Omit<SearchFilters, 'page' | 'query'>;
};

type SearchState = {
  filters: Omit<SearchFilters, 'page' | 'query'>;
  query: string;
  history: string[];
  favorites: FavoriteSearch[];
};

type SearchActions = {
  setQuery: (query: string) => void;
  setFilters: (filters: Partial<SearchState['filters']>) => void;
  loadFavorite: (favorite: FavoriteSearch) => void;
  addSearchToHistory: (query: string) => void;
  addFavorite: (favorite: Omit<FavoriteSearch, 'id'>) => void;
  removeFavorite: (id: string) => void;
  clearFilters: () => void;
  reset: () => void;
};

const initialState: SearchState = {
  filters: {},
  query: '',
  history: [],
  favorites: [],
};

export const useSearchStore = create<SearchState & SearchActions>()(
  persist(
    (set, get) => ({
      ...initialState,
      setQuery: query => set({ query }),
      setFilters: newFilters =>
        set(state => ({ filters: { ...state.filters, ...newFilters } })),
      loadFavorite: favorite => {
        set({ query: favorite.query, filters: favorite.filters });
      },
      addSearchToHistory: query => {
        if (!query.trim()) return;
        const lowerCaseQuery = query.toLowerCase();
        const history = [
          lowerCaseQuery,
          ...get().history.filter(h => h !== lowerCaseQuery),
        ];
        set({ history: history.slice(0, 10) });
      },
      addFavorite: favorite => {
        const newFavorite: FavoriteSearch = {
          ...favorite,
          id: new Date().toISOString(),
        };
        set(state => ({ favorites: [...state.favorites, newFavorite] }));
      },
      removeFavorite: id => {
        set(state => ({ favorites: state.favorites.filter(f => f.id !== id) }));
      },
      clearFilters: () =>
        set(state => ({ filters: { ...initialState.filters } })),
      reset: () => set(initialState),
    }),
    {
      name: 'hodei-search-storage',
      partialize: state => ({
        history: state.history,
        favorites: state.favorites,
      }),
    }
  )
);
