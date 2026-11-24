/**
 * The detail of an asset.
 *
 * What is measured here is what the console says about the asset and
 * what it does with the actions that change what the repository serves.
 * The promotion and the revocation ask first: they change what everybody
 * who reads that repository gets, and the person who did it cannot undo
 * it from a distance.
 */

import { http, HttpResponse } from 'msw';
import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import ConsolePackagePage from '@/pages/console/ConsolePackagePage';
import { useAuthStore } from '@/shared/stores/auth.store';
import { server } from '@/shared/test/mocks/server';
import { render } from '@/shared/test/test-utils';

import { PackageDetailView } from '../../components/PackageDetailView';

/** The digest of the asset. */
const theDigest = 'a'.repeat(64);

/** The asset, as the API answers it. */
const thePackage = {
  repository: 'libs',
  asset_path: 'requests/2.32.3/requests-2.32.3.tar.gz',
  package_name: 'requests',
  package_version: '2.32.3',
  digest: { algorithm: 'sha256', value: theDigest },
  size_bytes: 2048,
  updated_at: '2026-11-17T00:00:00Z',
  blob: {
    digest: { algorithm: 'sha256', value: theDigest },
    scope: 'global',
    size_bytes: 2048,
    updated_at: '2026-11-17T00:00:00Z',
  },
  published: {
    package_name: 'requests',
    package_version: '2.32.3',
    digest: { algorithm: 'sha256', value: theDigest },
    size_bytes: 2048,
    published_at: '2026-11-17T00:00:00Z',
  },
  lineage: { subject: null, referrers: [] },
  capsule: {
    subject: 'hrn:hodei:artifacts:eu:acme:repository/libs',
    version: '1',
    content_hash: 'b'.repeat(64),
    profile: {
      subject: {
        partition: 'hodei',
        service: 'artifacts',
        region: 'eu',
        account_id: 'acme',
        resource_type: 'repository',
        resource_id: 'libs',
      },
      dimensions: [
        { name: 'integrity', state: 'verified', evidence: [] },
        { name: 'signature_state', state: 'missing', evidence: [] },
      ],
    },
  },
  trust: { state: 'quarantined', decision: null },
};

describe('PackageDetailView', () => {
  beforeEach(() => {
    server.use(
      http.get('/api/v1/packages/:repository/*', () =>
        HttpResponse.json(thePackage)
      )
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('says what the asset is', async () => {
    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    expect(await screen.findByText(`sha256:${theDigest}`)).toBeInTheDocument();
    expect(screen.getAllByText('2.0 KiB').length).toBeGreaterThan(0);
    expect(screen.getAllByText('2.32.3').length).toBeGreaterThan(0);
    expect(
      screen.getByText('libs · requests/2.32.3/requests-2.32.3.tar.gz')
    ).toBeInTheDocument();
  });

  it('says that nobody has granted the content', async () => {
    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    expect(await screen.findByText('Quarantined')).toBeInTheDocument();
  });

  it('says what the evidence says, dimension by dimension', async () => {
    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    expect(await screen.findByText('integrity')).toBeInTheDocument();
    expect(screen.getByText('verified')).toBeInTheDocument();
    expect(screen.getByText('signature_state')).toBeInTheDocument();
    expect(screen.getByText('missing')).toBeInTheDocument();
  });

  it('asks before it promotes with repository, path, and digest, and refreshes state on grant', async () => {
    const user = userEvent.setup();
    let promoted: unknown = null;
    let isTrusted = false;
    let detailFetches = 0;

    server.use(
      http.get('/api/v1/packages/:repository/*', () => {
        detailFetches += 1;
        return HttpResponse.json(
          isTrusted
            ? {
                ...thePackage,
                trust: {
                  state: 'trusted',
                  decision: {
                    decision_id: 'decision-1',
                    actor: 'hrn:hodei:iam::acme:User/alice',
                    granted_at: '2026-11-17T00:00:00Z',
                  },
                },
              }
            : thePackage
        );
      }),
      http.post('/api/v1/promotions', async ({ request }) => {
        promoted = await request.json();
        isTrusted = true;
        return HttpResponse.json({
          promoted: true,
          decision_id: 'decision-1',
          digest: `sha256:${theDigest}`,
          granted: [`sha256:${theDigest}`],
          risk_version: 42,
        });
      })
    );

    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    await user.click(await screen.findByRole('button', { name: /Promote/ }));

    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText('Promote to the trusted channel')
    ).toBeInTheDocument();
    expect(within(dialog).getByText('libs')).toBeInTheDocument();
    expect(
      within(dialog).getByText('requests/2.32.3/requests-2.32.3.tar.gz')
    ).toBeInTheDocument();
    expect(within(dialog).getByText(`sha256:${theDigest}`)).toBeInTheDocument();
    expect(promoted).toBeNull();

    const fetchesBefore = detailFetches;
    await user.click(within(dialog).getByRole('button', { name: 'Promote' }));

    await waitFor(() =>
      expect(promoted).toEqual({
        repository: 'libs',
        path: 'requests/2.32.3/requests-2.32.3.tar.gz',
        digest: `sha256:${theDigest}`,
      })
    );

    expect(await screen.findByText('Trusted')).toBeInTheDocument();
    expect(screen.getByText('Promotion granted')).toBeInTheDocument();
    expect(screen.getAllByText('decision-1').length).toBeGreaterThan(0);
    expect(detailFetches).toBeGreaterThan(fetchesBefore);
  });

  it('requires both reason and grant reference before revoking, and refreshes state', async () => {
    const user = userEvent.setup();
    let revoked: unknown = null;
    let isTrusted = true;

    server.use(
      http.get('/api/v1/packages/:repository/*', () =>
        HttpResponse.json(
          isTrusted
            ? {
                ...thePackage,
                trust: {
                  state: 'trusted',
                  decision: {
                    decision_id: 'decision-1',
                    actor: 'hrn:hodei:iam::acme:User/alice',
                    granted_at: '2026-11-17T00:00:00Z',
                  },
                },
              }
            : thePackage
        )
      ),
      http.post('/api/v1/promotions/revoke', async ({ request }) => {
        revoked = await request.json();
        isTrusted = false;
        return HttpResponse.json({ revoked: 1 });
      })
    );

    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    await user.click(await screen.findByRole('button', { name: /Revoke/ }));

    expect(
      await screen.findByText('Revoke the concession')
    ).toBeInTheDocument();
    const confirm = screen.getByRole('button', { name: 'Revoke' });
    expect(confirm).toBeDisabled();
    expect(revoked).toBeNull();

    // Reason alone is not enough if grant decision ID is cleared.
    await user.type(
      screen.getByLabelText('Reason'),
      'the signature does not hold'
    );
    expect(confirm).toBeEnabled();

    const decisionInput = screen.getByLabelText('Grant decision ID');
    await user.clear(decisionInput);
    expect(confirm).toBeDisabled();

    await user.type(decisionInput, 'decision-1');
    expect(confirm).toBeEnabled();
    await user.click(confirm);

    await waitFor(() =>
      expect(revoked).toEqual({
        repository: 'libs',
        decision_id: 'decision-1',
        reason: 'the signature does not hold',
      })
    );

    expect(await screen.findByText('Quarantined')).toBeInTheDocument();
    expect(screen.getByText('Trust grant revoked')).toBeInTheDocument();
  });

  it('supports revoking by canonical digest when selected', async () => {
    const user = userEvent.setup();
    let revoked: unknown = null;

    server.use(
      http.get('/api/v1/packages/:repository/*', () =>
        HttpResponse.json({
          ...thePackage,
          trust: {
            state: 'trusted',
            decision: {
              decision_id: 'decision-1',
              actor: 'hrn:hodei:iam::acme:User/alice',
              granted_at: '2026-11-17T00:00:00Z',
            },
          },
        })
      ),
      http.post('/api/v1/promotions/revoke', async ({ request }) => {
        revoked = await request.json();
        return HttpResponse.json({ revoked: 1 });
      })
    );

    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    await user.click(await screen.findByRole('button', { name: /Revoke/ }));
    await user.type(screen.getByLabelText('Reason'), 'compromised key');
    await user.click(screen.getByRole('radio', { name: 'By digest' }));
    await user.click(screen.getByRole('button', { name: 'Revoke' }));

    await waitFor(() =>
      expect(revoked).toEqual({
        repository: 'libs',
        digest: `sha256:${theDigest}`,
        reason: 'compromised key',
      })
    );
  });

  it('prevents duplicate promotion submissions while the mutation is in flight', async () => {
    const user = userEvent.setup();
    let postCount = 0;
    const deferred: { resolve?: () => void } = {};

    server.use(
      http.post('/api/v1/promotions', () => {
        postCount += 1;
        return new Promise(resolve => {
          deferred.resolve = () =>
            resolve(
              HttpResponse.json({
                promoted: true,
                decision_id: 'decision-1',
                digest: `sha256:${theDigest}`,
                granted: [`sha256:${theDigest}`],
                risk_version: 1,
              })
            );
        });
      })
    );

    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    await user.click(await screen.findByRole('button', { name: /Promote/ }));
    const confirm = await screen.findByRole('button', { name: 'Promote' });
    await user.click(confirm);

    await waitFor(() => expect(confirm).toBeDisabled());
    await user.click(confirm);
    expect(postCount).toBe(1);

    deferred.resolve?.();
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    );
  });

  it('consolidates identity and size without duplicating equivalent facts', async () => {
    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    expect(await screen.findByText(`sha256:${theDigest}`)).toBeInTheDocument();
    expect(screen.getAllByText(`sha256:${theDigest}`)).toHaveLength(1);
    expect(screen.queryByText(theDigest)).not.toBeInTheDocument();
    expect(screen.getAllByText('2.0 KiB')).toHaveLength(1);
    expect(screen.getByText('libs')).toBeInTheDocument();
    expect(
      screen.getByText('requests/2.32.3/requests-2.32.3.tar.gz')
    ).toBeInTheDocument();
  });

  it('copies digest, reference, and clean shareable link without credentials', async () => {
    const user = userEvent.setup();
    const writeText = vi
      .spyOn(navigator.clipboard, 'writeText')
      .mockResolvedValue(undefined);
    useAuthStore.setState({ token: 'secret-bearer-token' });

    render(
      <MemoryRouter
        initialEntries={['/console/packages/libs/requests/2.32.3?token=leak']}
      >
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    await screen.findByText(`sha256:${theDigest}`);

    await user.click(screen.getByRole('button', { name: 'Copy digest' }));
    expect(writeText).toHaveBeenLastCalledWith(`sha256:${theDigest}`);

    await user.click(screen.getByRole('button', { name: 'Copy reference' }));
    expect(writeText).toHaveBeenLastCalledWith(
      'libs/requests/2.32.3/requests-2.32.3.tar.gz'
    );

    await user.click(screen.getByRole('button', { name: 'Copy link' }));
    const copiedLink = writeText.mock.calls.at(-1)?.[0] ?? '';
    expect(copiedLink).toContain(
      `/console/packages/libs/requests/2.32.3/requests-2.32.3.tar.gz?digest=sha256%3A${theDigest}`
    );
    expect(copiedLink).not.toContain('secret-bearer-token');
    expect(copiedLink).not.toContain('leak');

    useAuthStore.setState({ token: null });
  });

  it('warns when the expected digest in the URL does not match the current content', async () => {
    const expected = `sha256:${'f'.repeat(64)}`;
    render(
      <MemoryRouter
        initialEntries={[
          `/console/packages/libs/requests/2.32.3?digest=${encodeURIComponent(expected)}`,
        ]}
      >
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(
      'Content changed: this path no longer matches the expected digest'
    );
    expect(alert).toHaveTextContent(expected);
    expect(alert).toHaveTextContent(`sha256:${theDigest}`);
  });

  it('does not warn when the expected digest in the URL matches the current content', async () => {
    const expected = `sha256:${theDigest}`;
    render(
      <MemoryRouter
        initialEntries={[
          `/console/packages/libs/requests/2.32.3?digest=${encodeURIComponent(expected)}`,
        ]}
      >
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    await screen.findByText(`sha256:${theDigest}`);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('preserves special characters in paths across router navigation and link generation', async () => {
    const user = userEvent.setup();
    const writeText = vi
      .spyOn(navigator.clipboard, 'writeText')
      .mockResolvedValue(undefined);
    const specialPath = '@scope/pkg/1.0.0+build 1/foo%2Fbar.tgz';
    let requestedUrl = '';

    server.use(
      http.get('/api/v1/packages/:repository/*', ({ request }) => {
        requestedUrl = new URL(request.url).pathname;
        return HttpResponse.json({
          ...thePackage,
          repository: 'npm-hosted',
          asset_path: specialPath,
          package_name: '@scope/pkg',
          package_version: '1.0.0+build 1',
        });
      })
    );

    render(
      <MemoryRouter
        initialEntries={[
          '/console/packages/npm-hosted/%40scope/pkg/1.0.0%2Bbuild%201/foo%252Fbar.tgz',
        ]}
      >
        <Routes>
          <Route
            path="/console/packages/:repository/*"
            element={<ConsolePackagePage />}
          />
        </Routes>
      </MemoryRouter>
    );

    expect(await screen.findByText(specialPath)).toBeInTheDocument();
    expect(requestedUrl).toBe(
      '/api/v1/packages/npm-hosted/%40scope/pkg/1.0.0%2Bbuild%201/foo%252Fbar.tgz'
    );

    await user.click(screen.getByRole('button', { name: 'Copy link' }));
    const copiedLink = writeText.mock.calls.at(-1)?.[0] ?? '';
    expect(copiedLink).toContain(
      `/console/packages/npm-hosted/%40scope/pkg/1.0.0%2Bbuild%201/foo%252Fbar.tgz?digest=sha256%3A${theDigest}`
    );
  });
  it('fails closed when the trust decision changed before revocation', async () => {
    const user = userEvent.setup();
    let packageReads = 0;
    let revokeCalls = 0;

    server.use(
      http.get('/api/v1/packages/:repository/*', () => {
        packageReads += 1;
        const decisionId = packageReads === 1 ? 'decision-1' : 'decision-2';
        return HttpResponse.json({
          ...thePackage,
          trust: {
            state: 'trusted',
            decision: {
              decision_id: decisionId,
              actor: 'hrn:hodei:iam::acme:User/alice',
              granted_at: '2026-11-17T00:00:00Z',
            },
          },
        });
      }),
      http.post('/api/v1/promotions/revoke', () => {
        revokeCalls += 1;
        return HttpResponse.json({ revoked: 1 });
      })
    );

    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    await user.click(await screen.findByRole('button', { name: /Revoke/ }));
    await user.type(
      await screen.findByLabelText('Reason'),
      'the trust decision changed'
    );
    const dialog = screen.getByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Revoke' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The trust decision changed. Reopen the dialog before revoking.'
    );
    expect(revokeCalls).toBe(0);
    expect(screen.getAllByText('decision-2').length).toBeGreaterThan(0);
  });

  it.each([
    {
      label: '403 permission denial',
      status: 403,
      body: { error: 'permission', message: 'forbidden' },
      expectedText: 'You are not allowed to do this',
    },
    {
      label: '409 conflict',
      status: 409,
      body: { error: 'conflict', message: 'conflict' },
      expectedText: 'It already exists',
    },
    {
      label: '422 trust refusal',
      status: 422,
      body: { error: 'refused', message: 'policy refused' },
      expectedText: 'The trust policy did not allow it',
    },
    {
      label: '503 service unavailable',
      status: 503,
      body: { error: 'unavailable', message: 'promotions are not available' },
      expectedText: 'The service cannot answer right now',
    },
  ])(
    'never shows grant or success on $label',
    async ({ status, body, expectedText }) => {
      const user = userEvent.setup();
      server.use(
        http.post('/api/v1/promotions', () =>
          HttpResponse.json(body, { status })
        )
      );

      render(
        <MemoryRouter>
          <PackageDetailView repository="libs" path="requests/2.32.3" />
        </MemoryRouter>
      );

      await user.click(await screen.findByRole('button', { name: /Promote/ }));
      await user.click(await screen.findByRole('button', { name: 'Promote' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(expectedText);
      expect(screen.queryByText('Promotion granted')).not.toBeInTheDocument();
      expect(screen.getByText('Quarantined')).toBeInTheDocument();
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    }
  );
});
