import { createBrowserRouter } from 'react-router-dom';

// Direct imports
import Login from '@/pages/Login/Login';
import Dashboard from '@/pages/Dashboard/Dashboard';
import Repositories from '@/pages/Repositories/Repositories';
import RepositoryDetail from '@/pages/RepositoryDetail/RepositoryDetail';
import NotFound from '@/pages/NotFound/NotFound';
import SearchPage from '@/pages/search/SearchPage';
import ProfilePage from '@/pages/profile/ProfilePage';
import TokensPage from '@/pages/settings/tokens/TokensPage';
import UsersPage from '@/pages/admin/users/UsersPage';
import PoliciesPage from '@/pages/settings/policies/PoliciesPage';

import { MainLayout } from '@/components/templates/MainLayout';

export const router = createBrowserRouter([
  {
    path: '/login', // Login page
    element: <Login />,
  },
  {
    element: <MainLayout />, // Layout for authenticated routes
    children: [
      {
        path: '/', // Dashboard at root for now
        element: <Dashboard />,
      },
      {
        path: '/repositories',
        element: <Repositories />,
      },
      {
        path: '/repositories/:id',
        element: <RepositoryDetail />,
      },
      {
        path: '/search',
        element: <SearchPage />,
      },
      {
        path: '/profile',
        element: <ProfilePage />,
      },
      {
        path: '/settings/tokens',
        element: <TokensPage />,
      },
      {
        path: '/admin/users',
        element: <UsersPage />,
      },
      {
        path: '/settings/policies',
        element: <PoliciesPage />,
      },
    ],
  },
  {
    path: '*', // Catch-all for 404
    element: <NotFound />,
  },
]);
