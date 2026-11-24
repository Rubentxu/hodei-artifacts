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
import { screen, waitFor } from '@testing-library/react';
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

  it('asks before it promotes, and promotes what was confirmed', async () => {
    const user = userEvent.setup();
    let promoted: unknown = null;
    server.use(
      http.post('/api/v1/promotions', async ({ request }) => {
        promoted = await request.json();
        return HttpResponse.json({
          promoted: true,
          decision_id: 'decision-1',
          digest: `sha256:${theDigest}`,
          granted: [`sha256:${theDigest}`],
        });
      })
    );

    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    await user.click(await screen.findByRole('button', { name: /Promote/ }));

    expect(
      await screen.findByText('Promote to the trusted channel')
    ).toBeInTheDocument();
    expect(promoted).toBeNull();

    await user.click(screen.getByRole('button', { name: 'Promote' }));

    await waitFor(() =>
      expect(promoted).toEqual({
        repository: 'libs',
        path: 'requests/2.32.3/requests-2.32.3.tar.gz',
        digest: `sha256:${theDigest}`,
      })
    );
  });

  it('asks for a reason before it revokes', async () => {
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

    expect(
      await screen.findByText('Revoke the concession')
    ).toBeInTheDocument();
    const confirm = screen.getByRole('button', { name: 'Revoke' });
    expect(confirm).toBeDisabled();
    expect(revoked).toBeNull();

    await user.type(
      screen.getByLabelText('Reason'),
      'the signature does not hold'
    );
    await user.click(confirm);

    await waitFor(() =>
      expect(revoked).toEqual({
        repository: 'libs',
        digest: `sha256:${theDigest}`,
        reason: 'the signature does not hold',
      })
    );
  });

  it('says what the API refused when the promotion does not go through', async () => {
    const user = userEvent.setup();
    server.use(
      http.post('/api/v1/promotions', () =>
        HttpResponse.json(
          { error: 'permission', message: 'forbidden' },
          { status: 403 }
        )
      )
    );

    render(
      <MemoryRouter>
        <PackageDetailView repository="libs" path="requests/2.32.3" />
      </MemoryRouter>
    );

    await user.click(await screen.findByRole('button', { name: /Promote/ }));
    await user.click(await screen.findByRole('button', { name: 'Promote' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'You are not allowed to do this'
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
});
