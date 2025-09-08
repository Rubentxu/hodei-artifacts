/**
 * Sistema simple de notificaciones para demostraciones
 * Reemplaza temporalmente el sistema de toast complejo
 */

export interface Notification {
  id: string;
  type: 'success' | 'error' | 'info' | 'warning';
  message: string;
  timestamp: Date;
}

class NotificationManager {
  private notifications: Notification[] = [];
  private listeners: Array<(notifications: Notification[]) => void> = [];

  // Método para suscribirse a cambios
  subscribe(callback: (notifications: Notification[]) => void): () => void {
    this.listeners.push(callback);
    // Retornar función de limpieza
    return () => {
      const index = this.listeners.indexOf(callback);
      if (index > -1) {
        this.listeners.splice(index, 1);
      }
    };
  }

  // Notificar a todos los suscriptores
  private notify(): void {
    this.listeners.forEach(callback => callback([...this.notifications]));
  }

  // Añadir notificación
  addNotification(type: Notification['type'], message: string): void {
    const notification: Notification = {
      id: Math.random().toString(36).substring(2, 9),
      type,
      message,
      timestamp: new Date()
    };

    this.notifications.push(notification);
    this.notify();

    // Auto-remover después de 5 segundos
    setTimeout(() => {
      this.removeNotification(notification.id);
    }, 5000);
  }

  // Remover notificación
  removeNotification(id: string): void {
    this.notifications = this.notifications.filter(n => n.id !== id);
    this.notify();
  }

  // Limpiar todas las notificaciones
  clear(): void {
    this.notifications = [];
    this.notify();
  }

  // Obtener notificaciones actuales
  getNotifications(): Notification[] {
    return [...this.notifications];
  }
}

// Instancia global
const notificationManager = new NotificationManager();

// Funciones de conveniencia
export const toast = {
  success: (message: string) => notificationManager.addNotification('success', message),
  error: (message: string) => notificationManager.addNotification('error', message),
  info: (message: string) => notificationManager.addNotification('info', message),
  warning: (message: string) => notificationManager.addNotification('warning', message),
};

// Hook para usar notificaciones en componentes React
export function useNotifications() {
  const [notifications, setNotifications] = React.useState<Notification[]>([]);

  React.useEffect(() => {
    // Suscribirse a cambios
    const unsubscribe = notificationManager.subscribe(setNotifications);
    
    // Obtener notificaciones actuales
    setNotifications(notificationManager.getNotifications());

    // Limpiar suscripción
    return unsubscribe;
  }, []);

  const removeNotification = (id: string) => {
    notificationManager.removeNotification(id);
  };

  return {
    notifications,
    removeNotification,
    clear: () => notificationManager.clear()
  };
}

// Componente de notificaciones
export function NotificationContainer(): JSX.Element | null {
  const { notifications, removeNotification } = useNotifications();

  if (notifications.length === 0) return null;

  return (
    <div className="fixed top-4 right-4 z-50 space-y-2 max-w-sm">
      {notifications.map((notification) => (
        <NotificationItem
          key={notification.id}
          notification={notification}
          onClose={() => removeNotification(notification.id)}
        />
      ))}
    </div>
  );
}

interface NotificationItemProps {
  notification: Notification;
  onClose: () => void;
}

function NotificationItem({ notification, onClose }: NotificationItemProps): JSX.Element {
  const getStyles = () => {
    switch (notification.type) {
      case 'success':
        return 'bg-green-50 border-green-200 text-green-800';
      case 'error':
        return 'bg-red-50 border-red-200 text-red-800';
      case 'warning':
        return 'bg-yellow-50 border-yellow-200 text-yellow-800';
      case 'info':
        return 'bg-blue-50 border-blue-200 text-blue-800';
      default:
        return 'bg-gray-50 border-gray-200 text-gray-800';
    }
  };

  const getIcon = () => {
    switch (notification.type) {
      case 'success':
        return '✅';
      case 'error':
        return '❌';
      case 'warning':
        return '⚠️';
      case 'info':
        return 'ℹ️';
      default:
        return '📢';
    }
  };

  return (
    <div className={`border rounded-lg p-3 shadow-lg ${getStyles()} animate-slide-in-right`}>
      <div className="flex items-start justify-between">
        <div className="flex items-start space-x-2">
          <span className="text-lg">{getIcon()}</span>
          <div className="flex-1">
            <p className="text-sm font-medium">{notification.message}</p>
            <p className="text-xs opacity-75">
              {notification.timestamp.toLocaleTimeString()}
            </p>
          </div>
        </div>
        <button
          onClick={onClose}
          className="ml-2 text-lg leading-none hover:opacity-75"
          aria-label="Close notification"
        >
          ×
        </button>
      </div>
    </div>
  );
}

// Estilos CSS para las animaciones
const styles = `
@keyframes slide-in-right {
  from {
    transform: translateX(100%);
    opacity: 0;
  }
  to {
    transform: translateX(0);
    opacity: 1;
  }
}

.animate-slide-in-right {
  animation: slide-in-right 0.3s ease-out;
}
`;

// Añadir estilos al documento si no existen
if (typeof document !== 'undefined') {
  const styleElement = document.createElement('style');
  styleElement.textContent = styles;
  document.head.appendChild(styleElement);
}

// Exportar el manager para uso avanzado
export { notificationManager };