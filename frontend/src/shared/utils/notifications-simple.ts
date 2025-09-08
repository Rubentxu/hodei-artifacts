/**
 * Sistema simple de notificaciones para demostraciones
 * Reemplaza temporalmente el sistema de toast complejo
 */

// Funciones simples de notificación que usan console.log y alert
export const toast = {
  success: (message: string) => {
    console.log(`✅ SUCCESS: ${message}`);
    // También podemos usar alert para demostraciones visuales
    if (typeof window !== 'undefined' && window.alert) {
      // No usar alert en producción, solo para demo
      setTimeout(() => alert(`✅ Success: ${message}`), 100);
    }
  },
  
  error: (message: string) => {
    console.error(`❌ ERROR: ${message}`);
    if (typeof window !== 'undefined' && window.alert) {
      setTimeout(() => alert(`❌ Error: ${message}`), 100);
    }
  },
  
  info: (message: string) => {
    console.log(`ℹ️ INFO: ${message}`);
  },
  
  warning: (message: string) => {
    console.warn(`⚠️ WARNING: ${message}`);
  },
};

// Hook simple que no hace nada pero mantiene la interfaz
export function useNotifications() {
  return {
    notifications: [],
    removeNotification: () => {},
    clear: () => {}
  };
}

// Componente vacío para mantener compatibilidad
export function NotificationContainer(): null {
  return null;
}