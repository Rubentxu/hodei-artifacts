/**
 * The Policy Studio: the effects of a trust policy, measured by the server.
 *
 * The studio edits a *draft*, asks the server to validate it, asks the
 * server to simulate it against an evidence snapshot and shows what the
 * server answered. Four rules shape the view:
 *
 * 1. Nothing is decided in the browser. The editor holds text; the
 *    answers are the server's.
 * 2. The four refusals look different. A decision of the policy, a gap of
 *    evidence, an error of the engine and a denial of access (403) are
 *    different things and each has its own panel.
 * 3. The view offers fields only for what the backend has. There is no
 *    shadow mode, no choice of editor or approver and no account: who
 *    edits and who approves is the principal of the token.
 * 4. A denial changes nothing and a conflict loses nothing. The 403 of
 *    the first read closes the studio; the 403 of an action leaves the
 *    state as it was; a moved head keeps the draft.
 */

import { useState } from 'react';
import { AlertTriangle, CircleHelp, Gavel } from 'lucide-react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { ConfirmDialog } from '@/features/console/components/ConfirmDialog';
import type { Failure } from '@/features/console/api/errors';
import { FailureNotice } from '@/features/console/components/FailureNotice';

import type { StudioFailure } from '../api/studioFailure';
import { useStudio } from '../hooks/useStudio';
import type {
  Outcome,
  SimulatedEffect,
  StoredRevision,
} from '../api/studioTypes';
import { AccessDenied } from './AccessDenied';

const OUTCOME_VARIANT = {
  ALLOW: 'success',
  DENY: 'danger',
  WARN: 'warning',
  REQUIRE_APPROVAL: 'info',
  INDETERMINATE: 'secondary',
} as const satisfies Record<
  Outcome,
  'success' | 'danger' | 'warning' | 'info' | 'secondary'
>;

/** The failure of the studio, in the kinds the console notice knows. */
const asConsoleFailure = (failure: StudioFailure): Failure => ({
  kind:
    failure.kind === 'stale_head' ||
    failure.kind === 'rollback_to_head' ||
    failure.kind === 'history_full'
      ? 'conflict'
      : failure.kind === 'too_large' || failure.kind === 'not_implemented'
        ? 'invalid'
        : failure.kind === 'denied'
          ? 'permission'
          : failure.kind,
  message: failure.message,
  status: failure.status,
});

/** A list of identifiers as text, or what to say when there is none. */
const listOf = (ids: string[] | undefined, none: string) =>
  ids && ids.length > 0 ? ids.join(', ') : none;

const shortDigest = (digest: string) => `${digest.slice(0, 18)}…`;

/** One subject's answer, in the panel of its kind. */
const EffectRow = ({ effect }: { effect: SimulatedEffect }) => {
  if (effect.kind === 'EVIDENCE_GAP') {
    return (
      <li
        data-testid={`effect-${effect.subject}`}
        data-kind="EVIDENCE_GAP"
        className="rounded-md border border-amber-300 bg-amber-50 p-3"
      >
        <div className="flex items-center gap-2">
          <CircleHelp className="h-4 w-4 text-amber-700" aria-hidden="true" />
          <span className="text-sm font-medium">{effect.subject}</span>
          <Badge variant="warning" data-testid={`kind-${effect.subject}`}>
            Evidence gap
          </Badge>
        </div>
        <p className="mt-1 text-xs text-amber-900">
          The requirement was never measured: the evidence it waits on is
          absent. This is not a refusal of the artifact.
        </p>
        <p className="mt-1 text-xs text-gray-700">
          {effect.gap?.requirement} · {effect.gap?.reason} · waiting on{' '}
          {listOf(effect.gap?.evidence_ids, 'no evidence id')}
        </p>
      </li>
    );
  }
  if (effect.kind === 'ENGINE_ERROR') {
    return (
      <li
        data-testid={`effect-${effect.subject}`}
        data-kind="ENGINE_ERROR"
        className="rounded-md border border-red-300 bg-red-50 p-3"
      >
        <div className="flex items-center gap-2">
          <AlertTriangle className="h-4 w-4 text-red-700" aria-hidden="true" />
          <span className="text-sm font-medium">{effect.subject}</span>
          <Badge variant="danger" data-testid={`kind-${effect.subject}`}>
            Engine error
          </Badge>
        </div>
        <p className="mt-1 text-xs text-red-900">
          The engine could not answer, so nobody said no. A malformed policy or
          snapshot never denies.
        </p>
        <p className="mt-1 text-xs text-gray-700">{effect.error}</p>
      </li>
    );
  }
  const outcome = effect.decision?.outcome ?? 'INDETERMINATE';
  return (
    <li
      data-testid={`effect-${effect.subject}`}
      data-kind="POLICY_DECISION"
      className="rounded-md border border-gray-300 bg-white p-3"
    >
      <div className="flex items-center gap-2">
        <Gavel className="h-4 w-4 text-gray-700" aria-hidden="true" />
        <span className="text-sm font-medium">{effect.subject}</span>
        <Badge
          variant={OUTCOME_VARIANT[outcome]}
          data-testid={`kind-${effect.subject}`}
        >
          Policy decision · {outcome}
        </Badge>
      </div>
      <p className="mt-1 text-xs text-gray-700">
        policy {effect.decision?.policy_id} v{effect.decision?.policy_version} ·
        evidence {listOf(effect.decision?.evidence_ids, 'none')}
      </p>
    </li>
  );
};

/** One row of the history of revisions. */
const RevisionRow = ({
  revision,
  isHead,
  onOpen,
  onRollback,
  disabled,
}: {
  revision: StoredRevision;
  isHead: boolean;
  onOpen: (sequence: number) => void;
  onRollback: (sequence: number) => void;
  disabled: boolean;
}) => (
  <li
    data-testid={`revision-${revision.sequence}`}
    className="flex flex-wrap items-center gap-2 border-b py-2 text-sm"
  >
    <span className="font-medium">Revision {revision.sequence}</span>
    {isHead && <Badge variant="success">head</Badge>}
    <span className="text-xs text-gray-600">
      saved by {revision.saved_by} · {shortDigest(revision.digest)} ·{' '}
      {revision.reason}
    </span>
    {revision.rolled_back_by && (
      <span
        className="text-xs text-gray-600"
        data-testid={`rolled-back-by-${revision.sequence}`}
      >
        rolled back by {revision.rolled_back_by}
      </span>
    )}
    <Button
      variant="outline"
      size="sm"
      className="ml-auto"
      data-testid={`open-${revision.sequence}`}
      disabled={disabled}
      onClick={() => onOpen(revision.sequence)}
    >
      Open
    </Button>
    {!isHead && (
      <Button
        variant="outline"
        size="sm"
        data-testid={`rollback-${revision.sequence}`}
        disabled={disabled}
        onClick={() => onRollback(revision.sequence)}
      >
        Roll back to {revision.sequence}
      </Button>
    )}
  </li>
);

export const PolicyStudioView = () => {
  const studio = useStudio();
  const [confirm, setConfirm] = useState<
    { kind: 'activate' } | { kind: 'rollback'; target: number } | null
  >(null);

  if (studio.load.status === 'loading') {
    return (
      <p className="text-sm text-gray-600" data-testid="studio-loading">
        Loading the Policy Studio…
      </p>
    );
  }
  if (studio.load.status === 'denied') {
    // The gate closed the studio: no editor, no button, no stale data.
    return (
      <div data-testid="policy-studio">
        <AccessDenied scope="studio" detail={studio.load.failure.message} />
      </div>
    );
  }
  if (studio.load.status === 'failed') {
    return (
      <div className="flex flex-col gap-3" data-testid="policy-studio">
        <div data-testid="studio-failure">
          <FailureNotice failure={asConsoleFailure(studio.load.failure)} />
        </div>
        <div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void studio.reload()}
          >
            Try again
          </Button>
        </div>
      </div>
    );
  }

  const head = studio.head?.exists ? studio.head.head : undefined;
  const failure = studio.failure;

  return (
    <div className="flex flex-col gap-6" data-testid="policy-studio">
      <section className="rounded-md border p-4">
        <h2 className="text-sm font-semibold">The set that decides</h2>
        <p className="mt-1 text-sm text-gray-600" data-testid="studio-head">
          {head
            ? `Revision ${head.sequence} (${shortDigest(head.digest)}) activated by ${head.activated_by}, edited by ${head.editor}`
            : 'Never activated: the studio measures against no baseline.'}
        </p>
        {studio.revision !== null && (
          <p
            className="mt-1 text-sm text-gray-600"
            data-testid="studio-editing"
          >
            Editing revision {studio.revision}
            {studio.dirty ? ' (the draft has unsaved changes)' : ''}
          </p>
        )}
        <p className="mt-1 text-xs text-gray-500">
          The editor and the approver are the principal you signed in with.
        </p>
      </section>

      {failure &&
        (failure.kind === 'denied' ? (
          <AccessDenied
            scope="action"
            detail={failure.message}
            testId="studio-action-denied"
          />
        ) : (
          <div data-testid="studio-failure">
            <FailureNotice failure={asConsoleFailure(failure)} />
          </div>
        ))}
      {studio.notice && (
        <p
          role="status"
          data-testid="studio-notice"
          className={
            studio.notice.tone === 'warning'
              ? 'rounded-md border border-amber-300 bg-amber-50 p-3 text-sm text-amber-900'
              : 'rounded-md bg-gray-100 p-3 text-sm'
          }
        >
          {studio.notice.text}
        </p>
      )}

      <section className="flex flex-col gap-3">
        <h2 className="text-sm font-semibold">The draft</h2>
        {!studio.editable && (
          <p
            className="text-sm text-amber-700"
            data-testid="studio-not-editable"
          >
            This policy has requirements of a kind the studio does not speak. It
            is read only here.
          </p>
        )}
        <textarea
          aria-label="Policy draft"
          data-testid="studio-editor"
          className="h-64 w-full rounded-md border p-3 font-mono text-sm"
          value={studio.draftText}
          readOnly={!studio.editable}
          onChange={event => studio.setDraftText(event.target.value)}
        />
        {studio.inputError && (
          <p
            role="alert"
            data-testid="studio-input-error"
            className="text-sm text-red-700"
          >
            {studio.inputError}
          </p>
        )}

        <h2 className="text-sm font-semibold">The evidence snapshots</h2>
        <p className="text-xs text-gray-600">
          The simulation measures the draft against these snapshots. They are
          the evidence you bring; the server does the measuring.
        </p>
        <textarea
          aria-label="Evidence snapshots"
          data-testid="studio-snapshots"
          className="h-40 w-full rounded-md border p-3 font-mono text-xs"
          value={studio.snapshotText}
          onChange={event => studio.setSnapshotText(event.target.value)}
        />

        <label className="text-sm text-gray-700">
          Reason
          <input
            aria-label="Reason"
            data-testid="studio-reason"
            className="mt-1 w-full rounded-md border p-2 text-sm"
            value={studio.reason}
            onChange={event => studio.setReason(event.target.value)}
          />
        </label>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            data-testid="studio-validate"
            disabled={studio.busy || !studio.editable}
            onClick={() => void studio.validate()}
          >
            Validate
          </Button>
          <Button
            variant="outline"
            size="sm"
            data-testid="studio-simulate"
            disabled={studio.busy || !studio.editable}
            onClick={() => void studio.simulate()}
          >
            Simulate
          </Button>
          <Button
            variant="secondary"
            size="sm"
            data-testid="studio-save"
            disabled={studio.busy || !studio.editable}
            onClick={() => void studio.save()}
          >
            Save revision
          </Button>
          {/* The activation is the one of a saved revision: not of a
              draft and not of a simulation. */}
          <Button
            variant="primary"
            size="sm"
            data-testid="studio-activate"
            disabled={studio.busy || studio.revision === null}
            onClick={() => setConfirm({ kind: 'activate' })}
          >
            {`Activate revision ${studio.revision ?? '—'}`}
          </Button>
        </div>
      </section>

      {studio.validation && (
        <section
          className="rounded-md border p-4"
          data-testid="studio-validation"
        >
          <h2 className="text-sm font-semibold">Validation</h2>
          <p className="mt-1 text-sm" data-testid="studio-validation-answer">
            {studio.validation.valid
              ? 'The store would accept this draft as a revision.'
              : `The draft is refused: ${studio.validation.error ?? 'unknown reason'}`}
          </p>
        </section>
      )}

      {studio.simulation && (
        <section className="rounded-md border p-4" data-testid="studio-effects">
          <h2 className="text-sm font-semibold">Effects</h2>
          <p
            className="mt-1 text-xs text-gray-500"
            data-testid="studio-simulated"
          >
            Measured by the server · applied:{' '}
            {studio.simulation.applied ? 'yes' : 'no'} · active version:{' '}
            {studio.simulation.active_version ?? 'none'}. A simulation changes
            nothing.
          </p>
          <ul className="mt-3 flex flex-col gap-2">
            {studio.simulation.effects.map(effect => (
              <EffectRow key={effect.subject} effect={effect} />
            ))}
          </ul>
          <h3 className="mt-4 text-sm font-semibold">
            Diff against the active policy
          </h3>
          {studio.simulation.changes.length > 0 ? (
            <ul className="mt-1" data-testid="studio-changes">
              {studio.simulation.changes.map(change => (
                <li
                  key={change.subject}
                  className="text-sm"
                  data-testid={`change-${change.subject}`}
                >
                  {change.subject}: {change.before} → {change.after}
                  {change.loosens ? ' (loosens)' : ''}
                </li>
              ))}
            </ul>
          ) : (
            <p
              className="mt-1 text-sm text-gray-600"
              data-testid="studio-no-changes"
            >
              No subject changes its outcome.
            </p>
          )}
        </section>
      )}

      <section className="rounded-md border p-4" data-testid="studio-history">
        <h2 className="text-sm font-semibold">History</h2>
        {studio.historyFailure &&
          (studio.historyFailure.kind === 'denied' ? (
            <AccessDenied
              scope="action"
              detail={studio.historyFailure.message}
              testId="studio-history-denied"
            />
          ) : (
            <p
              role="alert"
              className="text-sm text-red-700"
              data-testid="studio-history-failure"
            >
              The history cannot be read: {studio.historyFailure.message}
            </p>
          ))}
        {studio.history && (
          <>
            {studio.history.revisions.length === 0 ? (
              <p className="mt-1 text-sm text-gray-600">
                No revision in this window.
              </p>
            ) : (
              <ul className="mt-2">
                {studio.history.revisions.map(revision => (
                  <RevisionRow
                    key={revision.sequence}
                    revision={revision}
                    isHead={head?.sequence === revision.sequence}
                    disabled={studio.busy}
                    onOpen={sequence => void studio.open(sequence)}
                    onRollback={target =>
                      setConfirm({ kind: 'rollback', target })
                    }
                  />
                ))}
              </ul>
            )}
            <h3 className="mt-3 text-sm font-semibold">Events</h3>
            <ul className="mt-1" data-testid="studio-events">
              {studio.history.events.map(event => (
                <li key={event.id} className="text-xs text-gray-700">
                  {event.kind} · revision {event.sequence} · by {event.actor}{' '}
                  (editor {event.editor})
                  {event.rolled_back_by
                    ? ` · rolled back by ${event.rolled_back_by}`
                    : ''}{' '}
                  · {event.reason}
                </li>
              ))}
            </ul>
            {(studio.history.revisions_truncated ||
              studio.history.events_truncated) && (
              <p
                className="mt-2 text-xs text-amber-800"
                data-testid="studio-truncated"
              >
                The window is cut: there is more history after it.
              </p>
            )}
            <div className="mt-2 flex gap-2">
              <Button
                variant="outline"
                size="sm"
                data-testid="history-previous"
                disabled={studio.historyOffset === 0}
                onClick={() =>
                  void studio.fetchHistory(
                    Math.max(0, studio.historyOffset - studio.history!.limit)
                  )
                }
              >
                Previous
              </Button>
              <Button
                variant="outline"
                size="sm"
                data-testid="history-next"
                disabled={
                  !studio.history.revisions_truncated &&
                  !studio.history.events_truncated
                }
                onClick={() =>
                  void studio.fetchHistory(
                    studio.historyOffset + studio.history!.limit
                  )
                }
              >
                Next
              </Button>
            </div>
          </>
        )}
      </section>

      <ConfirmDialog
        open={confirm?.kind === 'activate'}
        title={`Activate revision ${studio.revision ?? ''}?`}
        description="This changes what decides promotions for your account. The store refuses it if the head moved since you loaded it."
        confirmLabel={`Confirm: activate revision ${studio.revision ?? ''}`}
        pending={studio.busy}
        onClose={() => setConfirm(null)}
        onConfirm={() => {
          setConfirm(null);
          void studio.activate();
        }}
      />
      <ConfirmDialog
        open={confirm?.kind === 'rollback'}
        title={`Roll back to revision ${confirm?.kind === 'rollback' ? confirm.target : ''}?`}
        description="The content of that revision is saved again as a new revision and becomes the head."
        confirmLabel="Confirm: roll back"
        destructive
        pending={studio.busy}
        onClose={() => setConfirm(null)}
        onConfirm={() => {
          const target = confirm?.kind === 'rollback' ? confirm.target : null;
          setConfirm(null);
          if (target !== null) void studio.rollback(target);
        }}
      />
    </div>
  );
};

export default PolicyStudioView;
