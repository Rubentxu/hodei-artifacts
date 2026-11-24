/**
 * The confirmation of an action that cannot be undone.
 *
 * What is measured here is that the keyboard does what it says: the focus
 * goes to the button that confirms, the Enter key confirms what the focus
 * is on, and a key pressed where it does not mean "confirm" does not
 * confirm anything. A dialog that promotes a content because somebody
 * pressed the Enter key twice is a dialog that does not ask.
 */

import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { render } from '@/shared/test/test-utils';

import { ConfirmDialog } from '../../components/ConfirmDialog';

describe('ConfirmDialog', () => {
  it('puts the focus on the button that confirms', async () => {
    render(
      <ConfirmDialog
        open
        title="Promote"
        description="The content will be served as trusted."
        confirmLabel="Promote"
        onConfirm={() => {}}
        onClose={() => {}}
      />
    );

    const confirm = await screen.findByRole('button', { name: 'Promote' });
    expect(confirm).toHaveFocus();
  });

  it('confirms when the Enter key is on the button that confirms', async () => {
    const user = userEvent.setup();
    const confirmed = vi.fn();
    render(
      <ConfirmDialog
        open
        title="Promote"
        description="The content will be served as trusted."
        confirmLabel="Promote"
        onConfirm={confirmed}
        onClose={() => {}}
      />
    );

    await screen.findByRole('button', { name: 'Promote' });
    await user.keyboard('{Enter}');

    expect(confirmed).toHaveBeenCalled();
  });

  it('does not confirm when the Enter key is on the button that cancels', async () => {
    const user = userEvent.setup();
    const confirmed = vi.fn();
    render(
      <ConfirmDialog
        open
        title="Promote"
        description="The content will be served as trusted."
        confirmLabel="Promote"
        onConfirm={confirmed}
        onClose={() => {}}
      />
    );

    const cancel = await screen.findByRole('button', { name: 'Cancel' });
    cancel.focus();
    await user.keyboard('{Enter}');

    expect(confirmed).not.toHaveBeenCalled();
  });

  it('does not confirm when the Enter key is written in a field', async () => {
    const user = userEvent.setup();
    const confirmed = vi.fn();
    render(
      <ConfirmDialog
        open
        title="Revoke"
        description="The content will stop being served as trusted."
        confirmLabel="Revoke"
        destructive
        onConfirm={confirmed}
        onClose={() => {}}
      >
        <input aria-label="Reason" />
      </ConfirmDialog>
    );

    const reason = await screen.findByLabelText('Reason');
    expect(reason).toHaveFocus();
    await user.keyboard('the signature does not hold{Enter}');

    expect(confirmed).not.toHaveBeenCalled();
  });

  it('does not close or re-confirm while pending', async () => {
    const user = userEvent.setup();
    const confirmed = vi.fn();
    const closed = vi.fn();
    render(
      <ConfirmDialog
        open
        pending
        title="Promote"
        description="The content will be served as trusted."
        confirmLabel="Promote"
        onConfirm={confirmed}
        onClose={closed}
      />
    );

    const confirm = await screen.findByRole('button', { name: /Loading/ });
    const cancel = screen.getByRole('button', { name: 'Cancel' });
    expect(confirm).toBeDisabled();
    expect(cancel).toBeDisabled();

    await user.keyboard('{Escape}');
    expect(closed).not.toHaveBeenCalled();
    expect(confirmed).not.toHaveBeenCalled();
  });
});
