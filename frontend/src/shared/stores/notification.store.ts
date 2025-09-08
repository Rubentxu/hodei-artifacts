import { useNotifications } from './ui.store';

export const notificationService = {
  success: (title: string, message?: string, duration?: number) => {
    const { showSuccess } = useNotifications();
    showSuccess(title, message);
  },

  error: (title: string, message?: string) => {
    const { showError } = useNotifications();
    showError(title, message);
  },

  warning: (title: string, message?: string) => {
    const { showWarning } = useNotifications();
    showWarning(title, message);
  },

  info: (title: string, message?: string) => {
    const { showInfo } = useNotifications();
    showInfo(title, message);
  },
};
