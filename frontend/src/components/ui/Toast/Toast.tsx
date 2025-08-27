import { useEffect, useState } from 'react';
import { Notification } from '@/shared/stores/notification.store';
import { useNotificationStore } from '@/shared/stores';

interface ToastProps {
  notification: Notification;
  onClose: () => void;
}

const ToastIcon = ({ type }: { type: Notification['type'] }) => {
  switch (type) {
    case 'success':
      return (
        <div className="flex-shrink-0 w-5 h-5 text-green-500">
          <svg fill="currentColor" viewBox="0 0 20 20">
            <path
              fillRule="evenodd"
              d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z"
              clipRule="evenodd"
            />
          </svg>
        </div>
      );
    case 'error':
      return (
        <div className="flex-shrink-0 w-5 h-5 text-red-500">
          <svg fill="currentColor" viewBox="0 0 20 20">
            <path
              fillRule="evenodd"
              d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z"
              clipRule="evenodd"
            />
          </svg>
        </div>
      );
    case 'warning':
      return (
        <div className="flex-shrink-0 w-5 h-5 text-yellow-500">
          <svg fill="currentColor" viewBox="0 0 20 20">
            <path
              fillRule="evenodd"
              d="M8.257 3.099c.765-1.36 2.722-1.36 3.486 0l5.58 9.92c.75 1.334-.213 2.98-1.742 2.98H4.42c-1.53 0-2.493-1.646-1.743-2.98l5.58-9.92zM11 13a1 1 0 11-2 0 1 1 0 012 0zm-1-8a1 1 0 00-1 1v3a1 1 0 002 0V6a1 1 0 00-1-1z"
              clipRule="evenodd"
            />
          </svg>
        </div>
      );
    case 'info':
      return (
        <div className="flex-shrink-0 w-5 h-5 text-blue-500">
          <svg fill="currentColor" viewBox="0 0 20 20">
            <path
              fillRule="evenodd"
              d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a1 1 0 000 2v3a1 1 0 001 1h1a1 1 0 100-2v-3a1 1 0 00-1-1H9z"
              clipRule="evenodd"
            />
          </svg>
        </div>
      );
    default:
      return null;
  }
};

const getToastStyles = (type: Notification['type']) => {
  const baseStyles = 'max-w-sm w-full bg-white shadow-lg rounded-lg pointer-events-auto ring-1 ring-black ring-opacity-5';
  
  switch (type) {
    case 'success':
      return `${baseStyles} border-l-4 border-green-400`;
    case 'error':
      return `${baseStyles} border-l-4 border-red-400`;
    case 'warning':
      return `${baseStyles} border-l-4 border-yellow-400`;
    case 'info':
      return `${baseStyles} border-l-4 border-blue-400`;
    default:
      return baseStyles;
  }
};

export const Toast = ({ notification, onClose }: ToastProps) => {
  const [isVisible, setIsVisible] = useState(false);
  const [progress, setProgress] = useState(100);

  useEffect(() => {
    setIsVisible(true);

    if (notification.duration && notification.duration > 0) {
      const interval = 50;
      const totalSteps = notification.duration / interval;
      const stepDecrement = 100 / totalSteps;

      const progressInterval = setInterval(() => {
        setProgress((prev) => Math.max(0, prev - stepDecrement));
      }, interval);

      return () => clearInterval(progressInterval);
    }
  }, [notification.duration]);

  useEffect(() => {
    if (progress <= 0) {
      setIsVisible(false);
      setTimeout(onClose, 300); // Wait for fade-out animation
    }
  }, [progress, onClose]);

  const handleClose = () => {
    setIsVisible(false);
    setTimeout(onClose, 300);
  };

  return (
    <div
      className={`
        transform transition-all duration-300 ease-in-out
        ${isVisible ? 'translate-y-0 opacity-100' : 'translate-y-2 opacity-0'}
      `}
    >
      <div className={getToastStyles(notification.type)}>
        <div className="p-4">
          <div className="flex items-start">
            <div className="flex-shrink-0">
              <ToastIcon type={notification.type} />
            </div>
            <div className="ml-3 flex-1 pt-0.5">
              <p className="text-sm font-medium text-gray-900">
                {notification.title}
              </p>
              <p className="mt-1 text-sm text-gray-500">
                {notification.message}
              </p>
            </div>
            <div className="ml-4 flex flex-shrink-0">
              <button
                onClick={handleClose}
                className="inline-flex text-gray-400 hover:text-gray-500 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500"
              >
                <span className="sr-only">Close</span>
                <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>
          </div>
        </div>
        {notification.duration && notification.duration > 0 && (
          <div className="w-full bg-gray-200 h-1">
            <div
              className="h-1 bg-gray-400 transition-all duration-50"
              style={{ width: `${progress}%` }}
            />
          </div>
        )}
      </div>
    </div>
  );
};