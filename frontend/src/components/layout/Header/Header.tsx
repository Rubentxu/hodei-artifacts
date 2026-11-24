import { useEffect, useRef, useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import { Menu, Moon, Sun, X } from 'lucide-react';
import { useAuth } from '@/shared/stores/auth.store';
import { useTheme } from '@/shared/stores/ui.store';
import { THEMES } from '@/shared/constants';
import { Button } from '@/components/ui/Button';
import { CANONICAL_NAV_ITEMS, isNavItemActive } from './navigation';

const FOCUS_RING_CLASS =
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--hodei-focus-ring)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--hodei-surface)]';

const Header = () => {
  const { user, logout } = useAuth();
  const { theme, setTheme } = useTheme();
  const location = useLocation();
  const navigate = useNavigate();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const menuButtonRef = useRef<HTMLButtonElement>(null);

  const isDark =
    theme === THEMES.DARK ||
    (theme === THEMES.SYSTEM &&
      typeof window !== 'undefined' &&
      typeof window.matchMedia === 'function' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches);

  const handleToggleTheme = () => {
    setTheme(isDark ? THEMES.LIGHT : THEMES.DARK);
  };

  const handleLogout = () => {
    logout();
    setMobileMenuOpen(false);
    navigate('/login');
  };

  // Close the mobile navigation drawer when the route changes.
  useEffect(() => {
    setMobileMenuOpen(false);
  }, [location.pathname]);

  // Allow keyboard users to dismiss the mobile menu with Escape.
  useEffect(() => {
    if (!mobileMenuOpen) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setMobileMenuOpen(false);
        menuButtonRef.current?.focus();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [mobileMenuOpen]);

  const navLinkClass = (active: boolean) =>
    [
      'rounded-md px-3 py-2 text-sm font-medium transition-colors',
      FOCUS_RING_CLASS,
      active
        ? 'bg-[var(--hodei-teal)] text-white shadow-xs'
        : 'text-[var(--hodei-text-muted)] hover:bg-[var(--hodei-surface-muted)] hover:text-[var(--hodei-text)]',
    ].join(' ');

  return (
    <header className="border-b border-[var(--hodei-border)] bg-[var(--hodei-surface)] text-[var(--hodei-text)] shadow-xs">
      <div className="container mx-auto px-4 py-3 sm:px-6">
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-6">
            <Link
              to="/"
              className={`flex items-center gap-2 rounded-md px-1.5 py-1 text-lg font-bold tracking-tight text-[var(--hodei-text)] ${FOCUS_RING_CLASS}`}
            >
              <span
                className="inline-flex h-7 w-7 items-center justify-center rounded-md bg-[var(--hodei-slate)] text-xs font-semibold text-white"
                aria-hidden="true"
              >
                HA
              </span>
              <span>Hodei Artifacts</span>
            </Link>

            <nav
              aria-label="Primary navigation"
              className="hidden md:flex items-center gap-1"
            >
              {CANONICAL_NAV_ITEMS.map(item => {
                const active = isNavItemActive(location.pathname, item.id);
                return (
                  <Link
                    key={item.id}
                    to={item.to}
                    aria-current={active ? 'page' : undefined}
                    className={navLinkClass(active)}
                  >
                    {item.label}
                  </Link>
                );
              })}
            </nav>
          </div>

          <div className="hidden md:flex items-center gap-3">
            <button
              type="button"
              onClick={handleToggleTheme}
              aria-label={
                isDark ? 'Switch to light theme' : 'Switch to dark theme'
              }
              className={`inline-flex h-9 w-9 items-center justify-center rounded-md border border-[var(--hodei-border)] bg-[var(--hodei-surface)] text-[var(--hodei-text-muted)] hover:bg-[var(--hodei-surface-muted)] hover:text-[var(--hodei-text)] ${FOCUS_RING_CLASS}`}
            >
              {isDark ? (
                <Sun className="h-4 w-4" aria-hidden="true" />
              ) : (
                <Moon className="h-4 w-4" aria-hidden="true" />
              )}
            </button>
            <span className="text-sm text-[var(--hodei-text-muted)]">
              Welcome, {user?.name ?? 'User'}
            </span>
            <Button
              variant="outline"
              size="sm"
              onClick={handleLogout}
              className={FOCUS_RING_CLASS}
            >
              Logout
            </Button>
          </div>

          <div className="flex items-center gap-2 md:hidden">
            <button
              type="button"
              onClick={handleToggleTheme}
              aria-label={
                isDark ? 'Switch to light theme' : 'Switch to dark theme'
              }
              className={`inline-flex h-9 w-9 items-center justify-center rounded-md border border-[var(--hodei-border)] bg-[var(--hodei-surface)] text-[var(--hodei-text-muted)] hover:bg-[var(--hodei-surface-muted)] hover:text-[var(--hodei-text)] ${FOCUS_RING_CLASS}`}
            >
              {isDark ? (
                <Sun className="h-4 w-4" aria-hidden="true" />
              ) : (
                <Moon className="h-4 w-4" aria-hidden="true" />
              )}
            </button>
            <button
              ref={menuButtonRef}
              type="button"
              onClick={() => setMobileMenuOpen(prev => !prev)}
              aria-expanded={mobileMenuOpen}
              aria-controls="mobile-primary-nav"
              aria-label={
                mobileMenuOpen
                  ? 'Close navigation menu'
                  : 'Open navigation menu'
              }
              className={`inline-flex h-9 w-9 items-center justify-center rounded-md border border-[var(--hodei-border)] bg-[var(--hodei-surface)] text-[var(--hodei-text)] hover:bg-[var(--hodei-surface-muted)] ${FOCUS_RING_CLASS}`}
            >
              {mobileMenuOpen ? (
                <X className="h-5 w-5" aria-hidden="true" />
              ) : (
                <Menu className="h-5 w-5" aria-hidden="true" />
              )}
            </button>
          </div>
        </div>

        {mobileMenuOpen && (
          <div
            id="mobile-primary-nav"
            className="mt-3 border-t border-[var(--hodei-border)] pt-3 md:hidden"
          >
            <nav
              aria-label="Mobile primary navigation"
              className="flex flex-col gap-1"
            >
              {CANONICAL_NAV_ITEMS.map(item => {
                const active = isNavItemActive(location.pathname, item.id);
                return (
                  <Link
                    key={item.id}
                    to={item.to}
                    aria-current={active ? 'page' : undefined}
                    onClick={() => setMobileMenuOpen(false)}
                    className={navLinkClass(active)}
                  >
                    {item.label}
                  </Link>
                );
              })}
            </nav>

            <div className="mt-3 flex items-center justify-between border-t border-[var(--hodei-border)] pt-3">
              <span className="text-sm text-[var(--hodei-text-muted)]">
                Welcome, {user?.name ?? 'User'}
              </span>
              <Button
                variant="outline"
                size="sm"
                onClick={handleLogout}
                className={FOCUS_RING_CLASS}
              >
                Logout
              </Button>
            </div>
          </div>
        )}
      </div>
    </header>
  );
};

export default Header;
