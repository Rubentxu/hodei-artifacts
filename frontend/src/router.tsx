import { createBrowserRouter } from 'react-router-dom';

// Import from index files for consistency
import { Login } from '@/pages/Login';
import { Dashboard } from '@/pages/Dashboard';
import { Repositories } from '@/pages/Repositories';
import { RepositoryDetail } from '@/pages/RepositoryDetail';
import { NotFound } from '@/pages/NotFound';
import { SearchPage } from '@/pages/search';
import { ProfilePage } from '@/pages/profile';
import { TokensPage } from '@/pages/settings/tokens';
import { UsersPage } from '@/pages/admin/users';
import { PoliciesPage } from '@/pages/settings/policies';
import { OpenAPIDemoPage } from '@/pages/OpenAPIDemoPage';
import Artifacts from '@/pages/Artifacts/Artifacts';
import PoliciesSimple from '@/pages/Policies/PoliciesSimple';

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
        path: '/artifacts',
        element: <Artifacts />,
      },
      {
        path: '/policies',
        element: <PoliciesSimple />,
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
      {
        path: '/openapi-demo',
        element: <OpenAPIDemoPage />,
      },
    ],
  },
  {
    path: '*', // Catch-all for 404
    element: <NotFound />,
  },
]);
