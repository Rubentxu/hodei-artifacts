import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export interface Notification {
  id: string;
  type: 'success' | 'error' | 'warning' | 'info';
  title: string;
  message: string;
  duration?: number;
  createdAt: Date;
}

interface NotificationState {
  notifications: Notification[];
  addNotification: (notification: Omit<Notification, 'id' | 'createdAt'>) => void;
  removeNotification: (id: string) => void;
  clearAllNotifications: () => void;
  clearExpiredNotifications: () => void;
}

export const useNotificationStore = create<NotificationState>()(
  persist(
    (set, get) => ({
      notifications: [],

      addNotification: (notification) => {
        const newNotification: Notification = {
          ...notification,
          id: Math.random().toString(36).substr(2, 9),
          createdAt: new Date(),
          duration: notification.duration || 5000,
        };

        set((state) => ({
          notifications: [...state.notifications, newNotification],
        }));

        // Auto-remove after duration
        if (newNotification.duration > 0) {
          setTimeout(() => {
            get().removeNotification(newNotification.id);
          }, newNotification.duration);
        }
      },

      removeNotification: (id) => {
        set((state) => ({
          notifications: state.notifications.filter((n) => n.id !== id),
        }));
      },

      clearAllNotifications: () => {
        set({ notifications: [] });
      },

      clearExpiredNotifications: () => {
        const now = new Date();
        set((state) => ({
          notifications: state.notifications.filter(
            (n) => now.getTime() - n.createdAt.getTime() < (n.duration || 5000)
          ),
        }));
      },
    }),
    {
      name: 'notification-storage',
      partialize: (state) => ({
        notifications: state.notifications.filter(
          (n) => n.type === 'error' || n.type === 'warning'
        ),
      }),
    }
  )
);

// Helper functions for common notification types
export const notificationService = {
  success: (title: string, message: string, duration?: number) => {
    useNotificationStore.getState().addNotification({
      type: 'success',
      title,
      message,
      duration,
    });
  },

  error: (title: string, message: string, duration?: number) => {
    useNotificationStore.getState().addNotification({
      type: 'error',
      title,
      message,
      duration: duration || 8000, // Longer duration for errors
    });
  },

  warning: (title: string, message: string, duration?: number) => {
    useNotificationStore.getState().addNotification({
      type: 'warning',
      title,
      message,
      duration,
    });
  },

  info: (title: string, message: string, duration?: number) => {
    useNotificationStore.getState().addNotification({
      type: 'info',
      title,
      message,
      duration,
    });
  },
};