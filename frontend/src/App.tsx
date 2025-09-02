import { useEffect } from 'react';
import { RouterProvider } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { router } from './router';
import { NotificationProvider } from './components/providers/NotificationProvider';
import { useAuthStore } from './shared/stores/auth.store';

// Create a client
const queryClient = new QueryClient();

function App() {
  // Mock login for debugging purposes
  useEffect(() => {
    const mockUser = {
      id: '1',
      name: 'Ruben',
      email: 'ruben@example.com',
      role: 'admin',
      createdAt: new Date().toISOString(),
    };
    useAuthStore.getState().login(mockUser, 'mock-token');
  }, []);

  return (
    <>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
      <NotificationProvider />
    </>
  );
}

export default App;
