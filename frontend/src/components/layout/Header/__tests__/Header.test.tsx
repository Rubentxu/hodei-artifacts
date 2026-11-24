import { beforeEach, describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { Header } from '@/components/layout/Header';
import { useUIStore } from '@/shared/stores/ui.store';
import { THEMES } from '@/shared/constants';

function renderHeader(initialEntry = '/') {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <Header />
    </MemoryRouter>
  );
}

describe('Header canonical navigation', () => {
  beforeEach(() => {
    document.documentElement.classList.remove('dark');
    useUIStore.getState().setTheme(THEMES.LIGHT);
  });

  it('renders a single visible entry for each of the 4 canonical areas', () => {
    renderHeader('/');

    const primaryNav = screen.getByRole('navigation', {
      name: 'Primary navigation',
    });
    const links = within(primaryNav).getAllByRole('link');

    expect(links).toHaveLength(4);
    expect(links.map(link => link.textContent)).toEqual([
      'Overview',
      'Repositories',
      'Artifacts',
      'Administration',
    ]);
    expect(links.map(link => link.getAttribute('href'))).toEqual([
      '/',
      '/console/repositories',
      '/console/browse',
      '/admin',
    ]);
  });

  it('marks Catalog Repositories as Repositories and Registry Admin as Administration', () => {
    const { unmount } = renderHeader('/console/repositories');
    const catalogNav = screen.getByRole('navigation', {
      name: 'Primary navigation',
    });
    expect(
      within(catalogNav).getByRole('link', { name: 'Repositories' })
    ).toHaveAttribute('aria-current', 'page');
    expect(
      within(catalogNav).getByRole('link', { name: 'Administration' })
    ).not.toHaveAttribute('aria-current');
    unmount();

    renderHeader('/repositories');
    const registryNav = screen.getByRole('navigation', {
      name: 'Primary navigation',
    });
    expect(
      within(registryNav).getByRole('link', { name: 'Administration' })
    ).toHaveAttribute('aria-current', 'page');
    expect(
      within(registryNav).getByRole('link', { name: 'Repositories' })
    ).not.toHaveAttribute('aria-current');
  });

  it('highlights Artifacts for package detail routes', () => {
    renderHeader('/console/packages/npm-local/scope/pkg');
    const primaryNav = screen.getByRole('navigation', {
      name: 'Primary navigation',
    });
    expect(
      within(primaryNav).getByRole('link', { name: 'Artifacts' })
    ).toHaveAttribute('aria-current', 'page');
  });

  it('provides visible focus styles and keyboard-accessible mobile drawer', async () => {
    const user = userEvent.setup();
    renderHeader('/');

    const primaryNav = screen.getByRole('navigation', {
      name: 'Primary navigation',
    });
    for (const link of within(primaryNav).getAllByRole('link')) {
      expect(link.className).toContain('focus-visible:ring-2');
    }

    const toggleButton = screen.getByRole('button', {
      name: 'Open navigation menu',
    });
    expect(toggleButton).toHaveAttribute('aria-expanded', 'false');

    await user.click(toggleButton);

    expect(toggleButton).toHaveAttribute('aria-expanded', 'true');
    const mobileNav = screen.getByRole('navigation', {
      name: 'Mobile primary navigation',
    });
    expect(
      within(mobileNav)
        .getAllByRole('link')
        .map(link => link.textContent)
    ).toEqual(['Overview', 'Repositories', 'Artifacts', 'Administration']);

    await user.keyboard('{Escape}');
    expect(
      screen.queryByRole('navigation', { name: 'Mobile primary navigation' })
    ).not.toBeInTheDocument();
    expect(toggleButton).toHaveFocus();
  });

  it('toggles light and dark theme classes on the document root', async () => {
    const user = userEvent.setup();
    renderHeader('/');

    const themeButtons = screen.getAllByRole('button', {
      name: 'Switch to dark theme',
    });
    await user.click(themeButtons[0]);

    expect(document.documentElement.classList.contains('dark')).toBe(true);
    expect(
      screen.getAllByRole('button', { name: 'Switch to light theme' }).length
    ).toBeGreaterThan(0);
  });
});
