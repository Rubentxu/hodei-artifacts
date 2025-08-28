import { createBrowserRouter } from 'react-router-dom';
import {
  Home,
  Login,
  Dashboard,
  Repositories,
  RepositoryDetail,
  NotFound,
  SearchPage,
  ProfilePage,
  TokensPage,
  UsersPage,
  PoliciesPage,
} from './pages';
import { MainLayout } from './components/templates/main-layout';

export const router = createBrowserRouter([
  {
    path: '/',
    element: <Home />,
  },
  {
    path: '/login',
    element: <Login />,
  },
  {
    element: <MainLayout />,
    children: [
      {
        path: '/dashboard',
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
    path: '*',
    element: <NotFound />,
  },
]);
