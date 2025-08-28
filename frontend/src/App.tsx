import { RouterProvider } from 'react-router-dom';
import { router } from './router';
import { NotificationProvider } from './components/providers/NotificationProvider';

function App() {
  return (
    <>
      <RouterProvider router={router} />
      <NotificationProvider />
    </>
  );
}

export default App;
