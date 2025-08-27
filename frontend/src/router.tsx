import { createBrowserRouter } from 'react-router-dom';
import {
  Home,
  Login,
  Dashboard,
  Repositories,
  RepositoryDetail,
  NotFound,
} from './pages';

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
    path: '*',
    element: <NotFound />,
  },
]);
