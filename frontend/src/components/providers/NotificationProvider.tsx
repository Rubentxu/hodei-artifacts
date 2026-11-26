import { useNotifications } from '@/shared/stores/ui.store';
import { Toast } from '@/components/ui/Toast';

export function NotificationProvider() {
  const { notifications, removeNotification } = useNotifications();

  return (
    <div className="pointer-events-none fixed top-0 right-0 z-50 p-4 space-y-2 w-full max-w-sm">
      {notifications.map(notification => (
        <div key={notification.id} className="pointer-events-auto">
          <Toast
            notification={notification}
            onClose={() => removeNotification(notification.id)}
          />
        </div>
      ))}
    </div>
  );
}
