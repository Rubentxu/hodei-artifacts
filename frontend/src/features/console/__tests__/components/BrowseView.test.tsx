/**
 * The browser of the assets.
 *
 * What is measured here is what the console says when the API could not
 * count the matches and when the API could not answer. A browser that
 * says "nothing here" when what happened is that the catalog could not be
 * read sends somebody looking for a package that is there.
 */

import { http, HttpResponse, delay } from 'msw';
import { screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

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
});
