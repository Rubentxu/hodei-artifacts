import { act, cleanup, screen, waitFor } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAuthStore } from '@/shared/stores/auth.store';
import { server } from '@/shared/test/mocks/server';
import { render } from '@/shared/test/test-utils';
import type { User } from '@/shared/types';

import { RepositoriesView } from '../../components/RepositoriesView';
import { useCreateRepository, useRepositories } from '../../hooks/useConsole';

const principalA: User = {
  id: 'principal-a',
  email: 'alice@example.org',
  name: 'Alice',
  role: 'admin',
  createdAt: '2026-01-01T00:00:00Z',
};

const principalB: User = {
  id: 'principal-b',
  email: 'bob@example.org',
  name: 'Bob',
  role: 'user',
  createdAt: '2026-01-01T00:00:00Z',
};

const createDeferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(r => {
    resolve = r;
  });
  return { promise, resolve };
};

const makeRepo = (name: string) => ({
  name,
  format: 'pypi',
  mode: 'hosted',
  serve_channel: 'trusted',
  package_count: 1,
  asset_count: 2,
});

const capabilities = {
  formats: [{ name: 'pypi', modes: ['hosted', 'proxy', 'group'] }],
};

describe('Console session isolation', () => {
  beforeEach(() => {
    server.use(
      http.get('/api/v1/capabilities', () => HttpResponse.json(capabilities))
    );
  });

  afterEach(() => {
    cleanup();
    act(() => {
      useAuthStore.getState().logout();
    });
  });

  it('clears cached console queries when the principal logs out', async () => {
    server.use(
      http.get('/api/v1/catalog/repositories', ({ request }) => {
        if (request.headers.get('authorization') === 'Bearer token-a') {
          return HttpResponse.json({
            repositories: [makeRepo('repo-alpha')],
          });
        }
        return HttpResponse.json({ error: 'unauthenticated' }, { status: 401 });
      })
    );

    act(() => {
      useAuthStore.getState().login(principalA, 'token-a');
    });

    const { queryClient } = render(
      <MemoryRouter>
        <RepositoriesView />
      </MemoryRouter>
    );

    expect(await screen.findByText('repo-alpha')).toBeInTheDocument();

    act(() => {
      useAuthStore.getState().logout();
    });

    await waitFor(() => {
      expect(screen.queryByText('repo-alpha')).not.toBeInTheDocument();
    });

    const cachedAlpha = queryClient
      .getQueryCache()
      .findAll({ queryKey: ['console'] })
      .filter(query =>
        (JSON.stringify(query.state.data) ?? '').includes('repo-alpha')
      );
    expect(cachedAlpha).toHaveLength(0);
  });

  it('cancels in-flight console queries when the principal logs out', async () => {
    const delayedResponse = createDeferred<{
      repositories: ReturnType<typeof makeRepo>[];
    }>();
    let requestStarted = false;

    server.use(
      http.get('/api/v1/catalog/repositories', async ({ request }) => {
        if (request.headers.get('authorization') !== 'Bearer token-a') {
          return HttpResponse.json({ error: 'unauthenticated' }, { status: 401 });
        }
        requestStarted = true;
        const payload = await delayedResponse.promise;
        return HttpResponse.json(payload);
      })
    );

    act(() => {
      useAuthStore.getState().login(principalA, 'token-a');
    });

    const { queryClient } = render(
      <MemoryRouter>
        <RepositoriesView />
      </MemoryRouter>
    );
    const cancelQueries = vi.spyOn(queryClient, 'cancelQueries');

    await waitFor(() => {
      expect(requestStarted).toBe(true);
    });

    const previousSession = queryClient
      .getQueryCache()
      .findAll({ queryKey: ['console'] })[0]?.queryKey[1];
    expect(previousSession).toBeDefined();

    act(() => {
      useAuthStore.getState().logout();
    });

    await waitFor(() => {
      expect(cancelQueries).toHaveBeenCalledWith({
        queryKey: ['console', previousSession],
      });
    });

    delayedResponse.resolve({ repositories: [makeRepo('repo-alpha')] });
    await act(async () => {
      await Promise.resolve();
    });

    const cachedAlpha = queryClient
      .getQueryCache()
      .findAll({ queryKey: ['console'] })
      .filter(query =>
        (JSON.stringify(query.state.data) ?? '').includes('repo-alpha')
      );
    expect(cachedAlpha).toHaveLength(0);
  });

  it('prevents a deliberately delayed in-flight response from principal A from repopulating principal B', async () => {
    const delayedPrincipalAResponse = createDeferred<{
      repositories: ReturnType<typeof makeRepo>[];
    }>();
    const observedAuthHeaders: string[] = [];

    server.use(
      http.get('/api/v1/catalog/repositories', async ({ request }) => {
        const auth = request.headers.get('authorization') ?? '';
        observedAuthHeaders.push(auth);

        if (auth === 'Bearer token-a') {
          const payload = await delayedPrincipalAResponse.promise;
          return HttpResponse.json(payload);
        }

        if (auth === 'Bearer token-b') {
          return HttpResponse.json({
            repositories: [makeRepo('repo-beta')],
          });
        }

        return HttpResponse.json({ error: 'unauthenticated' }, { status: 401 });
      })
    );

    act(() => {
      useAuthStore.getState().login(principalA, 'token-a');
    });

    const { queryClient } = render(
      <MemoryRouter>
        <RepositoriesView />
      </MemoryRouter>
    );

    // Wait until Principal A's request is genuinely in-flight.
    await waitFor(() => {
      expect(observedAuthHeaders).toContain('Bearer token-a');
    });

    // Switch directly to Principal B while Principal A's request is still pending.
    act(() => {
      useAuthStore.getState().login(principalB, 'token-b');
    });

    expect(await screen.findByText('repo-beta')).toBeInTheDocument();

    // Now resolve Principal A's late response.
    await act(async () => {
      delayedPrincipalAResponse.resolve({
        repositories: [makeRepo('repo-alpha')],
      });
      await Promise.resolve();
    });

    // Principal B's UI and cache must never contain Principal A's data.
    expect(screen.getByText('repo-beta')).toBeInTheDocument();
    expect(screen.queryByText('repo-alpha')).not.toBeInTheDocument();

    const cachedAlpha = queryClient
      .getQueryCache()
      .findAll({ queryKey: ['console'] })
      .filter(query =>
        (JSON.stringify(query.state.data) ?? '').includes('repo-alpha')
      );
    expect(cachedAlpha).toHaveLength(0);
    expect(observedAuthHeaders).toEqual(['Bearer token-a', 'Bearer token-b']);
  });

  it('guards in-flight mutations against session switches and resets mutation state', async () => {
    const delayedMutation = createDeferred<ReturnType<typeof makeRepo>>();
    let latestMutationStatus = '';
    let latestMutationDataName: string | undefined;
    let triggerCreate!: () => void;

    server.use(
      http.get('/api/v1/catalog/repositories', ({ request }) => {
        if (request.headers.get('authorization') === 'Bearer token-b') {
          return HttpResponse.json({
            repositories: [makeRepo('repo-beta')],
          });
        }
        return HttpResponse.json({ repositories: [] });
      }),
      http.post('/api/v1/catalog/repositories', async () => {
        const created = await delayedMutation.promise;
        return HttpResponse.json(created, { status: 201 });
      })
    );

    const Probe = () => {
      const repositories = useRepositories();
      const createRepo = useCreateRepository();
      latestMutationStatus = createRepo.status;
      latestMutationDataName = createRepo.data?.name;
      triggerCreate = () => {
        createRepo.mutate({
          name: 'repo-alpha-created',
          format: 'pypi',
          mode: 'hosted',
        });
      };
      return (
        <div>
          {(repositories.data?.repositories ?? []).map(repo => (
            <span key={repo.name}>{repo.name}</span>
          ))}
        </div>
      );
    };

    act(() => {
      useAuthStore.getState().login(principalA, 'token-a');
    });

    render(<Probe />);

    act(() => {
      triggerCreate();
    });

    await waitFor(() => {
      expect(latestMutationStatus).toBe('pending');
    });

    // Switch to Principal B while Principal A's mutation is in flight.
    act(() => {
      useAuthStore.getState().login(principalB, 'token-b');
    });

    expect(await screen.findByText('repo-beta')).toBeInTheDocument();

    // Resolve Principal A's delayed mutation response.
    await act(async () => {
      delayedMutation.resolve(makeRepo('repo-alpha-created'));
      await Promise.resolve();
    });

    expect(latestMutationStatus).toBe('idle');
    expect(latestMutationDataName).toBeUndefined();
    expect(screen.queryByText('repo-alpha-created')).not.toBeInTheDocument();
  });

  it('distinguishes HTTP 401 and 403 in the UI without leaking resource existence or credentials', async () => {
    server.use(
      http.get('/api/v1/catalog/repositories', ({ request }) => {
        if (request.headers.get('authorization') === 'Bearer expired-token') {
          return HttpResponse.json(
            { message: 'expired-token rejected for /catalog/repositories' },
            { status: 401 }
          );
        }
        return HttpResponse.json(
          { message: 'principal-b lacks access to private-repo' },
          { status: 403 }
        );
      })
    );

    act(() => {
      useAuthStore.getState().login(principalA, 'expired-token');
    });

    render(
      <MemoryRouter>
        <RepositoriesView />
      </MemoryRouter>
    );

    const alert401 = await screen.findByRole('alert');
    expect(alert401).toHaveTextContent('Your credential is not valid');
    expect(alert401).not.toHaveTextContent('expired-token');
    expect(alert401).not.toHaveTextContent('private-repo');

    act(() => {
      useAuthStore.getState().login(principalB, 'token-b');
    });

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent(
        'You are not allowed to do this'
      );
    });
    const alert403 = screen.getByRole('alert');
    expect(alert403).not.toHaveTextContent('principal-b');
    expect(alert403).not.toHaveTextContent('private-repo');
    expect(alert403).not.toHaveTextContent('Your credential is not valid');
  });
});
