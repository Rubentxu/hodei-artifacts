import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, useLocation, useRoutes } from 'react-router-dom';
import { routeConfig } from '@/router';

vi.mock('@/pages/Dashboard', () => ({
  Dashboard: () => <div data-testid="overview-page">Overview Page</div>,
}));

vi.mock('@/pages/console/ConsoleRepositoriesPage', () => ({
  default: () => (
    <div data-testid="catalog-repositories-page">Catalog Repositories</div>
  ),
}));

vi.mock('@/pages/console/ConsoleBrowsePage', () => ({
  default: () => <div data-testid="catalog-browse-page">Catalog Browse</div>,
}));

vi.mock('@/pages/Repositories', () => ({
  Repositories: () => (
    <div data-testid="registry-repositories-page">
      Repository Registry (/api/v1/repositories)
    </div>
  ),
}));

const LocationDisplay = () => {
  const location = useLocation();
  return <div data-testid="current-pathname">{location.pathname}</div>;
};

const TestRoutes = () => {
  const element = useRoutes(routeConfig);
  return (
    <>
      {element}
      <LocationDisplay />
    </>
  );
};

function renderRoute(initialEntry: string) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[initialEntry]}>
        <TestRoutes />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

describe('Canonical product routing & legacy aliases', () => {
  it('preserves legacy aliases via redirects', async () => {
    const dashboardView = renderRoute('/dashboard');
    expect(await screen.findByTestId('overview-page')).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent('/');
    dashboardView.unmount();

    const browseView = renderRoute('/browse');
    expect(
      await screen.findByTestId('catalog-browse-page')
    ).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent(
      '/console/browse'
    );
    browseView.unmount();

    const usersView = renderRoute('/users');
    expect(
      await screen.findByRole('heading', { name: 'User Management' })
    ).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent(
      '/admin/users'
    );
    usersView.unmount();

    const tokensView = renderRoute('/tokens');
    expect(
      await screen.findByRole('heading', { name: 'API Tokens', level: 1 })
    ).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent(
      '/settings/tokens'
    );
    tokensView.unmount();
  });

  it('redirects canonical aliases and distinguishes policy settings from legacy paths', async () => {
    const overviewView = renderRoute('/overview');
    expect(await screen.findByTestId('overview-page')).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent('/');
    overviewView.unmount();

    const catalogView = renderRoute('/catalog/repositories');
    expect(
      await screen.findByTestId('catalog-repositories-page')
    ).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent(
      '/console/repositories'
    );
    catalogView.unmount();

    const registryView = renderRoute('/admin/registry');
    expect(
      await screen.findByTestId('registry-repositories-page')
    ).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent(
      '/repositories'
    );
    registryView.unmount();

    const policiesView = renderRoute('/policies');
    expect(
      await screen.findByRole('heading', {
        name: 'Access Policies',
        level: 1,
      })
    ).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent(
      '/settings/policies'
    );
    policiesView.unmount();
  });

  it('does NOT redirect Registry (/repositories) to Catalog (/console/repositories)', async () => {
    const registryView = renderRoute('/repositories');
    expect(
      await screen.findByTestId('registry-repositories-page')
    ).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent(
      '/repositories'
    );
    registryView.unmount();

    renderRoute('/console/repositories');
    expect(
      await screen.findByTestId('catalog-repositories-page')
    ).toBeInTheDocument();
    expect(screen.getByTestId('current-pathname')).toHaveTextContent(
      '/console/repositories'
    );
  });

  it('presents Registry Admin as distinct from Catalog in Administration hub', async () => {
    renderRoute('/admin');

    expect(
      await screen.findByRole('heading', { name: 'Administration' })
    ).toBeInTheDocument();
    expect(screen.getByText('Repository Registry')).toBeInTheDocument();
    expect(screen.getByText('Registry Admin')).toBeInTheDocument();
    expect(
      screen.getByText(/Distinct from Catalog Repositories/i)
    ).toBeInTheDocument();
  });

  it('renders skip-to-content link targeting main content in MainLayout', async () => {
    renderRoute('/');

    const skipLink = await screen.findByRole('link', {
      name: 'Skip to main content',
    });
    expect(skipLink).toHaveAttribute('href', '#main-content');
    const main = screen.getByRole('main');
    expect(main).toHaveAttribute('id', 'main-content');
    expect(main).toHaveAttribute('tabindex', '-1');
  });
});
