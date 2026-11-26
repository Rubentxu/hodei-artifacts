/**
 * Account usage and quota view tests.
 *
 * Verifies AC1 (authoritative server response), AC2 (null/unconfigured limits
 * vs 0), AC3 (403/5xx/unavailable never renders as 0 usage), AC4 (query key
 * scoped by principal/session), and Overview reuse of the same query hook.
 */

import { http, HttpResponse, delay } from 'msw';
import { act, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it } from 'vitest';

import Dashboard from '@/pages/Dashboard/Dashboard';
import { useAuthStore } from '@/shared/stores/auth.store';
import { server } from '@/shared/test/mocks/server';
import { render } from '@/shared/test/test-utils';

import { AccountUsageSummary } from '../../components/AccountUsageSummary';
import { AccountUsageView } from '../../components/AccountUsageView';

const baseUsage = {
  account: 'acct-alpha',
  stored_bytes: 2048,
  in_flight_bytes: 512,
  active_uploads: 2,
  limits: {
    max_stored_bytes_per_account: 1048576,
    max_concurrent_uploads_per_account: 5,
    retry_after_secs: 30,
  },
  note: 'the ledger is in memory and rate-shaping: a restart forgets it, and it is not a billing book',
};

describe('AccountUsageView', () => {
  afterEach(() => {
    useAuthStore.getState().logout();
  });

  it('does not render 0 usage while loading', async () => {
    server.use(
      http.get('/api/v1/account/usage', async () => {
        await delay('infinite');
        return HttpResponse.json({});
      })
    );

    render(
      <MemoryRouter>
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(
      await screen.findByText('Loading account usage…')
    ).toBeInTheDocument();
    expect(screen.queryByTestId('stored-bytes-value')).not.toBeInTheDocument();
    expect(screen.queryByText('0 B')).not.toBeInTheDocument();
  });

  it('displays authoritative usage and configured limits from the server', async () => {
    server.use(
      http.get('/api/v1/account/usage', () => HttpResponse.json(baseUsage))
    );

    render(
      <MemoryRouter>
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(await screen.findByText('acct-alpha')).toBeInTheDocument();
    expect(screen.getByTestId('stored-bytes-value')).toHaveTextContent('2 KB');
    expect(screen.getByTestId('in-flight-bytes-value')).toHaveTextContent(
      '512 B'
    );
    expect(screen.getByTestId('storage-limit-value')).toHaveTextContent('1 MB');
    expect(screen.getByTestId('active-uploads-value')).toHaveTextContent('2');
    expect(screen.getByTestId('upload-limit-value')).toHaveTextContent('5');
    expect(screen.getByText('30s')).toBeInTheDocument();
    expect(screen.getByText(baseUsage.note)).toBeInTheDocument();
  });

  it('distinguishes null/unconfigured limits from zero limits', async () => {
    server.use(
      http.get('/api/v1/account/usage', () =>
        HttpResponse.json({
          ...baseUsage,
          stored_bytes: 0,
          in_flight_bytes: 0,
          active_uploads: 0,
          limits: {
            max_stored_bytes_per_account: null,
            max_concurrent_uploads_per_account: null,
            retry_after_secs: 15,
          },
        })
      )
    );

    render(
      <MemoryRouter>
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(await screen.findByTestId('storage-limit-value')).toHaveTextContent(
      'Unlimited / Not configured'
    );
    expect(screen.getByTestId('upload-limit-value')).toHaveTextContent(
      'Unlimited / Not configured'
    );
    expect(screen.getByTestId('stored-bytes-value')).toHaveTextContent('0 B');
  });

  it('renders zero limits explicitly as 0 rather than unlimited', async () => {
    server.use(
      http.get('/api/v1/account/usage', () =>
        HttpResponse.json({
          ...baseUsage,
          stored_bytes: 0,
          in_flight_bytes: 0,
          active_uploads: 0,
          limits: {
            max_stored_bytes_per_account: 0,
            max_concurrent_uploads_per_account: 0,
            retry_after_secs: 60,
          },
        })
      )
    );

    render(
      <MemoryRouter>
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(await screen.findByTestId('storage-limit-value')).toHaveTextContent(
      '0 B'
    );
    expect(screen.getByTestId('upload-limit-value')).toHaveTextContent('0');
    expect(
      screen.queryByText('Unlimited / Not configured')
    ).not.toBeInTheDocument();
  });

  it('shows permission error on 403 and never renders 0 usage', async () => {
    server.use(
      http.get('/api/v1/account/usage', () =>
        HttpResponse.json(
          { error: 'permission', message: 'forbidden' },
          { status: 403 }
        )
      )
    );

    render(
      <MemoryRouter>
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'You are not allowed to do this'
    );
    expect(screen.queryByTestId('stored-bytes-value')).not.toBeInTheDocument();
    expect(screen.queryByText('0 B')).not.toBeInTheDocument();
  });

  it('shows unavailable error on 503 and never renders 0 usage', async () => {
    server.use(
      http.get('/api/v1/account/usage', () =>
        HttpResponse.json(
          { error: 'unavailable', message: 'the usage is not available' },
          { status: 503 }
        )
      )
    );

    render(
      <MemoryRouter>
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The service cannot answer right now'
    );
    expect(screen.queryByTestId('stored-bytes-value')).not.toBeInTheDocument();
    expect(screen.queryByText('0 B')).not.toBeInTheDocument();
  });

  it('shows network failure as unavailable and never renders 0 usage', async () => {
    server.use(http.get('/api/v1/account/usage', () => HttpResponse.error()));

    render(
      <MemoryRouter>
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The service cannot answer right now'
    );
    expect(screen.queryByTestId('stored-bytes-value')).not.toBeInTheDocument();
    expect(screen.queryByText('0 B')).not.toBeInTheDocument();
  });

  it('shows error on 500 and never renders 0 usage', async () => {
    server.use(
      http.get('/api/v1/account/usage', () =>
        HttpResponse.json({ error: 'internal' }, { status: 500 })
      )
    );

    render(
      <MemoryRouter>
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The decision could not be taken'
    );
    expect(screen.queryByTestId('stored-bytes-value')).not.toBeInTheDocument();
    expect(screen.queryByText('0 B')).not.toBeInTheDocument();
  });

  it('scopes the query key by principal so switching accounts does not leak cached data', async () => {
    server.use(
      http.get('/api/v1/account/usage', ({ request }) => {
        const auth = request.headers.get('authorization');
        if (auth === 'Bearer token-beta') {
          return HttpResponse.json({
            ...baseUsage,
            account: 'acct-beta',
            stored_bytes: 4096,
          });
        }
        return HttpResponse.json(baseUsage);
      })
    );

    useAuthStore.getState().login(
      {
        id: 'principal-alpha',
        email: 'alpha@example.com',
        name: 'Alpha',
        role: 'user',
        createdAt: '2026-01-01T00:00:00Z',
      },
      'token-alpha'
    );

    render(
      <MemoryRouter>
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(await screen.findByText('acct-alpha')).toBeInTheDocument();
    expect(screen.getByTestId('stored-bytes-value')).toHaveTextContent('2 KB');

    act(() => {
      useAuthStore.getState().login(
        {
          id: 'principal-beta',
          email: 'beta@example.com',
          name: 'Beta',
          role: 'user',
          createdAt: '2026-01-01T00:00:00Z',
        },
        'token-beta'
      );
    });

    await waitFor(() => {
      expect(screen.getByText('acct-beta')).toBeInTheDocument();
    });
    expect(screen.getByTestId('stored-bytes-value')).toHaveTextContent('4 KB');
  });

  it('reuses the same principal-scoped query between the Dashboard summary and account usage details', async () => {
    let requestCount = 0;
    server.use(
      http.get('/api/v1/account/usage', () => {
        requestCount += 1;
        return HttpResponse.json(baseUsage);
      })
    );

    render(
      <MemoryRouter>
        <AccountUsageSummary />
        <AccountUsageView />
      </MemoryRouter>
    );

    expect(await screen.findAllByText('acct-alpha')).toHaveLength(2);
    expect(requestCount).toBe(1);
    expect(screen.getByRole('link', { name: 'View details' })).toHaveAttribute(
      'href',
      '/console/usage'
    );
  });

  it('renders AccountUsageSummary inside Dashboard Overview without falling back to 0 on error', async () => {
    server.use(
      http.get('/api/v1/account/usage', () =>
        HttpResponse.json(
          { error: 'unavailable', message: 'the usage is not available' },
          { status: 503 }
        )
      )
    );

    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>
    );

    expect(
      await screen.findByText('Account Usage & Quotas')
    ).toBeInTheDocument();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The service cannot answer right now'
    );
  });
});
