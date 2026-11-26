/**
 * The Policy Studio, end to end, against the real service.
 *
 * The service is the binary that ships, on the real Dgraph, with a Cedar
 * bundle that tells the principals apart (see the fixture): `alice` edits,
 * `carol` approves, `viewer` only reads and `bob` has no grant at all. The
 * studio separates duties, so the author of a revision cannot approve it:
 * the lane needs two people, as a deployment does.
 *
 * What is measured here is the console against the contract:
 *
 * - a modification is validated, simulated (a decision, an evidence gap
 *   and an engine error are told apart) and saved as a revision that
 *   decides nothing, and its author cannot approve it (403);
 * - a different principal opens the revision, activates it and the head
 *   moves;
 * - a head that moved under an editor arrives as a 409 `stale_head`, the
 *   studio says where the head is and keeps the draft;
 * - a rollback is a new revision that says who rolled back;
 * - the denial of access is its own state: no grant closes the studio,
 *   and a read-only grant leaves an attempted write without effect.
 *
 * The tests run in order over one store, and the sequence numbers are
 * read from the page, never guessed.
 */

import { readFileSync } from 'node:fs';

import { expect, test, type Page } from '@playwright/test';

import { theTokenPath, theTokenPathOf } from './fixture/token.mjs';

type Principal = 'alice' | 'carol' | 'viewer' | 'bob';

/** The token that the fixture signed for a principal. */
const tokenOf = (who: Principal) =>
  readFileSync(
    who === 'alice' ? theTokenPath : theTokenPathOf(who),
    'utf8'
  ).trim();

/** Signs the browser in as a principal and opens the studio. */
const openStudio = async (page: Page, who: Principal) => {
  await page.addInitScript(
    ([name, token]) => {
      window.localStorage.setItem(
        'hodei-auth-token',
        JSON.stringify({
          state: {
            user: { name, role: 'admin' },
            token,
            isAuthenticated: true,
          },
          version: 0,
        })
      );
    },
    [who, tokenOf(who)]
  );
  await page.goto('/console/policy-studio');
  await expect(page.getByTestId('policy-studio')).toBeVisible();
};

/** A policy whose only requirement accepts the given states. */
const policyText = (version: string, accepts: string[]) =>
  JSON.stringify(
    {
      id: 'default',
      version,
      schema_version: '1.0.0',
      requirements: [
        {
          kind: {
            kind: 'dimension',
            dimension: 'inventory_completeness',
            accepts,
          },
          on_failure: 'deny',
          overridable_by: null,
        },
      ],
    },
    null,
    2
  );

/** An evidence snapshot of one subject. */
const snapshot = (
  subject: string,
  state: string | null,
  version = '1.0.0'
) => ({
  subject,
  context: {
    version,
    subject: {
      hrn: `hrn:hodei:artifact::acme:oci:registry/${subject}:latest`,
      digest: null,
    },
    target: 'production',
    dimensions:
      state === null ? [] : [{ name: 'inventory_completeness', state }],
    capsule_hash: null,
    risk: {
      critical_known: 0,
      critical_reachable: 0,
      exposure: 'UNKNOWN',
      vex_accepted: 0,
      established: false,
      risk_version: 0,
    },
    exceptions: [],
    evidence_ids: ['sbom-0001'],
    assembled_at: '2026-11-20T00:00:00Z',
  },
});

/** The sequence that a save announced. */
const savedSequence = async (page: Page) => {
  const notice = page.getByTestId('studio-notice');
  await expect(notice).toContainText(/Revision \d+ saved/);
  const text = (await notice.textContent()) ?? '';
  return Number(/Revision (\d+) saved/.exec(text)?.[1]);
};

/** Saves the draft as a revision, as the signed-in principal. */
const saveDraft = async (page: Page, text: string, reason: string) => {
  await page.getByTestId('studio-editor').fill(text);
  await page.getByTestId('studio-reason').fill(reason);
  await page.getByTestId('studio-save').click();
  return savedSequence(page);
};

/** Confirms the activation of the revision the editor stands on. */
const activate = async (page: Page) => {
  await page.getByTestId('studio-activate').click();
  await page
    .getByRole('button', { name: /Confirm: activate revision/ })
    .click();
};

/** What the sequences of the lane are, for the tests that follow. */
const lane: { first: number; second: number; third: number; head: number } = {
  first: 0,
  second: 0,
  third: 0,
  head: 0,
};

test.describe('the Policy Studio', () => {
  test('the editor validates, simulates and saves a revision that decides nothing, and cannot approve it', async ({
    page,
  }) => {
    await openStudio(page, 'alice');

    // Nothing was ever activated: the studio measures against no baseline,
    // and there is no revision to activate yet.
    await expect(page.getByTestId('studio-head')).toContainText(
      'Never activated'
    );
    await expect(page.getByTestId('studio-activate')).toBeDisabled();

    // The studio offers the fields of the backend and no other.
    await expect(page.getByRole('textbox')).toHaveCount(3);
    await expect(page.getByText(/shadow/i)).toHaveCount(0);

    await page
      .getByTestId('studio-editor')
      .fill(policyText('e2e-1', ['COMPLETE', 'PARTIAL']));
    await page.getByTestId('studio-validate').click();
    await expect(page.getByTestId('studio-validation-answer')).toContainText(
      'The store would accept this draft'
    );

    // A decision, an evidence gap and an engine error, side by side.
    await page
      .getByTestId('studio-snapshots')
      .fill(
        JSON.stringify([
          snapshot('decided', 'COMPLETE'),
          snapshot('gapped', null),
          snapshot('broken', 'COMPLETE', '9.9.9'),
        ])
      );
    await page.getByTestId('studio-simulate').click();
    await expect(page.getByTestId('studio-effects')).toBeVisible();
    await expect(page.getByTestId('effect-decided')).toHaveAttribute(
      'data-kind',
      'POLICY_DECISION'
    );
    await expect(page.getByTestId('kind-decided')).toContainText(
      'Policy decision'
    );
    await expect(page.getByTestId('effect-gapped')).toHaveAttribute(
      'data-kind',
      'EVIDENCE_GAP'
    );
    await expect(page.getByTestId('effect-gapped')).toContainText(
      'inventory_completeness'
    );
    await expect(page.getByTestId('effect-broken')).toHaveAttribute(
      'data-kind',
      'ENGINE_ERROR'
    );
    await expect(page.getByTestId('studio-simulated')).toContainText(
      'applied: no'
    );
    await expect(
      page.getByTestId('studio-effects').locator('button')
    ).toHaveCount(0);

    lane.first = await saveDraft(
      page,
      policyText('e2e-1', ['COMPLETE', 'PARTIAL']),
      'first'
    );
    // Saving is not activating: the head has not moved.
    await expect(page.getByTestId('studio-head')).toContainText(
      'Never activated'
    );
    await expect(page.getByTestId(`revision-${lane.first}`)).toBeVisible();
    await expect(page.getByTestId('studio-activate')).toContainText(
      `Activate revision ${lane.first}`
    );

    // The author cannot approve their own revision: the separation of
    // duties answers 403 and nothing happens.
    await activate(page);
    await expect(page.getByTestId('studio-action-denied')).toContainText(
      'Access denied'
    );
    await expect(page.getByTestId('studio-action-denied')).toContainText(
      /separation/i
    );
    await expect(page.getByTestId('studio-head')).toContainText(
      'Never activated'
    );
  });

  test('the approver opens the revision and activates it: the head moves', async ({
    page,
  }) => {
    await openStudio(page, 'carol');
    await expect(page.getByTestId('studio-head')).toContainText(
      'Never activated'
    );

    await page.getByTestId(`open-${lane.first}`).click();
    await expect(page.getByTestId('studio-editing')).toContainText(
      `Editing revision ${lane.first}`
    );
    await activate(page);

    await expect(page.getByTestId('studio-notice')).toContainText(
      'is the head'
    );
    await expect(page.getByTestId('studio-head')).toContainText(
      `Revision ${lane.first}`
    );
    // The approver and the editor are the principals of the tokens.
    await expect(page.getByTestId('studio-head')).toContainText(
      'activated by hrn:hodei:iam::acme:User/carol'
    );
    await expect(page.getByTestId('studio-head')).toContainText(
      'edited by hrn:hodei:iam::acme:User/alice'
    );
    lane.head = lane.first;
  });

  test('a head that moved arrives as a conflict, with the head that is, and the draft is kept', async ({
    page,
    browser,
  }) => {
    // The editor saves two more revisions.
    const editor = await browser.newContext();
    const editorPage = await editor.newPage();
    await openStudio(editorPage, 'alice');
    lane.second = await saveDraft(
      editorPage,
      policyText('e2e-2', ['COMPLETE']),
      'second'
    );
    lane.third = await saveDraft(
      editorPage,
      policyText('e2e-3', ['COMPLETE', 'VERIFIED']),
      'third'
    );
    await editor.close();

    // The first approver loads the head and opens the second revision.
    await openStudio(page, 'carol');
    await expect(page.getByTestId('studio-head')).toContainText(
      `Revision ${lane.head}`
    );
    await page.getByTestId(`open-${lane.second}`).click();
    await expect(page.getByTestId('studio-editing')).toContainText(
      `Editing revision ${lane.second}`
    );

    // The diff against the head that decides: a partial inventory was
    // accepted by the active policy and the draft refuses it.
    await page
      .getByTestId('studio-snapshots')
      .fill(JSON.stringify([snapshot('web-app', 'PARTIAL')]));
    await page.getByTestId('studio-simulate').click();
    await expect(page.getByTestId('change-web-app')).toContainText(
      'ALLOW → DENY'
    );

    // A second approver moves the head while the first one still holds it.
    const other = await browser.newContext();
    const otherPage = await other.newPage();
    await openStudio(otherPage, 'carol');
    await otherPage.getByTestId(`open-${lane.third}`).click();
    await expect(otherPage.getByTestId('studio-editing')).toContainText(
      `Editing revision ${lane.third}`
    );
    await activate(otherPage);
    await expect(otherPage.getByTestId('studio-head')).toContainText(
      `Revision ${lane.third}`
    );
    await other.close();

    // The first one types a change and activates against the head they saw.
    const typed = policyText('e2e-typed', ['COMPLETE', 'UNVERIFIED']);
    await page.getByTestId('studio-editor').fill(typed);
    await activate(page);

    await expect(page.getByTestId('studio-notice')).toContainText(
      `The head moved: it is now at revision ${lane.third}`
    );
    // The studio reloaded the truth and kept what was typed.
    await expect(page.getByTestId('studio-head')).toContainText(
      `Revision ${lane.third}`
    );
    await expect(page.getByTestId('studio-editor')).toHaveValue(typed);
    await expect(page.getByTestId('studio-effects')).toHaveCount(0);
    lane.head = lane.third;
  });

  test('a rollback is a new revision that says who rolled back', async ({
    page,
  }) => {
    await openStudio(page, 'carol');
    await expect(page.getByTestId('studio-head')).toContainText(
      `Revision ${lane.head}`
    );

    // The head itself has nothing to roll back to.
    await expect(page.getByTestId(`rollback-${lane.head}`)).toHaveCount(0);

    await page.getByTestId(`rollback-${lane.first}`).click();
    await page.getByRole('button', { name: 'Confirm: roll back' }).click();

    await expect(page.getByTestId('studio-notice')).toContainText(
      `Rolled back to revision ${lane.first}`
    );
    // The rollback is a new revision: the head is not the old one.
    await expect(page.getByTestId('studio-head')).not.toContainText(
      `Revision ${lane.head} `
    );
    await expect(page.getByTestId('studio-history')).toContainText(
      'rolled back by hrn:hodei:iam::acme:User/carol'
    );
    // Its author is the one who wrote the content, not the one who rolled back.
    await expect(page.getByTestId('studio-head')).toContainText(
      'edited by hrn:hodei:iam::acme:User/alice'
    );
  });

  test('no grant closes the studio; a read-only grant leaves a write without effect', async ({
    page,
    browser,
  }) => {
    // No grant: the gate refuses the first read and the studio is closed.
    await openStudio(page, 'bob');
    await expect(page.getByTestId('studio-denied')).toContainText(
      'Access denied'
    );
    await expect(page.getByTestId('studio-editor')).toHaveCount(0);
    await expect(page.getByTestId('studio-activate')).toHaveCount(0);

    // Read-only: the studio opens, and the write is refused without effect.
    const reader = await browser.newContext();
    const readerPage = await reader.newPage();
    await openStudio(readerPage, 'viewer');
    await expect(readerPage.getByTestId('studio-head')).toContainText(
      'Revision'
    );
    const before = await readerPage.getByTestId('studio-history').textContent();

    await readerPage
      .getByTestId('studio-editor')
      .fill(policyText('e2e-denied', ['COMPLETE']));
    await readerPage.getByTestId('studio-save').click();
    await expect(readerPage.getByTestId('studio-action-denied')).toContainText(
      'Access denied'
    );
    await expect(readerPage.getByTestId('studio-notice')).toHaveCount(0);

    await readerPage.reload();
    await expect(readerPage.getByTestId('studio-history')).toHaveText(
      before ?? ''
    );
    await reader.close();
  });
});
