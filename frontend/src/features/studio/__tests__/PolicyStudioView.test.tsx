/**
 * The Policy Studio view, against the contract of `/api/v1/policy-studio`.
 *
 * What is measured: the view says what the server said (head, revision,
 * validation, effects, diff, history), tells apart the four refusals, never
 * offers a field for a capability the backend does not have, loses no
 * draft on a conflict and leaves everything as it was on a denial.
 */

import { http, HttpResponse } from 'msw';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { server } from '@/shared/test/mocks/server';
import { render } from '@/shared/test/test-utils';
import { useAuthStore } from '@/shared/stores/auth.store';

import { PolicyStudioView } from '../components/PolicyStudioView';

const BASE = '/api/v1/policy-studio';

const policy = (accepts: string[], version = '1') => ({
  id: 'default',
  version,
  schema_version: '1.0.0',
  requirements: [
    {
      kind: { kind: 'dimension', dimension: 'inventory_completeness', accepts },
      on_failure: 'deny',
      overridable_by: null,
    },
  ],
});

const head = (sequence: number) => ({
  policy_id: 'default',
  sequence,
  digest: `sha256:${String(sequence).repeat(64).slice(0, 64)}`,
  activated_by: 'hrn:hodei:iam::acme:User/carol',
  editor: 'hrn:hodei:iam::acme:User/alice',
  reason: 'go live',
  activated_at: '2026-11-26T10:00:00Z',
});

const revision = (sequence: number, extra: object = {}) => ({
  policy_id: 'default',
  sequence,
  digest: head(sequence).digest,
  policy: policy(['COMPLETE'], String(sequence)),
  saved_by: 'hrn:hodei:iam::acme:User/alice',
  saved_at: '2026-11-26T09:00:00Z',
  reason: `revision ${sequence}`,
  ...extra,
});

const historyPage = (over: object = {}) => ({
  offset: 0,
  limit: 20,
  revisions_truncated: false,
  events_truncated: false,
  policy_id: 'default',
  revisions: [revision(1), revision(2)],
  events: [],
  ...over,
});

/** The server's answer for a head at `sequence`. */
const serveHead = (sequence: number) => {
  server.use(
    http.get(`${BASE}/head`, () =>
      HttpResponse.json({ exists: true, head: head(sequence) })
    ),
    http.get(`${BASE}/revisions`, () =>
      HttpResponse.json({
        policy_id: 'default',
        sequence,
        digest: head(sequence).digest,
        policy: policy(['COMPLETE'], String(sequence)),
        saved_by: 'hrn:hodei:iam::acme:User/alice',
        reason: 'go live',
        active: true,
      })
    ),
    http.get(`${BASE}/history`, () => HttpResponse.json(historyPage()))
  );
};

const denied = () =>
  HttpResponse.json(
    { error: 'forbidden', message: 'the gate says no' },
    { status: 403 }
  );

const editor = () => screen.getByTestId('studio-editor') as HTMLTextAreaElement;

const decision = (outcome: string) => ({
  outcome,
  reasons: [],
  policy_id: 'default',
  policy_version: '1',
  evidence_ids: ['sbom-0001'],
});

describe('PolicyStudioView', () => {
  beforeEach(() => {
    useAuthStore.setState({ token: 'a-token' });
  });
  afterEach(() => {
    useAuthStore.getState().logout();
  });

  it('loads the head and the revision that decides', async () => {
    serveHead(3);
    render(<PolicyStudioView />);

    expect(await screen.findByTestId('studio-head')).toHaveTextContent(
      'Revision 3'
    );
    expect(editor().value).toContain('"accepts"');
    expect(screen.getByTestId('studio-activate')).toHaveTextContent(
      'Activate revision 3'
    );
    expect(await screen.findByTestId('revision-1')).toBeInTheDocument();
  });

  it('starts a draft that cannot be activated when nothing was ever activated', async () => {
    server.use(
      http.get(`${BASE}/head`, () => HttpResponse.json({ exists: false })),
      http.get(`${BASE}/history`, () =>
        HttpResponse.json(historyPage({ revisions: [] }))
      )
    );
    render(<PolicyStudioView />);

    expect(await screen.findByTestId('studio-head')).toHaveTextContent(
      'Never activated'
    );
    expect(screen.getByTestId('studio-activate')).toBeDisabled();
  });

  it('lets the approver open a saved revision and activate it with the head they saw', async () => {
    let activated: Record<string, unknown> = {};
    server.use(
      http.get(`${BASE}/head`, () => HttpResponse.json({ exists: false })),
      http.get(`${BASE}/history`, () => HttpResponse.json(historyPage())),
      http.get(`${BASE}/revisions`, ({ request }) => {
        expect(new URL(request.url).searchParams.get('sequence')).toBe('2');
        return HttpResponse.json({
          ...revision(2),
          active: false,
        });
      }),
      http.post(`${BASE}/activate`, async ({ request }) => {
        activated = (await request.json()) as Record<string, unknown>;
        return HttpResponse.json({ head: head(2), applied: true });
      })
    );
    render(<PolicyStudioView />);
    expect(await screen.findByTestId('studio-activate')).toBeDisabled();

    await userEvent.click(await screen.findByTestId('open-2'));
    await waitFor(() =>
      expect(screen.getByTestId('studio-activate')).toHaveTextContent(
        'Activate revision 2'
      )
    );
    await userEvent.click(screen.getByTestId('studio-activate'));
    await userEvent.click(
      await screen.findByRole('button', { name: /Confirm: activate/ })
    );

    await waitFor(() =>
      expect(screen.getByTestId('studio-notice')).toHaveTextContent(
        'Revision 2 is the head'
      )
    );
    expect(activated).toMatchObject({ sequence: 2, expected_head: null });
    // Who activates is the principal of the token: the body never says.
    expect(Object.keys(activated).sort()).toEqual([
      'expected_head',
      'reason',
      'sequence',
    ]);
  });

  it('validates the draft and says what the server said', async () => {
    serveHead(1);
    server.use(
      http.post(`${BASE}/validate`, () =>
        HttpResponse.json({
          valid: false,
          error: 'schema mismatch',
          applied: false,
        })
      )
    );
    render(<PolicyStudioView />);
    await screen.findByTestId('studio-head');

    await userEvent.click(screen.getByTestId('studio-validate'));

    expect(
      await screen.findByTestId('studio-validation-answer')
    ).toHaveTextContent('The draft is refused: schema mismatch');
  });

  it('tells a decision, an evidence gap and an engine error apart, and shows the diff', async () => {
    serveHead(1);
    let sent: Record<string, unknown> = {};
    server.use(
      http.post(`${BASE}/simulate`, async ({ request }) => {
        sent = (await request.json()) as Record<string, unknown>;
        return HttpResponse.json({
          candidate: { policy: policy(['COMPLETE']), revision: null },
          active_version: '1',
          applied: false,
          effects: [
            {
              subject: 'decided',
              kind: 'POLICY_DECISION',
              decision: decision('DENY'),
              gap: null,
              error: null,
            },
            {
              subject: 'gapped',
              kind: 'EVIDENCE_GAP',
              decision: decision('INDETERMINATE'),
              gap: {
                requirement: 'inventory_completeness',
                dimension: 'inventory_completeness',
                reason: 'MISSING_EVIDENCE',
                evidence_ids: ['sbom-9'],
              },
              error: null,
            },
            {
              subject: 'broken',
              kind: 'ENGINE_ERROR',
              decision: null,
              gap: null,
              error: 'the snapshot cannot be read',
            },
          ],
          changes: [
            {
              subject: 'decided',
              before: 'ALLOW',
              after: 'DENY',
              loosens: false,
            },
          ],
        });
      })
    );
    render(<PolicyStudioView />);
    await screen.findByTestId('studio-head');

    await userEvent.click(screen.getByTestId('studio-simulate'));
    await screen.findByTestId('studio-effects');

    expect(screen.getByTestId('effect-decided')).toHaveAttribute(
      'data-kind',
      'POLICY_DECISION'
    );
    expect(screen.getByTestId('kind-decided')).toHaveTextContent(
      'Policy decision · DENY'
    );
    expect(screen.getByTestId('effect-gapped')).toHaveAttribute(
      'data-kind',
      'EVIDENCE_GAP'
    );
    expect(screen.getByTestId('effect-gapped')).toHaveTextContent(
      'MISSING_EVIDENCE'
    );
    expect(screen.getByTestId('effect-gapped')).toHaveTextContent('sbom-9');
    expect(screen.getByTestId('effect-gapped')).not.toHaveTextContent(
      'Policy decision'
    );
    expect(screen.getByTestId('effect-broken')).toHaveAttribute(
      'data-kind',
      'ENGINE_ERROR'
    );
    expect(screen.getByTestId('effect-broken')).toHaveTextContent(
      'the snapshot cannot be read'
    );
    expect(screen.getByTestId('change-decided')).toHaveTextContent(
      'ALLOW → DENY'
    );
    expect(screen.getByTestId('studio-simulated')).toHaveTextContent(
      'applied: no'
    );
    // A simulation has no way to activate anything.
    expect(
      within(screen.getByTestId('studio-effects')).queryAllByRole('button')
    ).toHaveLength(0);
    // The body carries the draft and the snapshots, and nothing else.
    expect(Object.keys(sent).sort()).toEqual(['policy', 'subjects']);
  });

  it('offers no field for a capability the backend does not have', async () => {
    serveHead(1);
    render(<PolicyStudioView />);
    await screen.findByTestId('studio-head');

    for (const word of [/shadow/i, /approver:/i, /\baccount\b/i]) {
      expect(screen.queryByLabelText(word)).not.toBeInTheDocument();
      expect(
        screen.queryByRole('textbox', { name: word })
      ).not.toBeInTheDocument();
    }
    expect(document.body.textContent).not.toMatch(/shadow/i);
    // Exactly the fields the backend takes: the draft, the snapshots and the reason.
    expect(
      screen
        .getAllByRole('textbox')
        .map(field => field.getAttribute('aria-label'))
    ).toEqual(['Policy draft', 'Evidence snapshots', 'Reason']);
    expect(screen.queryByRole('combobox')).not.toBeInTheDocument();
  });

  it('closes the studio on the 403 of the first read', async () => {
    server.use(
      http.get(`${BASE}/head`, denied),
      http.get(`${BASE}/history`, denied)
    );
    render(<PolicyStudioView />);

    expect(await screen.findByTestId('studio-denied')).toHaveTextContent(
      'Access denied'
    );
    expect(screen.queryByTestId('studio-editor')).not.toBeInTheDocument();
    expect(screen.queryByTestId('studio-activate')).not.toBeInTheDocument();
    expect(screen.queryByTestId('studio-save')).not.toBeInTheDocument();
  });

  it('leaves everything as it was when an action is denied', async () => {
    serveHead(3);
    server.use(http.post(`${BASE}/activate`, denied));
    render(<PolicyStudioView />);
    await screen.findByTestId('studio-head');
    const before = editor().value;

    await userEvent.click(screen.getByTestId('studio-activate'));
    await userEvent.click(
      await screen.findByRole('button', { name: /Confirm: activate/ })
    );

    const alert = await screen.findByTestId('studio-action-denied');
    expect(alert).toHaveTextContent('Access denied');
    expect(alert).toHaveTextContent('Nothing was changed');
    expect(screen.getByTestId('studio-head')).toHaveTextContent('Revision 3');
    expect(editor().value).toBe(before);
    expect(screen.queryByTestId('studio-notice')).not.toBeInTheDocument();
  });

  it('keeps the draft, reloads the head and says where it is on a stale head', async () => {
    serveHead(3);
    let headNow = 3;
    let sentExpected: unknown;
    server.use(
      http.get(`${BASE}/head`, () =>
        HttpResponse.json({ exists: true, head: head(headNow) })
      ),
      http.post(`${BASE}/activate`, async ({ request }) => {
        sentExpected = ((await request.json()) as { expected_head: unknown })
          .expected_head;
        headNow = 5;
        return HttpResponse.json(
          {
            error: 'stale_head',
            message: 'the head is 5, not the head the caller saw',
            found_head: 5,
          },
          { status: 409 }
        );
      })
    );
    render(<PolicyStudioView />);
    await screen.findByTestId('studio-head');
    const typed = JSON.stringify(
      policy(['COMPLETE', 'PARTIAL'], 'typed'),
      null,
      2
    );
    fireEvent.change(editor(), { target: { value: typed } });

    await userEvent.click(screen.getByTestId('studio-activate'));
    await userEvent.click(
      await screen.findByRole('button', { name: /Confirm: activate/ })
    );

    expect(await screen.findByTestId('studio-notice')).toHaveTextContent(
      'The head moved: it is now at revision 5. Your draft was kept.'
    );
    expect(sentExpected).toBe(3);
    await waitFor(() =>
      expect(screen.getByTestId('studio-head')).toHaveTextContent('Revision 5')
    );
    expect(editor().value).toBe(typed);
  });

  it('rolls back from the history and shows who rolled back', async () => {
    serveHead(2);
    let body: Record<string, unknown> = {};
    server.use(
      http.get(`${BASE}/history`, () =>
        HttpResponse.json(
          historyPage({
            revisions: [
              revision(1),
              revision(2),
              revision(3, { rolled_back_by: 'hrn:hodei:iam::acme:User/carol' }),
            ],
          })
        )
      ),
      http.post(`${BASE}/rollback`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>;
        return HttpResponse.json({ head: head(3), applied: true });
      })
    );
    render(<PolicyStudioView />);

    expect(await screen.findByTestId('rolled-back-by-3')).toHaveTextContent(
      'rolled back by hrn:hodei:iam::acme:User/carol'
    );
    expect(screen.queryByTestId('rollback-2')).not.toBeInTheDocument();

    await userEvent.click(await screen.findByTestId('rollback-1'));
    await userEvent.click(
      await screen.findByRole('button', { name: 'Confirm: roll back' })
    );

    expect(await screen.findByTestId('studio-notice')).toHaveTextContent(
      'Rolled back to revision 1'
    );
    expect(body).toMatchObject({ target: 1, expected_head: 2 });
    expect(Object.keys(body).sort()).toEqual([
      'expected_head',
      'reason',
      'target',
    ]);
  });

  it('says that rolling back to the head is a conflict', async () => {
    serveHead(2);
    server.use(
      http.post(`${BASE}/rollback`, () =>
        HttpResponse.json(
          { error: 'rollback_to_head', message: 'revision 2 already decides' },
          { status: 409 }
        )
      )
    );
    render(<PolicyStudioView />);

    await userEvent.click(await screen.findByTestId('rollback-1'));
    await userEvent.click(
      await screen.findByRole('button', { name: 'Confirm: roll back' })
    );

    expect(await screen.findByTestId('studio-failure')).toHaveTextContent(
      'revision 2 already decides'
    );
  });

  it('pages the history with the offset and says when the window is cut', async () => {
    serveHead(1);
    const offsets: string[] = [];
    server.use(
      http.get(`${BASE}/history`, ({ request }) => {
        const url = new URL(request.url);
        offsets.push(
          `${url.searchParams.get('offset')}/${url.searchParams.get('limit')}`
        );
        const first = url.searchParams.get('offset') === '0';
        return HttpResponse.json(
          historyPage({
            offset: Number(url.searchParams.get('offset')),
            revisions_truncated: first,
            revisions: first ? [revision(1)] : [revision(2)],
          })
        );
      })
    );
    render(<PolicyStudioView />);

    expect(await screen.findByTestId('studio-truncated')).toBeInTheDocument();
    await userEvent.click(screen.getByTestId('history-next'));
    expect(await screen.findByTestId('revision-2')).toBeInTheDocument();

    expect(offsets).toEqual(['0/20', '20/20']);
    expect(screen.queryByTestId('studio-truncated')).not.toBeInTheDocument();
    expect(screen.getByTestId('history-next')).toBeDisabled();
    expect(screen.getByTestId('history-previous')).toBeEnabled();
  });

  it('says that the body is too large on a 413', async () => {
    serveHead(1);
    server.use(
      http.post(`${BASE}/revisions`, () =>
        HttpResponse.text('length limit exceeded', { status: 413 })
      )
    );
    render(<PolicyStudioView />);
    await screen.findByTestId('studio-head');

    await userEvent.click(screen.getByTestId('studio-save'));

    expect(await screen.findByTestId('studio-failure')).toHaveTextContent(
      '512 KiB'
    );
  });

  it('says that the lifecycle has no durable store on a 501', async () => {
    server.use(
      http.get(`${BASE}/head`, () =>
        HttpResponse.json(
          { error: 'not_implemented', message: 'no durable store' },
          { status: 501 }
        )
      )
    );
    render(<PolicyStudioView />);

    expect(await screen.findByTestId('studio-failure')).toHaveTextContent(
      'no durable store'
    );
    expect(screen.queryByTestId('studio-editor')).not.toBeInTheDocument();
  });

  it('refuses to send a draft that is not JSON', async () => {
    serveHead(1);
    render(<PolicyStudioView />);
    await screen.findByTestId('studio-head');
    fireEvent.change(editor(), { target: { value: '{ not json' } });

    await userEvent.click(screen.getByTestId('studio-validate'));

    expect(await screen.findByTestId('studio-input-error')).toHaveTextContent(
      'The draft is not valid JSON'
    );
  });
});
