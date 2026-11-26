import { Outlet } from 'react-router-dom';
import { Header } from '@/components/layout/Header';
import { NotificationProvider } from '@/components/providers/NotificationProvider';

const MainLayout = () => {
  return (
    <div className="min-h-screen bg-[var(--hodei-canvas)] text-[var(--hodei-text)] transition-colors">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-50 focus:rounded-md focus:bg-[var(--hodei-slate)] focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-white focus:outline-none focus:ring-2 focus:ring-[var(--hodei-focus-ring)]"
      >
        Skip to main content
      </a>
      <Header />
      <main
        id="main-content"
        tabIndex={-1}
        className="container mx-auto px-4 py-6 sm:px-6 focus:outline-none"
      >
        <Outlet />
      </main>
      <NotificationProvider />
    </div>
  );
};

export default MainLayout;
