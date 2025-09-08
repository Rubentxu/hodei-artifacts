import React from 'react';
import { Outlet } from 'react-router-dom';
import { Header } from '@/components/layout/Header';
import { NotificationProvider } from '@/components/providers/NotificationProvider';

const MainLayout = () => {
  return (
    <div className="min-h-screen bg-gray-50">
      <Header />
      <main className="container mx-auto p-6">
        <Outlet />
      </main>
      <NotificationProvider />
    </div>
  );
};

export default MainLayout;
