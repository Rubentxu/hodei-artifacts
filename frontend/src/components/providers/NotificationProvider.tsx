import { useNotifications } from '@/shared/stores/ui.store';
import { Toast } from '@/components/ui/Toast';

export function NotificationProvider() {
  const { notifications, removeNotification } = useNotifications();

  return (
    <div className="fixed top-0 right-0 z-50 p-4 space-y-2 w-full max-w-sm">
      {notifications.map(notification => (
        <Toast
          key={notification.id}
          notification={notification}
          onClose={() => removeNotification(notification.id)}
        />
      ))}
    </div>
  );
}
