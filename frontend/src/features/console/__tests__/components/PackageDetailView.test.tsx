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
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

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

    await user.type(screen.getByLabelText('Reason'), 'the signature does not hold');
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
});
