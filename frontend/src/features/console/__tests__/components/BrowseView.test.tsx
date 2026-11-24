/**
 * The browser of the assets.
 *
 * What is measured here is what the console says when the API could not
 * count the matches and when the API could not answer, how URL search
 * state and browser history drive the query, and that only parameters
 * accepted by `GET /api/v1/search` are sent.
 */

import { http, HttpResponse, delay } from 'msw';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, useLocation, useNavigate } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import SearchPage from '@/pages/search/SearchPage';
import { server } from '@/shared/test/mocks/server';
import { render } from '@/shared/test/test-utils';

import { BrowseView } from '../../components/BrowseView';

/** A result of the search. */
const theHit = {
  repository: 'libs',
  format: 'pypi',
  asset_path: 'requests/2.32.3/requests-2.32.3.tar.gz',
  package_name: 'requests',
  package_version: '2.32.3',
  digest: { algorithm: 'sha256', value: 'a'.repeat(64) },
  size_bytes: 2048,
  trust_state: 'trusted',
  updated_at: '2026-11-17T00:00:00Z',
};

describe('BrowseView', () => {
  it('does not say that there are no results while the search is on its way', async () => {
    server.use(
      http.get('/api/v1/search', async () => {
        await delay('infinite');
        return HttpResponse.json({});
      })
    );

    render(
      <MemoryRouter>
        <BrowseView />
      </MemoryRouter>
    );

    expect(await screen.findByText('Searching…')).toBeInTheDocument();
    expect(screen.queryByText(/0 results/)).not.toBeInTheDocument();
  });

  it('says that there are more when the total was not counted', async () => {
    server.use(
      http.get('/api/v1/search', () =>
        HttpResponse.json({
          total: null,
          more: true,
          limit: 20,
          offset: 0,
          results: [theHit],
        })
      )
    );

    render(
      <MemoryRouter>
        <BrowseView />
      </MemoryRouter>
    );

    const summary = await screen.findByText(/More than 1 results/);
    expect(summary).toBeInTheDocument();
    expect(summary).not.toHaveTextContent('0 results');
  });

  it('never renders 0 or an invented total when total is null and more is true with empty page', async () => {
    server.use(
      http.get('/api/v1/search', () =>
        HttpResponse.json({
          total: null,
          more: true,
          limit: 20,
          offset: 0,
          results: [],
        })
      )
    );

    render(
      <MemoryRouter>
        <BrowseView />
      </MemoryRouter>
    );

    const summary = await screen.findByText('More results available.');
    expect(summary).toBeInTheDocument();
    expect(summary).not.toHaveTextContent(/\b0\b/);
  });

  it('names the asset that was found', async () => {
    server.use(
      http.get('/api/v1/search', () =>
        HttpResponse.json({
          total: 1,
          more: false,
          limit: 20,
          offset: 0,
          results: [theHit],
        })
      )
    );

    render(
      <MemoryRouter>
        <BrowseView />
      </MemoryRouter>
    );

    expect(await screen.findByText('requests')).toBeInTheDocument();
    expect(screen.getByText('2.32.3')).toBeInTheDocument();
  });

  it('says that what is stored cannot be read', async () => {
    server.use(
      http.get('/api/v1/search', () =>
        HttpResponse.json(
          { error: 'corruption', message: 'what is stored cannot be read' },
          { status: 500 }
        )
      )
    );

    render(
      <MemoryRouter>
        <BrowseView />
      </MemoryRouter>
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'What is stored cannot be read'
    );
  });

  it('says that the decision could not be taken', async () => {
    server.use(
      http.get('/api/v1/search', () =>
        HttpResponse.json(
          { error: 'indeterminate', message: 'internal error' },
          { status: 500 }
        )
      )
    );

    render(
      <MemoryRouter>
        <BrowseView />
      </MemoryRouter>
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The decision could not be taken'
    );
  });

  it('says that the permission was refused', async () => {
    server.use(
      http.get('/api/v1/search', () =>
        HttpResponse.json(
          { error: 'permission', message: 'forbidden' },
          { status: 403 }
        )
      )
    );

    render(
      <MemoryRouter>
        <BrowseView />
      </MemoryRouter>
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'You are not allowed to do this'
    );
  });

  it('powers global search page via BrowseView and maps q and format to backend contract', async () => {
    const capturedParams: Array<Record<string, string>> = [];
    server.use(
      http.get('/api/v1/search', ({ request }) => {
        const url = new URL(request.url);
        capturedParams.push(Object.fromEntries(url.searchParams.entries()));
        return HttpResponse.json({
          total: 1,
          more: false,
          limit: 20,
          offset: 0,
          results: [theHit],
        });
      })
    );

    render(
      <MemoryRouter initialEntries={['/search?q=requests&format=pypi']}>
        <SearchPage />
      </MemoryRouter>
    );

    expect(await screen.findByText('requests')).toBeInTheDocument();
    expect(screen.getByLabelText('Text')).toHaveValue('requests');
    expect(screen.getByLabelText('Format')).toHaveValue('pypi');

    expect(capturedParams[0]).toEqual({
      attribute: 'format=pypi',
      text: 'requests',
      limit: '20',
      offset: '0',
    });
    expect(capturedParams[0]).not.toHaveProperty('q');
    expect(capturedParams[0]).not.toHaveProperty('format');
    expect(capturedParams[0]).not.toHaveProperty('cursor');
  });

  it('scopes per-repository exploration when repository prop is provided', async () => {
    const capturedParams: Array<Record<string, string>> = [];
    server.use(
      http.get('/api/v1/search', ({ request }) => {
        const url = new URL(request.url);
        capturedParams.push(Object.fromEntries(url.searchParams.entries()));
        return HttpResponse.json({
          total: 1,
          more: false,
          limit: 20,
          offset: 0,
          results: [theHit],
        });
      })
    );

    render(
      <MemoryRouter>
        <BrowseView repository="libs" />
      </MemoryRouter>
    );

    expect(await screen.findByText('requests')).toBeInTheDocument();
    const repoInput = screen.getByLabelText('Repository');
    expect(repoInput).toHaveValue('libs');
    expect(repoInput).toBeDisabled();
    expect(capturedParams[0]).toMatchObject({
      repository: 'libs',
      limit: '20',
      offset: '0',
    });
  });

  it('synchronizes filters and form inputs on browser back and forward navigation', async () => {
    const user = userEvent.setup();
    const capturedParams: Array<Record<string, string>> = [];
    server.use(
      http.get('/api/v1/search', ({ request }) => {
        const url = new URL(request.url);
        capturedParams.push(Object.fromEntries(url.searchParams.entries()));
        return HttpResponse.json({
          total: 1,
          more: false,
          limit: 20,
          offset: 0,
          results: [theHit],
        });
      })
    );

    const HistoryHarness = () => {
      const navigate = useNavigate();
      const location = useLocation();
      return (
        <div>
          <span data-testid="location-search">{location.search}</span>
          <button type="button" onClick={() => navigate(-1)}>
            Go Back
          </button>
          <button type="button" onClick={() => navigate(1)}>
            Go Forward
          </button>
          <BrowseView />
        </div>
      );
    };

    render(
      <MemoryRouter initialEntries={['/console/browse?package=requests']}>
        <HistoryHarness />
      </MemoryRouter>
    );

    expect(await screen.findByText('requests')).toBeInTheDocument();
    expect(screen.getByLabelText('Package')).toHaveValue('requests');

    const pkgInput = screen.getByLabelText('Package');
    await user.clear(pkgInput);
    await user.type(pkgInput, 'flask');
    await user.selectOptions(screen.getByLabelText('Format'), 'npm');
    await user.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => {
      const search = screen.getByTestId('location-search').textContent ?? '';
      expect(search).toContain('package=flask');
      expect(search).toContain('format=npm');
    });

    await user.click(screen.getByRole('button', { name: 'Go Back' }));

    await waitFor(() => {
      expect(screen.getByTestId('location-search')).toHaveTextContent(
        '?package=requests'
      );
      expect(screen.getByLabelText('Package')).toHaveValue('requests');
      expect(screen.getByLabelText('Format')).toHaveValue('');
    });

    await user.click(screen.getByRole('button', { name: 'Go Forward' }));

    await waitFor(() => {
      const search = screen.getByTestId('location-search').textContent ?? '';
      expect(search).toContain('package=flask');
      expect(screen.getByLabelText('Package')).toHaveValue('flask');
      expect(screen.getByLabelText('Format')).toHaveValue('npm');
    });

    expect(capturedParams.some(p => 'cursor' in p)).toBe(false);
  });

  it('paginates with offset and limit without sending cursor parameters', async () => {
    const user = userEvent.setup();
    const capturedParams: Array<Record<string, string>> = [];
    server.use(
      http.get('/api/v1/search', ({ request }) => {
        const url = new URL(request.url);
        const params = Object.fromEntries(url.searchParams.entries());
        capturedParams.push(params);
        const offset = Number(params.offset ?? '0');
        return HttpResponse.json({
          total: null,
          more: offset === 0,
          limit: 20,
          offset,
          results: [theHit],
        });
      })
    );

    render(
      <MemoryRouter initialEntries={['/console/browse']}>
        <BrowseView />
      </MemoryRouter>
    );

    expect(await screen.findByText('From 0')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled();
    const nextButton = screen.getByRole('button', { name: 'Next' });
    expect(nextButton).toBeEnabled();

    await user.click(nextButton);

    expect(await screen.findByText('From 20')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Previous' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();

    expect(capturedParams).toEqual([
      { limit: '20', offset: '0' },
      { limit: '20', offset: '20' },
    ]);
    for (const params of capturedParams) {
      expect(params).not.toHaveProperty('cursor');
    }
  });
});
