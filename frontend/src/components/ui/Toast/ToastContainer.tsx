import { useNotificationStore } from '@/shared/stores';
import { Toast } from './Toast';

export const ToastContainer = () => {
  const { notifications, removeNotification } = useNotificationStore();

  return (
    <div
      className="fixed inset-0 flex flex-col items-end justify-start p-4 pointer-events-none z-50"
      aria-live="assertive"
    >
      <div className="w-full max-w-sm space-y-2">
        {notifications.map(notification => (
          <Toast
            key={notification.id}
            notification={notification}
            onClose={() => removeNotification(notification.id)}
          />
        ))}
      </div>
    </div>
  );
};
