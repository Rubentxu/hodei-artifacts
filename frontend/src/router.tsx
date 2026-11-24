import {
  createBrowserRouter,
  Navigate,
  type RouteObject,
} from 'react-router-dom';

// Import from index files for consistency
import { Login } from '@/pages/Login';
import { Dashboard } from '@/pages/Dashboard';
import { Repositories } from '@/pages/Repositories';
import { RepositoryDetail } from '@/pages/RepositoryDetail';
import { NotFound } from '@/pages/NotFound';
import { SearchPage } from '@/pages/search';
import { ProfilePage } from '@/pages/profile';
import { TokensPage } from '@/pages/settings/tokens';
import { AdminPage, UsersPage } from '@/pages/admin';
import { PoliciesPage } from '@/pages/settings/policies';
import { OpenAPIDemoPage } from '@/pages/OpenAPIDemoPage';
import Artifacts from '@/pages/Artifacts/Artifacts';
import PoliciesSimple from '@/pages/Policies/PoliciesSimple';
import ConsoleRepositoriesPage from '@/pages/console/ConsoleRepositoriesPage';
import ConsoleBrowsePage from '@/pages/console/ConsoleBrowsePage';
import ConsolePackagePage from '@/pages/console/ConsolePackagePage';

import { MainLayout } from '@/components/templates/MainLayout';

export const routeConfig: RouteObject[] = [
  {
    path: '/login', // Login page
    element: <Login />,
  },
  {
    element: <MainLayout />, // Layout for authenticated routes
    children: [
      {
        path: '/', // Overview (primary landing)
        element: <Dashboard />,
      },
      {
        path: '/dashboard',
        element: <Navigate to="/" replace />,
      },
      {
        path: '/overview',
        element: <Navigate to="/" replace />,
      },
      {
        // Primary Repositories experience (Catalog Console: /api/v1/catalog/repositories)
        path: '/console/repositories',
        element: <ConsoleRepositoriesPage />,
      },
      {
        path: '/catalog/repositories',
        element: <Navigate to="/console/repositories" replace />,
      },
      {
        // Primary Artifacts experience (Catalog Console browser & asset detail)
        path: '/console/browse',
        element: <ConsoleBrowsePage />,
      },
      {
        path: '/browse',
        element: <Navigate to="/console/browse" replace />,
      },
      {
        // The path of the asset is a wildcard: the slashes are part of
        // the name of the content inside the repository.
        path: '/console/packages/:repository/*',
        element: <ConsolePackagePage />,
      },
      {
        // Legacy artifact upload/validation workspace preserved
        path: '/artifacts',
        element: <Artifacts />,
      },
      {
        path: '/search',
        element: <SearchPage />,
      },
      {
        // Administration hub
        path: '/admin',
        element: <AdminPage />,
      },
      {
        // Registry Admin (/api/v1/repositories) — semantically distinct from Catalog Repositories
        path: '/repositories',
        element: <Repositories />,
      },
      {
        path: '/repositories/:id',
        element: <RepositoryDetail />,
      },
      {
        path: '/admin/registry',
        element: <Navigate to="/repositories" replace />,
      },
      {
        path: '/admin/users',
        element: <UsersPage />,
      },
      {
        path: '/users',
        element: <Navigate to="/admin/users" replace />,
      },
      {
        path: '/settings/tokens',
        element: <TokensPage />,
      },
      {
        path: '/tokens',
        element: <Navigate to="/settings/tokens" replace />,
      },
      {
        path: '/settings/policies',
        element: <PoliciesPage />,
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
        path: '/openapi-demo',
        element: <OpenAPIDemoPage />,
      },
    ],
  },
  {
    path: '*', // Catch-all for 404
    element: <NotFound />,
  },
];

export const router = createBrowserRouter(routeConfig);
