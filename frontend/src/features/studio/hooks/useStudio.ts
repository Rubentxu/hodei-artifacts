/**
 * The state of the Policy Studio.
 *
 * The hook holds what the editor holds (the text of the draft, the text of
 * the snapshots) and what the server answered, and it keeps them apart.
 * Three rules:
 *
 * 1. Nothing is decided here. Every answer is the server's.
 * 2. A refused action changes nothing: on a 403 the head, the revision
 *    and the draft stay as they were.
 * 3. A conflict never costs the draft. On `409 stale_head` the studio
 *    reads the head again, says where it is and keeps what was typed.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';

import * as api from '../api/studioApi';
import { studioFailureOf, type StudioFailure } from '../api/studioFailure';
import {
  defaultSnapshots,
  HISTORY_PAGE_SIZE,
  isStudioEditable,
  newDraft,
  type HeadResponse,
  type HistoryResponse,
  type SimulatedSubject,
  type SimulateResponse,
  type StudioPolicy,
  type ValidateResponse,
} from '../api/studioTypes';

/** What the first read of the studio came to. */
export type LoadState =
  | { status: 'loading' }
  | { status: 'ready' }
  | { status: 'denied'; failure: StudioFailure }
  | { status: 'failed'; failure: StudioFailure };

/** A line the studio says about what just happened. */
export interface Notice {
  tone: 'info' | 'warning';
  text: string;
}

const pretty = (value: unknown) => JSON.stringify(value, null, 2);

/** Parses the text of the editor, or says why it is not a value. */
const parse = <T>(
  text: string,
  what: string
): { value: T } | { error: string } => {
  try {
    return { value: JSON.parse(text) as T };
  } catch (error) {
    return {
      error: `${what} is not valid JSON: ${error instanceof Error ? error.message : 'unreadable'}`,
    };
  }
};

export const useStudio = () => {
  const [load, setLoad] = useState<LoadState>({ status: 'loading' });
  const [head, setHead] = useState<HeadResponse | null>(null);
  /** The saved revision the editor stands on: what an activation acts on. */
  const [revision, setRevision] = useState<number | null>(null);
  const [savedText, setSavedText] = useState('');
  const [draftText, setDraftText] = useState('');
  const [snapshotText, setSnapshotText] = useState(() =>
    pretty(defaultSnapshots())
  );
  const [reason, setReason] = useState('');
  const [inputError, setInputError] = useState<string | null>(null);
  const [validation, setValidation] = useState<ValidateResponse | null>(null);
  const [simulation, setSimulation] = useState<SimulateResponse | null>(null);
  const [failure, setFailure] = useState<StudioFailure | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(false);

  const [history, setHistory] = useState<HistoryResponse | null>(null);
  const [historyOffset, setHistoryOffset] = useState(0);
  const [historyFailure, setHistoryFailure] = useState<StudioFailure | null>(
    null
  );

  const fetchHistory = useCallback(async (offset: number) => {
    setHistoryFailure(null);
    try {
      const page = await api.loadHistory(offset, HISTORY_PAGE_SIZE);
      setHistory(page);
      setHistoryOffset(offset);
    } catch (error) {
      setHistoryFailure(studioFailureOf(error));
    }
  }, []);

  /** The first read: the head and the revision the editor starts from. */
  const reload = useCallback(async () => {
    setLoad({ status: 'loading' });
    try {
      const fresh = await api.loadHead();
      setHead(fresh);
      if (fresh.exists && fresh.head) {
        const loaded = await api.loadRevision(fresh.head.sequence);
        setRevision(loaded.sequence);
        setSavedText(pretty(loaded.policy));
        setDraftText(pretty(loaded.policy));
      } else {
        const blank = pretty(newDraft('default'));
        setRevision(null);
        setSavedText('');
        setDraftText(blank);
      }
      setLoad({ status: 'ready' });
      void fetchHistory(0);
    } catch (error) {
      const known = studioFailureOf(error);
      setLoad(
        known.kind === 'denied'
          ? { status: 'denied', failure: known }
          : { status: 'failed', failure: known }
      );
    }
  }, [fetchHistory]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const draftPolicy = useMemo(() => {
    const parsed = parse<StudioPolicy>(draftText, 'The draft');
    return 'value' in parsed ? parsed.value : null;
  }, [draftText]);

  const editable = draftPolicy === null || isStudioEditable(draftPolicy);
  const dirty = revision !== null && draftText !== savedText;

  /** Runs one action: it clears the last failure and says that it is busy. */
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setFailure(null);
    setNotice(null);
    try {
      await action();
    } catch (error) {
      setFailure(studioFailureOf(error));
    } finally {
      setBusy(false);
    }
  };

  const readDraft = (): StudioPolicy | null => {
    const parsed = parse<StudioPolicy>(draftText, 'The draft');
    if ('error' in parsed) {
      setInputError(parsed.error);
      return null;
    }
    setInputError(null);
    return parsed.value;
  };

  /**
   * Loads a revision of the history into the editor. It is how a principal
   * other than the author reaches the revision they have to approve: the
   * activation is always of the revision the editor stands on.
   */
  const open = (sequence: number) =>
    run(async () => {
      const loaded = await api.loadRevision(sequence);
      setRevision(loaded.sequence);
      setSavedText(pretty(loaded.policy));
      setDraftText(pretty(loaded.policy));
      setValidation(null);
      setSimulation(null);
      setInputError(null);
      setNotice({
        tone: 'info',
        text: `Revision ${loaded.sequence} loaded into the editor.`,
      });
    });

  const validate = () =>
    run(async () => {
      const policy = readDraft();
      if (!policy) return;
      setValidation(await api.validateDraft(policy));
    });

  const simulate = () =>
    run(async () => {
      const policy = readDraft();
      if (!policy) return;
      const subjects = parse<SimulatedSubject[]>(snapshotText, 'The snapshot');
      if ('error' in subjects) {
        setInputError(subjects.error);
        return;
      }
      setSimulation(await api.simulateDraft(policy, subjects.value));
    });

  const save = () =>
    run(async () => {
      const policy = readDraft();
      if (!policy) return;
      const saved = await api.saveRevision(
        policy,
        reason.trim() || 'edited in the studio'
      );
      setRevision(saved.revision.sequence);
      setSavedText(draftText);
      setNotice({
        tone: 'info',
        text: `Revision ${saved.revision.sequence} saved. It does not decide until it is activated.`,
      });
      void fetchHistory(historyOffset);
    });

  /** What a conflict does: read the head again and keep the draft. */
  const keepDraftOnConflict = async (failed: StudioFailure) => {
    setSimulation(null);
    setNotice({
      tone: 'warning',
      text:
        failed.foundHead !== undefined
          ? `The head moved: it is now at revision ${failed.foundHead}. Your draft was kept. Simulate again before you activate.`
          : 'The head moved. Your draft was kept. Simulate again before you activate.',
    });
    try {
      setHead(await api.loadHead());
    } catch {
      // The head could not be read again: the notice already says it moved.
    }
    void fetchHistory(historyOffset);
  };

  const expectedHead = head?.exists ? (head.head?.sequence ?? null) : null;

  /**
   * Moves the head: an activation or a rollback. Both carry the head the
   * editor saw, and both answer a moved head the same way.
   */
  const move = (
    call: () => Promise<{ head: { sequence: number } }>,
    said: (head: number) => string
  ) =>
    run(async () => {
      try {
        const answer = await call();
        setNotice({ tone: 'info', text: said(answer.head.sequence) });
        setHead(await api.loadHead());
        setSimulation(null);
        void fetchHistory(historyOffset);
      } catch (error) {
        const known = studioFailureOf(error);
        if (known.kind !== 'stale_head') throw error;
        await keepDraftOnConflict(known);
      }
    });

  const activate = () => {
    if (revision === null) return Promise.resolve();
    return move(
      () =>
        api.activateRevision(
          revision,
          expectedHead,
          reason.trim() || `activated revision ${revision} from the studio`
        ),
      sequence => `Revision ${sequence} is the head. It decides from now on.`
    );
  };

  const rollback = (target: number) =>
    move(
      () =>
        api.rollbackTo(
          target,
          expectedHead,
          reason.trim() || `rolled back to revision ${target} from the studio`
        ),
      sequence =>
        `Rolled back to revision ${target}: revision ${sequence} is the head.`
    );

  return {
    load,
    head,
    revision,
    draftText,
    setDraftText: (text: string) => {
      setDraftText(text);
      setValidation(null);
      setInputError(null);
    },
    snapshotText,
    setSnapshotText,
    reason,
    setReason,
    editable,
    dirty,
    inputError,
    validation,
    simulation,
    failure,
    notice,
    busy,
    history,
    historyOffset,
    historyFailure,
    fetchHistory,
    reload,
    open,
    validate,
    simulate,
    save,
    activate,
    rollback,
  };
};
