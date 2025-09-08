import React from 'react';
import { useAuth } from '@/shared/stores/auth.store';
import { Button } from '@/components/ui/Button';

const Header = () => {
  const { user, logout } = useAuth();

  const handleLogout = () => {
    logout();
    window.location.href = '/login';
  };

  return (
    <header className="bg-white shadow-sm border-b border-gray-200">
      <div className="container mx-auto px-4 py-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <h1 className="text-xl font-bold text-gray-900">Hodei Artifacts</h1>
            <nav className="hidden md:flex items-center gap-4">
              <a href="/" className="text-gray-600 hover:text-gray-900">Dashboard</a>
              <a href="/repositories" className="text-gray-600 hover:text-gray-900">Repositories</a>
              <a href="/artifacts" className="text-gray-600 hover:text-gray-900">Artifacts</a>
              <a href="/search" className="text-gray-600 hover:text-gray-900">Search</a>
              <a href="/policies" className="text-gray-600 hover:text-gray-900">Policies</a>
              <a href="/tokens" className="text-gray-600 hover:text-gray-900">Tokens</a>
              <a href="/users" className="text-gray-600 hover:text-gray-900">Users</a>
            </nav>
          </div>
          
          <div className="flex items-center gap-4">
            <span className="text-sm text-gray-600">
              Welcome, {user?.name || 'User'}
            </span>
            <Button
              variant="outline"
              size="sm"
              onClick={handleLogout}
            >
              Logout
            </Button>
          </div>
        </div>
      </div>
    </header>
  );
};

export default Header;
