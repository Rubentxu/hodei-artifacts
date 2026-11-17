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
import ConsoleRepositoriesPage from '@/pages/console/ConsoleRepositoriesPage';
import ConsoleBrowsePage from '@/pages/console/ConsoleBrowsePage';
import ConsolePackagePage from '@/pages/console/ConsolePackagePage';

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
      {
        // The console of the catalog: the repositories, the browser of
        // the assets and the detail of each one.
        path: '/console/repositories',
        element: <ConsoleRepositoriesPage />,
      },
      {
        path: '/console/browse',
        element: <ConsoleBrowsePage />,
      },
      {
        // The path of the asset is a wildcard: the slashes are part of
        // the name of the content inside the repository.
        path: '/console/packages/:repository/*',
        element: <ConsolePackagePage />,
      },
    ],
  },
  {
    path: '*', // Catch-all for 404
    element: <NotFound />,
  },
]);
