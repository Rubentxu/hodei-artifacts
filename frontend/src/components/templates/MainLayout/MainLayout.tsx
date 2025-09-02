import React from 'react';
import { Outlet } from 'react-router-dom';
import { Header } from '@/components/layout/Header';
// import { Sidebar } from '../../layout/sidebar'; // Placeholder

const MainLayout = () => {
  return (
    <div className="min-h-screen bg-background">
      {/* <Sidebar /> */}
      <Header />
      <main className="container mx-auto p-8">
        <Outlet />
      </main>
    </div>
  );
};

export default MainLayout;
