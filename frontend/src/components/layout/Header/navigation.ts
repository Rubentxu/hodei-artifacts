export type CanonicalNavId =
  | 'overview'
  | 'repositories'
  | 'artifacts'
  | 'administration';

export interface CanonicalNavItem {
  id: CanonicalNavId;
  label: string;
  to: string;
}

/**
 * Canonical 4-area product navigation.
 *
 * `Repositories` and `Artifacts` point to the catalog console as the primary
 * experience, while registry administration lives under `Administration`.
 */
export const CANONICAL_NAV_ITEMS: readonly CanonicalNavItem[] = [
  { id: 'overview', label: 'Overview', to: '/' },
  { id: 'repositories', label: 'Repositories', to: '/console/repositories' },
  { id: 'artifacts', label: 'Artifacts', to: '/console/browse' },
  { id: 'administration', label: 'Administration', to: '/admin' },
] as const;

/**
 * Determines which canonical navigation area corresponds to the current path.
 */
export function isNavItemActive(
  pathname: string,
  itemId: CanonicalNavId
): boolean {
  switch (itemId) {
    case 'overview':
      return (
        pathname === '/' ||
        pathname === '/dashboard' ||
        pathname === '/overview'
      );
    case 'repositories':
      return pathname.startsWith('/console/repositories');
    case 'artifacts':
      return (
        pathname.startsWith('/console/browse') ||
        pathname.startsWith('/console/packages') ||
        pathname.startsWith('/artifacts') ||
        pathname.startsWith('/search')
      );
    case 'administration':
      return (
        pathname.startsWith('/admin') ||
        pathname.startsWith('/settings') ||
        pathname.startsWith('/policies') ||
        pathname.startsWith('/repositories') ||
        pathname.startsWith('/openapi-demo')
      );
  }
}
