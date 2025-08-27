import { createBrowserRouter } from 'react-router-dom'
import { Home } from './pages/Home'
import { Login } from './pages/Login'
import { Dashboard } from './pages/Dashboard'
import { NotFound } from './pages/NotFound'

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
    path: '*',
    element: <NotFound />,
  },
])