import React from 'react';
import { NavLink } from 'react-router-dom';
import { SearchBox } from '../../forms/search-box';
// import { UserNav } from './UserNav'; // Placeholder for user menu

const Header = () => {
  return (
    <header className="sticky top-0 z-50 w-full border-b border-border/40 bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60">
      <div className="container flex h-14 max-w-screen-2xl items-center">
        <div className="mr-4 hidden md:flex">
          <NavLink to="/" className="mr-6 flex items-center space-x-2">
            {/* <Logo /> */}
            <span className="hidden font-bold sm:inline-block">
              Hodei Artifacts
            </span>
          </NavLink>
          <nav className="flex items-center gap-6 text-sm">
            <NavLink
              to="/dashboard"
              className={({ isActive }) =>
                isActive
                  ? 'font-semibold text-primary'
                  : 'text-muted-foreground'
              }
            >
              Dashboard
            </NavLink>
            <NavLink
              to="/repositories"
              className={({ isActive }) =>
                isActive
                  ? 'font-semibold text-primary'
                  : 'text-muted-foreground'
              }
            >
              Repositories
            </NavLink>
          </nav>
        </div>
        <div className="flex flex-1 items-center justify-between space-x-2 md:justify-end">
          <div className="w-full flex-1 md:w-auto md:flex-none">
            <SearchBox />
          </div>
          <nav className="flex items-center">
            {/* <UserNav /> will go here */}
            <p className="text-sm text-muted-foreground">User Menu</p>
          </nav>
        </div>
      </div>
    </header>
  );
};

export { Header };
