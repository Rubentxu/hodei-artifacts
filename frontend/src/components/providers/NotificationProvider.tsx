import { useNotificationStore } from '@/shared/stores/notificationStore';
import {
  Toast,
  ToastClose,
  ToastDescription,
  ToastProvider,
  ToastTitle,
  ToastViewport,
} from '@/components/ui/toast';

export function NotificationProvider() {
  const { notifications, dismissNotification } = useNotificationStore();

  return (
    <ToastProvider>
      {notifications.map(({ id, type, title, message }) => (
        <Toast key={id} variant={type === 'error' ? 'destructive' : 'default'}>
          <div className="grid gap-1">
            <ToastTitle>{title}</ToastTitle>
            {message && <ToastDescription>{message}</ToastDescription>}
          </div>
          <ToastClose onClick={() => dismissNotification(id)} />
        </Toast>
      ))}
      <ToastViewport />
    </ToastProvider>
  );
}
