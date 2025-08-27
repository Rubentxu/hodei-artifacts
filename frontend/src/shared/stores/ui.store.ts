import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import {
  THEMES,
  STORAGE_KEYS,
  type Theme,
  type NotificationType,
} from '@/shared/constants';

export interface Notification {
  id: string;
  type: NotificationType;
  title: string;
  message?: string;
  duration?: number;
  action?: {
    label: string;
    onClick: () => void;
  };
}

interface UIState {
  // Theme state
  theme: Theme;
  setTheme: (theme: Theme) => void;

  // Loading states
  isLoading: boolean;
  setIsLoading: (loading: boolean) => void;

  // Sidebar state
  isSidebarOpen: boolean;
  setSidebarOpen: (open: boolean) => void;
  toggleSidebar: () => void;

  // Modal state
  activeModal: string | null;
  openModal: (modalId: string) => void;
  closeModal: () => void;

  // Notifications
  notifications: Notification[];
  addNotification: (notification: Omit<Notification, 'id'>) => void;
  removeNotification: (id: string) => void;
  clearNotifications: () => void;

  // Search state
  globalSearchQuery: string;
  setGlobalSearchQuery: (query: string) => void;

  // Responsive helpers
  isMobile: boolean;
  setIsMobile: (mobile: boolean) => void;
}

export const useUIStore = create<UIState>()(
  persist(
    (set, get) => ({
      // Theme
      theme: THEMES.SYSTEM,
      setTheme: theme => {
        set({ theme });
        // Apply theme to document
        if (
          theme === THEMES.DARK ||
          (theme === THEMES.SYSTEM &&
            window.matchMedia('(prefers-color-scheme: dark)').matches)
        ) {
          document.documentElement.classList.add('dark');
        } else {
          document.documentElement.classList.remove('dark');
        }
      },

      // Loading
      isLoading: false,
      setIsLoading: isLoading => set({ isLoading }),

      // Sidebar
      isSidebarOpen: true,
      setSidebarOpen: isSidebarOpen => set({ isSidebarOpen }),
      toggleSidebar: () =>
        set(state => ({ isSidebarOpen: !state.isSidebarOpen })),

      // Modal
      activeModal: null,
      openModal: modalId => set({ activeModal: modalId }),
      closeModal: () => set({ activeModal: null }),

      // Notifications
      notifications: [],
      addNotification: notification => {
        const id = Math.random().toString(36).substr(2, 9);
        const newNotification: Notification = {
          ...notification,
          id,
          duration: notification.duration ?? 5000,
        };

        set(state => ({
          notifications: [...state.notifications, newNotification],
        }));

        // Auto-remove notification after duration
        if (newNotification.duration > 0) {
          setTimeout(() => {
            get().removeNotification(id);
          }, newNotification.duration);
        }
      },
      removeNotification: id =>
        set(state => ({
          notifications: state.notifications.filter(n => n.id !== id),
        })),
      clearNotifications: () => set({ notifications: [] }),

      // Search
      globalSearchQuery: '',
      setGlobalSearchQuery: globalSearchQuery => set({ globalSearchQuery }),

      // Responsive
      isMobile: false,
      setIsMobile: isMobile => set({ isMobile }),
    }),
    {
      name: STORAGE_KEYS.USER_PREFERENCES,
      partialize: state => ({
        theme: state.theme,
        isSidebarOpen: state.isSidebarOpen,
      }),
    }
  )
);

// Convenience hooks for specific UI actions
export const useTheme = () => {
  const theme = useUIStore(state => state.theme);
  const setTheme = useUIStore(state => state.setTheme);
  return { theme, setTheme };
};

export const useNotifications = () => {
  const notifications = useUIStore(state => state.notifications);
  const addNotification = useUIStore(state => state.addNotification);
  const removeNotification = useUIStore(state => state.removeNotification);
  const clearNotifications = useUIStore(state => state.clearNotifications);

  // Convenience methods for different notification types
  const showSuccess = (title: string, message?: string) => {
    addNotification({ type: 'success', title, message });
  };

  const showError = (title: string, message?: string) => {
    addNotification({ type: 'error', title, message, duration: 0 }); // Don't auto-dismiss errors
  };

  const showWarning = (title: string, message?: string) => {
    addNotification({ type: 'warning', title, message });
  };

  const showInfo = (title: string, message?: string) => {
    addNotification({ type: 'info', title, message });
  };

  return {
    notifications,
    addNotification,
    removeNotification,
    clearNotifications,
    showSuccess,
    showError,
    showWarning,
    showInfo,
  };
};

export const useSidebar = () => {
  const isSidebarOpen = useUIStore(state => state.isSidebarOpen);
  const setSidebarOpen = useUIStore(state => state.setSidebarOpen);
  const toggleSidebar = useUIStore(state => state.toggleSidebar);
  return { isSidebarOpen, setSidebarOpen, toggleSidebar };
};

export const useModal = () => {
  const activeModal = useUIStore(state => state.activeModal);
  const openModal = useUIStore(state => state.openModal);
  const closeModal = useUIStore(state => state.closeModal);
  return { activeModal, openModal, closeModal };
};

export const useGlobalSearch = () => {
  const globalSearchQuery = useUIStore(state => state.globalSearchQuery);
  const setGlobalSearchQuery = useUIStore(state => state.setGlobalSearchQuery);
  return { globalSearchQuery, setGlobalSearchQuery };
};
