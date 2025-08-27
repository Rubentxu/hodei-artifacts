import { describe, it, expect, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useUIStore } from '../ui.store';

// Test component to use the store
describe('UI Store', () => {
  beforeEach(() => {
    // Reset store state before each test
    const { result } = renderHook(() => useUIStore());
    act(() => {
      // Skip theme setting to avoid window.matchMedia issues in tests
      result.current.setSidebarOpen(false);
      result.current.setIsLoading(false);
      result.current.closeModal();
      result.current.clearNotifications();
      result.current.setGlobalSearchQuery('');
    });
  });

  it('has correct initial state', () => {
    const { result } = renderHook(() => useUIStore());

    expect(result.current.isSidebarOpen).toBe(false);
    expect(result.current.isLoading).toBe(false);
    expect(result.current.activeModal).toBeNull();
    expect(result.current.notifications).toEqual([]);
    expect(result.current.globalSearchQuery).toBe('');
  });

  it('toggles sidebar', () => {
    const { result } = renderHook(() => useUIStore());

    // Toggle from closed to open
    act(() => {
      result.current.toggleSidebar();
    });
    expect(result.current.isSidebarOpen).toBe(true);

    // Toggle from open to closed
    act(() => {
      result.current.toggleSidebar();
    });
    expect(result.current.isSidebarOpen).toBe(false);
  });

  it('sets sidebar open state', () => {
    const { result } = renderHook(() => useUIStore());

    act(() => {
      result.current.setSidebarOpen(true);
    });
    expect(result.current.isSidebarOpen).toBe(true);

    act(() => {
      result.current.setSidebarOpen(false);
    });
    expect(result.current.isSidebarOpen).toBe(false);
  });

  it('manages loading state', () => {
    const { result } = renderHook(() => useUIStore());

    act(() => {
      result.current.setIsLoading(true);
    });
    expect(result.current.isLoading).toBe(true);

    act(() => {
      result.current.setIsLoading(false);
    });
    expect(result.current.isLoading).toBe(false);
  });

  it('manages modal state', () => {
    const { result } = renderHook(() => useUIStore());

    act(() => {
      result.current.openModal('test-modal');
    });
    expect(result.current.activeModal).toBe('test-modal');

    act(() => {
      result.current.closeModal();
    });
    expect(result.current.activeModal).toBeNull();
  });

  it('manages notifications', () => {
    const { result } = renderHook(() => useUIStore());

    // Add notification
    act(() => {
      result.current.addNotification({
        type: 'success',
        title: 'Test Success',
        message: 'This is a test notification',
      });
    });

    expect(result.current.notifications).toHaveLength(1);
    expect(result.current.notifications[0].title).toBe('Test Success');
    expect(result.current.notifications[0].type).toBe('success');

    // Remove notification
    const notificationId = result.current.notifications[0].id;
    act(() => {
      result.current.removeNotification(notificationId);
    });

    expect(result.current.notifications).toHaveLength(0);

    // Clear all notifications
    act(() => {
      result.current.addNotification({ type: 'info', title: 'Notification 1' });
      result.current.addNotification({
        type: 'warning',
        title: 'Notification 2',
      });
    });

    expect(result.current.notifications).toHaveLength(2);

    act(() => {
      result.current.clearNotifications();
    });

    expect(result.current.notifications).toHaveLength(0);
  });

  it('manages global search query', () => {
    const { result } = renderHook(() => useUIStore());

    act(() => {
      result.current.setGlobalSearchQuery('test query');
    });
    expect(result.current.globalSearchQuery).toBe('test query');

    act(() => {
      result.current.setGlobalSearchQuery('');
    });
    expect(result.current.globalSearchQuery).toBe('');
  });

  it('provides all expected actions', () => {
    const { result } = renderHook(() => useUIStore());

    expect(typeof result.current.setTheme).toBe('function');
    expect(typeof result.current.setIsLoading).toBe('function');
    expect(typeof result.current.setSidebarOpen).toBe('function');
    expect(typeof result.current.toggleSidebar).toBe('function');
    expect(typeof result.current.openModal).toBe('function');
    expect(typeof result.current.closeModal).toBe('function');
    expect(typeof result.current.addNotification).toBe('function');
    expect(typeof result.current.removeNotification).toBe('function');
    expect(typeof result.current.clearNotifications).toBe('function');
    expect(typeof result.current.setGlobalSearchQuery).toBe('function');
    expect(typeof result.current.setIsMobile).toBe('function');
  });

  it('handles mobile state', () => {
    const { result } = renderHook(() => useUIStore());

    act(() => {
      result.current.setIsMobile(true);
    });
    expect(result.current.isMobile).toBe(true);

    act(() => {
      result.current.setIsMobile(false);
    });
    expect(result.current.isMobile).toBe(false);
  });
});
